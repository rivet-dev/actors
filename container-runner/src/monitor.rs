//! Periodic instance resource monitor.
//!
//! Opt-in via the [`ENABLE_ENV`] environment variable. When enabled it samples
//! memory and CPU usage every [`sample_interval`] and logs them, so memory
//! growth toward the limit (and the OOM that follows) is visible in the logs at
//! fine granularity. Disabled by default so nothing is logged unless explicitly
//! turned on.
//!
//! Memory and CPU are detected independently, because the gVisor sandbox
//! exposes cgroup v1 memory but no cgroup v2 or cgroup v1 CPU accounting, so the
//! two counters legitimately come from different sources.
//!
//! Only memory *usage* is reported, not a limit or percentage: under gVisor both
//! the cgroup `memory.limit_in_bytes` and `/proc/meminfo` report the sandbox size
//! rather than the container's configured limit, so any percentage would be
//! misleading.
//!
//! Memory sources, in preference order:
//!   - **cgroup v2** `memory.current` (real Linux). Exact.
//!   - **cgroup v1** `memory/memory.usage_in_bytes` (gVisor sandbox).
//!     Container-wide usage (all processes plus page cache).
//!   - **`/proc/meminfo`** last resort. Under gVisor this reflects the whole
//!     sandbox, not the container, so it is only an approximation.
//!
//! CPU sources, in preference order:
//!   - **cgroup v2** `cpu.stat`.
//!   - **`/proc/stat`**, the gVisor sandbox fallback (sandbox-wide, approximate).
//!
//! If neither a memory nor a CPU source is readable the monitor logs once and
//! disables itself.

use std::time::{Duration, Instant};

use tokio::time::{MissedTickBehavior, interval};

/// Environment variable that enables the monitor. Unset or a falsey value means
/// the monitor does not run and nothing is logged. Truthy values are `1`,
/// `true`, `yes`, and `on` (case-insensitive).
const ENABLE_ENV: &str = "RIVET_LOG_RESOURCE_USAGE";

/// Default cadence for sampling and logging resource usage, when
/// [`INTERVAL_ENV`] is unset.
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

/// Environment variable overriding the sample/log interval, in milliseconds.
/// Unset, unparseable, or `0` falls back to [`DEFAULT_SAMPLE_INTERVAL`].
const INTERVAL_ENV: &str = "RIVET_RESOURCE_USAGE_INTERVAL_MS";

/// cgroup v2 memory usage (real Linux).
const MEMORY_CURRENT_V2: &str = "/sys/fs/cgroup/memory.current";
/// cgroup v1 memory usage (gVisor sandbox). Container-wide, includes page
/// cache. The sibling `memory.limit_in_bytes` is deliberately not read: under
/// gVisor it reports the sandbox size, not the configured limit.
const MEMORY_USAGE_V1: &str = "/sys/fs/cgroup/memory/memory.usage_in_bytes";
const CPU_STAT_V2: &str = "/sys/fs/cgroup/cpu.stat";

const PROC_MEMINFO: &str = "/proc/meminfo";
const PROC_STAT: &str = "/proc/stat";

/// Kernel clock ticks per second, the unit of `/proc/stat` jiffies. 100 on Linux
/// and gVisor.
const USER_HZ: u64 = 100;

/// Where the monitor reads memory counters from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemSource {
	/// cgroup v2 `memory.current` (real Linux).
	CgroupV2,
	/// cgroup v1 `memory.usage_in_bytes` (gVisor sandbox).
	CgroupV1,
	/// `/proc/meminfo` (sandbox-wide approximation).
	Proc,
}

impl MemSource {
	fn label(self) -> &'static str {
		match self {
			MemSource::CgroupV2 => "cgroup_v2",
			MemSource::CgroupV1 => "cgroup_v1",
			MemSource::Proc => "proc",
		}
	}
}

/// Where the monitor reads CPU counters from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CpuSource {
	/// cgroup v2 `cpu.stat` (real Linux).
	CgroupV2,
	/// `/proc/stat` (gVisor sandbox, which does not expose cgroup CPU).
	Proc,
}

impl CpuSource {
	fn label(self) -> &'static str {
		match self {
			CpuSource::CgroupV2 => "cgroup_v2",
			CpuSource::Proc => "proc",
		}
	}
}

