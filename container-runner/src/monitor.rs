//! Periodic instance resource monitor.
//!
//! Opt-in via the [`ENABLE_ENV`] environment variable. When enabled it samples
//! memory and CPU usage every [`SAMPLE_INTERVAL`] and logs them, so memory
//! growth toward the limit (and the OOM that follows) is visible in the logs at
//! fine granularity. Disabled by default so nothing is logged unless explicitly
//! turned on.
//!
//! Two counter sources are supported, picked at startup:
//!   - **cgroup v2** under `/sys/fs/cgroup` (real Linux). Exact against the
//!     container limits.
//!   - **`/proc`** (`/proc/meminfo`, `/proc/stat`), the fallback for a gVisor
//!     sandbox, which does not expose cgroup v2. Values reflect the sandbox and
//!     are approximate.
//! If neither is readable the monitor logs once and disables itself.

use std::time::{Duration, Instant};

use tokio::time::{MissedTickBehavior, interval};

/// Environment variable that enables the monitor. Unset or a falsey value means
/// the monitor does not run and nothing is logged. Truthy values are `1`,
/// `true`, `yes`, and `on` (case-insensitive).
const ENABLE_ENV: &str = "RIVET_LOG_RESOURCE_USAGE";

/// How often to sample and log resource usage.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

const MEMORY_CURRENT: &str = "/sys/fs/cgroup/memory.current";
const MEMORY_MAX: &str = "/sys/fs/cgroup/memory.max";
const CPU_STAT: &str = "/sys/fs/cgroup/cpu.stat";
const CPU_MAX: &str = "/sys/fs/cgroup/cpu.max";

const PROC_MEMINFO: &str = "/proc/meminfo";
const PROC_STAT: &str = "/proc/stat";

/// Kernel clock ticks per second, the unit of `/proc/stat` jiffies. 100 on Linux
/// and gVisor.
const USER_HZ: u64 = 100;

/// Where the monitor reads counters from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
	/// cgroup v2 under `/sys/fs/cgroup` (real Linux).
	CgroupV2,
	/// `/proc` (gVisor sandbox, which does not expose cgroup v2).
	Proc,
}