/// Spawn the background resource monitor if enabled via [`ENABLE_ENV`]. A no-op
/// when disabled (the default). Fire-and-forget for the process lifetime; it
/// runs until the process exits.
pub fn spawn_resource_monitor() {
	if !monitor_enabled() {
		return;
	}
	let sample_interval = sample_interval();
	tracing::info!(
		interval_ms = sample_interval.as_millis() as u64,
		"resource monitor enabled"
	);
	tokio::spawn(run_monitor(sample_interval));
}

/// The configured sample/log interval: [`INTERVAL_ENV`] in milliseconds when set
/// to a positive integer, otherwise [`DEFAULT_SAMPLE_INTERVAL`].
fn sample_interval() -> Duration {
	match std::env::var(INTERVAL_ENV)
		.ok()
		.and_then(|value| value.trim().parse::<u64>().ok())
	{
		Some(ms) if ms > 0 => Duration::from_millis(ms),
		_ => DEFAULT_SAMPLE_INTERVAL,
	}
}

fn monitor_enabled() -> bool {
	match std::env::var(ENABLE_ENV) {
		Ok(value) => matches!(
			value.trim().to_ascii_lowercase().as_str(),
			"1" | "true" | "yes" | "on"
		),
		Err(_) => false,
	}
}

/// Whether the monitor is turned on via [`ENABLE_ENV`]. Exposed so the actor can
/// report the monitor's status tagged with its id (process-level monitor logs
/// are otherwise invisible in actor-scoped log views).
pub fn enabled() -> bool {
	monitor_enabled()
}

/// The memory counter source the monitor would use right now: `"cgroup_v2"`,
/// `"cgroup_v1"`, `"proc"`, or `"none"`. Exposed for the actor-scoped status log.
/// Memory is the monitor's headline signal, so this reports the memory source.
pub fn sampling_source() -> &'static str {
	match detect_mem_source() {
		Some(source) => source.label(),
		None => "none",
	}
}

/// Pick the memory source, preferring exact cgroup v2, then cgroup v1 (gVisor
/// sandbox), then the sandbox-wide `/proc/meminfo` approximation.
fn detect_mem_source() -> Option<MemSource> {
	if read_u64(MEMORY_CURRENT_V2).is_some() {
		Some(MemSource::CgroupV2)
	} else if read_u64(MEMORY_USAGE_V1).is_some() {
		Some(MemSource::CgroupV1)
	} else if read_proc_memory().is_some() {
		Some(MemSource::Proc)
	} else {
		None
	}
}

/// Pick the CPU source, preferring cgroup v2 over the `/proc/stat` fallback.
fn detect_cpu_source() -> Option<CpuSource> {
	if read_cgroup_cpu_usage_usec().is_some() {
		Some(CpuSource::CgroupV2)
	} else if read_proc_cpu_busy_usec().is_some() {
		Some(CpuSource::Proc)
	} else {
		None
	}
}

async fn run_monitor(sample_interval: Duration) {
	let mem_source = detect_mem_source();
	let cpu_source = detect_cpu_source();
	if mem_source.is_none() && cpu_source.is_none() {
		tracing::warn!(
			cgroup_dir = "/sys/fs/cgroup",
			proc_meminfo = PROC_MEMINFO,
			proc_stat = PROC_STAT,
			"resource monitor disabled: no readable memory or cpu counters"
		);
		return;
	}
	tracing::info!(
		mem_source = mem_source.map(MemSource::label).unwrap_or("none"),
		cpu_source = cpu_source.map(CpuSource::label).unwrap_or("none"),
		"resource monitor sampling"
	);

	let mut ticker = interval(sample_interval);
	// A slow sample must not make the monitor try to catch up with a burst of
	// back-to-back ticks; just resume on the next boundary.
	ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
	// The first tick fires immediately; consume it so the first logged line
	// already covers a full interval of CPU time.
	ticker.tick().await;

	let mut prev_cpu_usec = cpu_source.and_then(cpu_busy_usec);
	let mut prev_at = Instant::now();

	loop {
		ticker.tick().await;
		let now = Instant::now();
		let elapsed = now.duration_since(prev_at);

		let cpu_now_usec = cpu_source.and_then(cpu_busy_usec);
		// CPU time used over the interval, divided by wall time, is the number of
		// vCPU cores consumed (1.0 == one core fully busy).
		let cpu_cores = match (prev_cpu_usec, cpu_now_usec) {
			(Some(prev), Some(cur)) if !elapsed.is_zero() => {
				Some(cur.saturating_sub(prev) as f64 / elapsed.as_micros() as f64)
			}
			_ => None,
		};
		prev_cpu_usec = cpu_now_usec;
		prev_at = now;

		// Only log while an actor is running, and attribute the sample to it so it
		// shows up in that actor's logs. The counters are instance-wide; with more
		// than one actor on the instance the same sample is logged for each.
		let actor_ids = crate::active_actor_ids().await;
		if actor_ids.is_empty() {
			continue;
		}

		let mem_used_mib = mem_source.and_then(memory_used_bytes).map(bytes_to_mib);

		for actor_id in actor_ids {
			tracing::info!(
				actor_id = %actor_id,
				mem_source = mem_source.map(MemSource::label).unwrap_or("none"),
				cpu_source = cpu_source.map(CpuSource::label).unwrap_or("none"),
				mem_used_mib = ?mem_used_mib,
				cpu_cores = ?cpu_cores,
				"instance resource usage"
			);
		}
	}
}