impl Source {
	fn label(self) -> &'static str {
		match self {
			Source::CgroupV2 => "cgroup_v2",
			Source::Proc => "proc",
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
	tracing::info!(interval_ms = SAMPLE_INTERVAL.as_millis() as u64, "resource monitor enabled");
	tokio::spawn(run_monitor());
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

/// The counter source the monitor would use right now: `"cgroup_v2"`, `"proc"`,
/// or `"none"` when neither is readable. Exposed for the actor-scoped status log.
pub fn sampling_source() -> &'static str {
	match detect_source() {
		Some(source) => source.label(),
		None => "none",
	}
}

/// Pick the counter source, preferring exact cgroup v2 over the `/proc` fallback.
fn detect_source() -> Option<Source> {
	if read_u64(MEMORY_CURRENT).is_some() && read_cgroup_cpu_usage_usec().is_some() {
		Some(Source::CgroupV2)
	} else if read_proc_memory().is_some() && read_proc_cpu_busy_usec().is_some() {
		Some(Source::Proc)
	} else {
		None
	}
}

async fn run_monitor() {
	let Some(source) = detect_source() else {
		tracing::warn!(
			cgroup_dir = "/sys/fs/cgroup",
			proc_meminfo = PROC_MEMINFO,
			proc_stat = PROC_STAT,
			"resource monitor disabled: neither cgroup v2 nor /proc counters are readable"
		);
		return;
	};
	tracing::info!(source = source.label(), "resource monitor sampling");

	let mut ticker = interval(SAMPLE_INTERVAL);
	// A slow sample must not make the monitor try to catch up with a burst of
	// back-to-back ticks; just resume on the next boundary.
	ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
	// The first tick fires immediately; consume it so the first logged line
	// already covers a full interval of CPU time.
	ticker.tick().await;

	// Limits are fixed for the instance lifetime, so read them once.
	let mem_limit_mib = memory_limit_bytes(source).map(bytes_to_mib);
	let cpu_limit_cores = cpu_limit_cores(source);

	let mut prev_cpu_usec = cpu_busy_usec(source);
	let mut prev_at = Instant::now();

	loop {
		ticker.tick().await;
		let now = Instant::now();
		let elapsed = now.duration_since(prev_at);

		let cpu_now_usec = cpu_busy_usec(source);
		// CPU time used over the interval, divided by wall time, is the number of
		// vCPU cores consumed (1.0 == one core fully busy).
		let cpu_cores = match (prev_cpu_usec, cpu_now_usec) {
			(Some(prev), Some(cur)) if !elapsed.is_zero() => {
				cur.saturating_sub(prev) as f64 / elapsed.as_micros() as f64
			}
			_ => 0.0,
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

		let mem_used_mib = memory_used_bytes(source).map(bytes_to_mib);
		let mem_pct = match (mem_used_mib, mem_limit_mib) {
			(Some(used), Some(limit)) if limit > 0.0 => Some(used / limit * 100.0),
			_ => None,
		};
		let cpu_pct = match cpu_limit_cores {
			Some(limit) if limit > 0.0 => Some(cpu_cores / limit * 100.0),
			_ => None,
		};

		for actor_id in actor_ids {
			tracing::info!(
				actor_id = %actor_id,
				source = source.label(),
				mem_used_mib = ?mem_used_mib,
				mem_limit_mib = ?mem_limit_mib,
				mem_pct = ?mem_pct,
				cpu_cores,
				cpu_limit_cores = ?cpu_limit_cores,
				cpu_pct = ?cpu_pct,
				"instance resource usage"
			);
		}
	}
}

/// Current memory usage in bytes for the selected source.
fn memory_used_bytes(source: Source) -> Option<u64> {
	match source {
		Source::CgroupV2 => read_u64(MEMORY_CURRENT),
		Source::Proc => read_proc_memory().map(|(used, _limit)| used),
	}
}

/// Memory limit in bytes for the selected source, or `None` when unlimited.
fn memory_limit_bytes(source: Source) -> Option<u64> {
	match source {
		Source::CgroupV2 => read_cgroup_memory_max(),
		Source::Proc => read_proc_memory().map(|(_used, limit)| limit),
	}
}

/// Cumulative busy CPU time in microseconds for the selected source.
fn cpu_busy_usec(source: Source) -> Option<u64> {
	match source {
		Source::CgroupV2 => read_cgroup_cpu_usage_usec(),
		Source::Proc => read_proc_cpu_busy_usec(),
	}
}

/// CPU limit in vCPU cores for the selected source, or `None` when unknown.
fn cpu_limit_cores(source: Source) -> Option<f64> {
	match source {
		Source::CgroupV2 => read_cgroup_cpu_limit_cores(),
		Source::Proc => read_proc_cpu_count(),
	}
}

/// Read a pseudo-file holding a single unsigned integer. These are in-memory
/// kernel files, so the synchronous read does not block meaningfully.
fn read_u64(path: &str) -> Option<u64> {
	std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// cgroup v2 memory limit in bytes, or `None` when unlimited (`memory.max` is
/// `"max"`).
fn read_cgroup_memory_max() -> Option<u64> {
	let raw = std::fs::read_to_string(MEMORY_MAX).ok()?;
	let raw = raw.trim();
	if raw == "max" {
		None
	} else {
		raw.parse().ok()
	}
}

/// Cumulative CPU time consumed by the cgroup, in microseconds, from the
/// `usage_usec` line of `cpu.stat`.
fn read_cgroup_cpu_usage_usec() -> Option<u64> {
	let stat = std::fs::read_to_string(CPU_STAT).ok()?;
	stat.lines()
		.find_map(|line| line.strip_prefix("usage_usec "))
		.and_then(|value| value.trim().parse().ok())
}

/// cgroup v2 CPU limit in vCPU cores from `cpu.max` (`"<quota> <period>"`), or
/// `None` when unlimited (`quota` is `"max"`).
fn read_cgroup_cpu_limit_cores() -> Option<f64> {
	let raw = std::fs::read_to_string(CPU_MAX).ok()?;
	let mut parts = raw.split_whitespace();
	let quota = parts.next()?;
	let period: f64 = parts.next()?.parse().ok()?;
	if quota == "max" || period <= 0.0 {
		return None;
	}
	let quota: f64 = quota.parse().ok()?;
	Some(quota / period)
}

/// `(used_bytes, total_bytes)` from `/proc/meminfo`. Used is `MemTotal -
/// MemAvailable`; total doubles as the limit under gVisor, where it reflects the
/// sandbox memory.
fn read_proc_memory() -> Option<(u64, u64)> {
	let total_kb = read_meminfo_kb("MemTotal")?;
	let available_kb = read_meminfo_kb("MemAvailable")?;
	let used_kb = total_kb.saturating_sub(available_kb);
	Some((used_kb * 1024, total_kb * 1024))
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

/// Number of vCPUs from the per-CPU (`cpu0`, `cpu1`, ...) lines of `/proc/stat`.
fn read_proc_cpu_count() -> Option<f64> {
	let content = std::fs::read_to_string(PROC_STAT).ok()?;
	// The aggregate line is `"cpu "` (trailing space); per-CPU lines are `"cpu0"`.
	let count = content
		.lines()
		.filter(|line| line.starts_with("cpu") && !line.starts_with("cpu "))
		.count();
	(count > 0).then_some(count as f64)
}

fn bytes_to_mib(bytes: u64) -> f64 {
	bytes as f64 / (1024.0 * 1024.0)
}