/// Current memory usage in bytes for the selected source.
fn memory_used_bytes(source: MemSource) -> Option<u64> {
	match source {
		MemSource::CgroupV2 => read_u64(MEMORY_CURRENT_V2),
		MemSource::CgroupV1 => read_u64(MEMORY_USAGE_V1),
		MemSource::Proc => read_proc_memory(),
	}
}

/// Cumulative busy CPU time in microseconds for the selected source.
fn cpu_busy_usec(source: CpuSource) -> Option<u64> {
	match source {
		CpuSource::CgroupV2 => read_cgroup_cpu_usage_usec(),
		CpuSource::Proc => read_proc_cpu_busy_usec(),
	}
}

/// Read a pseudo-file holding a single unsigned integer. These are in-memory
/// kernel files, so the synchronous read does not block meaningfully.
fn read_u64(path: &str) -> Option<u64> {
	std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Cumulative CPU time consumed by the cgroup, in microseconds, from the
/// `usage_usec` line of `cpu.stat`.
fn read_cgroup_cpu_usage_usec() -> Option<u64> {
	let stat = std::fs::read_to_string(CPU_STAT_V2).ok()?;
	stat.lines()
		.find_map(|line| line.strip_prefix("usage_usec "))
		.and_then(|value| value.trim().parse().ok())
}

/// Used memory in bytes from `/proc/meminfo` (`MemTotal - MemAvailable`). Under
/// gVisor this reflects the whole sandbox, not the container.
fn read_proc_memory() -> Option<u64> {
	let total_kb = read_meminfo_kb("MemTotal")?;
	let available_kb = read_meminfo_kb("MemAvailable")?;
	Some(total_kb.saturating_sub(available_kb) * 1024)
}

/// Value in kB of a `/proc/meminfo` key such as `"MemTotal"`.
fn read_meminfo_kb(key: &str) -> Option<u64> {
	let content = std::fs::read_to_string(PROC_MEMINFO).ok()?;
	content.lines().find_map(|line| {
		let rest = line.strip_prefix(key)?;
		rest.trim_start_matches(':')
			.split_whitespace()
			.next()?
			.parse()
			.ok()
	})
}

/// Cumulative busy CPU time in microseconds from the aggregate `cpu` line of
/// `/proc/stat` (`total - idle - iowait`, converted from jiffies).
fn read_proc_cpu_busy_usec() -> Option<u64> {
	let content = std::fs::read_to_string(PROC_STAT).ok()?;
	let line = content.lines().next()?;
	let mut fields = line.split_whitespace();
	if fields.next()? != "cpu" {
		return None;
	}
	let values: Vec<u64> = fields.filter_map(|value| value.parse().ok()).collect();
	if values.len() < 4 {
		return None;
	}
	let total: u64 = values.iter().sum();
	// Fields are user, nice, system, idle, iowait, ... Treat idle + iowait as idle.
	let idle = values[3] + values.get(4).copied().unwrap_or(0);
	let busy = total.saturating_sub(idle);
	Some(busy * (1_000_000 / USER_HZ))
}

fn bytes_to_mib(bytes: u64) -> f64 {
	bytes as f64 / (1024.0 * 1024.0)
}
