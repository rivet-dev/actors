use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pegboard {
	/// Time to delay an actor from rescheduling after a rescheduling failure.
	///
	/// Unit is in milliseconds.
	pub base_retry_timeout: Option<usize>,
	/// How long to wait for an ack response from the outbound request layer before setting actor as lost.
	///
	/// Unit is in milliseconds.
	pub actor_allocation_threshold: Option<i64>,
	/// How long to wait after creating and not receiving a starting state before setting actor as lost.
	///
	/// Unit is in milliseconds.
	pub actor_start_threshold: Option<i64>,
	/// How long to wait after stopping and not receiving a stop state before setting actor as lost.
	///
	/// Unit is in milliseconds.
	pub actor_stop_threshold: Option<i64>,
	/// How long to wait after starting to attempt to reallocate before before setting actor to sleep.
	///
	/// Unit is in milliseconds.
	pub actor_retry_duration_threshold: Option<i64>,
	/// How long an actor goes without retries before it's retry count is reset to 0, effectively resetting its
	/// backoff to 0.
	///
	/// Unit is in milliseconds.
	pub retry_reset_duration: Option<i64>,
	/// Maximum exponent for the reschedule backoff calculation.
	///
	/// This controls the maximum backoff duration when rescheduling actors.
	pub reschedule_backoff_max_exponent: Option<usize>,
	/// How long after last ping before considering a runner ineligible for allocation.
	///
	/// Unit is in milliseconds.
	pub runner_eligible_threshold: Option<i64>,
	/// How long to wait after last ping before forcibly removing a runner from the database
	/// and deleting its workflow, evicting all actors.
	///
	/// Note that the runner may still be running and can reconnect.
	///
	/// Unit is in milliseconds.
	pub runner_lost_threshold: Option<i64>,
	/// How long after last ping before considering a hibernating request disconnected.
	///
	/// Unit is in milliseconds.
	pub hibernating_request_eligible_threshold: Option<i64>,
	/// Time to delay a serverless runner from attempting a new outbound connection after a connection failure.
	///
	/// Unit is in milliseconds.
	pub serverless_base_retry_timeout: Option<usize>,
	/// How long a serverless runner goes without connection failures before it's retry count is reset to 0,
	/// effectively resetting its backoff to 0.
	///
	/// Unit is in milliseconds.
	pub serverless_retry_reset_duration: Option<i64>,
	/// Maximum exponent for the serverless backoff calculation.
	///
	/// This controls the maximum backoff duration when serverlessly connecting to runners.
	pub serverless_backoff_max_exponent: Option<usize>,

	/// Global pool desired max.
	pub pool_desired_max_override: Option<u32>,

	/// Default metadata poll interval for serverless runners when not specified in runner config.
	///
	/// Unit is in milliseconds.
	pub default_metadata_poll_interval: Option<u64>,

	/// Minimum metadata poll interval for serverless runners.
	///
	/// The actual poll interval will be the maximum of this value and the runner config's
	/// `metadata_poll_interval` setting. This prevents excessive polling even if the
	/// runner config specifies a very short interval.
	///
	/// Unit is in milliseconds.
	pub min_metadata_poll_interval: Option<u64>,

	/// Number of consecutive successes required to clear an active runner pool error.
	///
	/// This prevents a single success from clearing an error during flapping conditions.
	/// Higher values provide more stability but slower recovery from transient errors.
	pub runner_pool_consecutive_successes_to_clear_error: Option<u32>,

	/// Amount of runners to query from the allocation queue and choose at random when allocating an actor.
	pub actor_allocation_candidate_sample_size: Option<usize>,

	/// Max response payload size in bytes from actors.
	pub runner_max_response_payload_body_size: Option<usize>,
	/// Ping interval for runner updates in milliseconds.
	pub runner_update_ping_interval_ms: Option<u64>,
	/// Max time since last pong before the runner connection is terminated. Unit is in milliseconds.
	pub runner_ping_timeout_ms: Option<i64>,
	/// GC interval for actor event demuxer in milliseconds.
	pub runner_event_demuxer_gc_interval_ms: Option<u64>,
	/// Max time since last seen before actor is considered stale, in milliseconds.
	pub runner_event_demuxer_max_last_seen_ms: Option<u64>,

	// === Gateway Settings ===
	/// WebSocket open/handshake timeout in milliseconds.
	pub gateway_websocket_open_timeout_ms: Option<u64>,
	/// Timeout for response to start in milliseconds.
	pub gateway_response_start_timeout_ms: Option<u64>,
	/// Ping interval for gateway updates in milliseconds.
	pub gateway_update_ping_interval_ms: Option<u64>,
	/// GC interval for in-flight requests in milliseconds.
	pub gateway_gc_interval_ms: Option<u64>,
	/// Tunnel ping timeout in milliseconds.
	pub gateway_tunnel_ping_timeout_ms: Option<i64>,
	/// Hibernating WebSocket message ack timeout in milliseconds.
	pub gateway_hws_message_ack_timeout_ms: Option<u64>,
	/// Max pending message buffer size for hibernating WebSockets in bytes.
	pub gateway_hws_max_pending_size: Option<u64>,
	/// Max HTTP request body size in bytes for requests to actors.
	pub gateway_http_max_request_body_size: Option<usize>,
	/// Max burst of inbound WebSocket messages on a single connection before throttling.
	pub gateway_websocket_rate_limit_requests: Option<u64>,
	/// Time to regain one inbound WebSocket message token on a single connection.
	///
	/// Unit is in milliseconds.
	pub gateway_websocket_rate_limit_drip_rate_ms: Option<u64>,

	// === Envoy Settings ===
	/// How long to wait before considering an envoy lost and evicting all of its actors.
	///
	/// Unit is in milliseconds.
	pub envoy_lost_threshold: Option<i64>,
	/// Max time since last pong before the envoy connection is terminated. Unit is in milliseconds.
	pub envoy_ping_timeout: Option<i64>,
	/// GC interval for actor event demuxer in milliseconds.
	pub envoy_event_demuxer_gc_interval: Option<u64>,
	/// Max time since last seen before actor is considered stale, in milliseconds.
	pub envoy_event_demuxer_max_last_seen_threshold: Option<u64>,
	/// Max response payload size in bytes from actors.
	pub envoy_max_response_payload_size: Option<usize>,
	/// Ping interval for envoy updates in milliseconds.
	pub envoy_update_ping_interval: Option<u64>,
	/// How long after last ping before considering a envoy ineligible for allocation.
	///
	/// Unit is in milliseconds.
	pub envoy_eligible_threshold: Option<i64>,
	/// Strategy for choosing an Envoy when allocating a serverful actor.
	pub envoy_load_balancer: Option<EnvoyLoadBalancer>,
	/// Maximum concurrent background expire operations spawned by the read-path envoy expire scheduler.
	pub envoy_expire_scheduler_max_concurrent_expires: Option<usize>,
	/// Maximum pending envoys tracked by the read-path envoy expire scheduler.
	pub envoy_expire_scheduler_max_pending: Option<usize>,
	/// Max burst of inbound WebSocket messages on a single envoy connection before throttling.
	pub envoy_websocket_rate_limit_requests: Option<u64>,
	/// Time to regain one inbound WebSocket message token on a single envoy connection.
	///
	/// Unit is in microseconds. The envoy connection multiplexes every actor on a runner, so the
	/// sustained ceiling is far higher than the per-client gateway limit and needs sub-millisecond
	/// granularity to express.
	pub envoy_websocket_rate_limit_drip_rate_us: Option<u64>,

	// === Serverless Settings ===
	/// **Deprecated** Configure the drain period in the runner config.
	///
	/// Drain grace period for serverless runners.
	///
	/// This time is subtracted from the configured request duration. Once `duration - grace` is reached, the
	/// runner is sent stop commands for all of its actors. After the grace period is over (i.e. the full
	/// duration is reached) the runner websocket is forcibly closed.
	///
	/// Unit is in milliseconds.
	pub serverless_drain_grace_period: Option<u64>,

	// === KV Preload Settings ===
	/// Maximum total size of all preloaded KV data sent with the actor start command.
	/// Setting to 0 disables all preloading.
	///
	/// Unit is in bytes. Default: 1,048,576 (1 MiB).
	pub preload_max_total_bytes: Option<u64>,

	// === Rate Limiting ===
	/// Max burst of actor creations per namespace before throttling.
	pub actor_create_rate_limit_requests: Option<u64>,
	/// Time to regain one actor creation token per namespace.
	///
	/// Unit is in milliseconds.
	pub actor_create_rate_limit_drip_rate_ms: Option<u64>,
}

impl Pegboard {
	pub fn validate(&self) -> Result<()> {
		if let Some(EnvoyLoadBalancer::Hash {
			virtual_nodes,
			samples,
			max_scan,
			slot_jitter,
			..
		}) = self.envoy_load_balancer
		{
			if samples == 0 || samples > 8 {
				bail!("pegboard.envoy_load_balancer.hash.samples must be in 1..=8");
			}

			if virtual_nodes == 0 || virtual_nodes > 64 {
				bail!("pegboard.envoy_load_balancer.hash.virtual_nodes must be in 1..=64");
			}

			if max_scan == 0 || max_scan > 256 {
				bail!("pegboard.envoy_load_balancer.hash.max_scan must be in 1..=256");
			}

			if slot_jitter > 64 {
				bail!("pegboard.envoy_load_balancer.hash.slot_jitter must be in 0..=64");
			}
		}

		if self.envoy_expire_scheduler_max_concurrent_expires == Some(0) {
			bail!("pegboard.envoy_expire_scheduler_max_concurrent_expires must be greater than 0");
		}

		Ok(())
	}

	pub fn base_retry_timeout(&self) -> usize {
		self.base_retry_timeout.unwrap_or(2_000)
	}

	pub fn actor_allocation_threshold(&self) -> i64 {
		self.actor_allocation_threshold.unwrap_or(2_000)
	}

	pub fn actor_start_threshold(&self) -> i64 {
		self.actor_start_threshold.unwrap_or(30_000)
	}

	/// When changing this default, update
	/// website/src/content/docs/actors/versions.mdx (SIGTERM Handling section).
	pub fn actor_stop_threshold(&self) -> i64 {
		self.actor_stop_threshold.unwrap_or(30 * 60 * 1000)
	}

	pub fn actor_retry_duration_threshold(&self) -> i64 {
		self.actor_retry_duration_threshold.unwrap_or(5 * 60 * 1000)
	}

	pub fn retry_reset_duration(&self) -> i64 {
		self.retry_reset_duration.unwrap_or(10 * 60 * 1000)
	}

	pub fn reschedule_backoff_max_exponent(&self) -> usize {
		self.reschedule_backoff_max_exponent.unwrap_or(8)
	}

	pub fn runner_eligible_threshold(&self) -> i64 {
		self.runner_eligible_threshold.unwrap_or(10_000)
	}

	pub fn runner_lost_threshold(&self) -> i64 {
		self.runner_lost_threshold.unwrap_or(15_000)
	}

	pub fn hibernating_request_eligible_threshold(&self) -> i64 {
		self.hibernating_request_eligible_threshold
			.unwrap_or(90_000)
	}

	pub fn serverless_base_retry_timeout(&self) -> usize {
		self.serverless_base_retry_timeout.unwrap_or(2_000)
	}

	pub fn serverless_retry_reset_duration(&self) -> i64 {
		self.serverless_retry_reset_duration
			.unwrap_or(10 * 60 * 1000)
	}

	pub fn serverless_backoff_max_exponent(&self) -> usize {
		self.serverless_backoff_max_exponent.unwrap_or(8)
	}

	pub fn runner_pool_error_consecutive_successes_to_clear(&self) -> u32 {
		self.runner_pool_consecutive_successes_to_clear_error
			.unwrap_or(3)
	}

	pub fn default_metadata_poll_interval(&self) -> u64 {
		self.default_metadata_poll_interval.unwrap_or(10_000)
	}

	pub fn min_metadata_poll_interval(&self) -> u64 {
		self.min_metadata_poll_interval.unwrap_or(5_000)
	}

	pub fn actor_allocation_candidate_sample_size(&self) -> usize {
		self.actor_allocation_candidate_sample_size.unwrap_or(100)
	}

	pub fn gateway_websocket_open_timeout_ms(&self) -> u64 {
		self.gateway_websocket_open_timeout_ms.unwrap_or(15_000)
	}

	pub fn gateway_response_start_timeout_ms(&self) -> u64 {
		self.gateway_response_start_timeout_ms
			.unwrap_or(5 * 60 * 1000)
	}

	pub fn gateway_update_ping_interval_ms(&self) -> u64 {
		self.gateway_update_ping_interval_ms.unwrap_or(3_000)
	}

	pub fn gateway_gc_interval_ms(&self) -> u64 {
		self.gateway_gc_interval_ms.unwrap_or(15_000)
	}

	pub fn gateway_tunnel_ping_timeout_ms(&self) -> i64 {
		self.gateway_tunnel_ping_timeout_ms.unwrap_or(30_000)
	}

	pub fn gateway_hws_message_ack_timeout_ms(&self) -> u64 {
		self.gateway_hws_message_ack_timeout_ms.unwrap_or(30_000)
	}

	pub fn gateway_hws_max_pending_size(&self) -> u64 {
		self.gateway_hws_max_pending_size
			.unwrap_or(128 * 1024 * 1024) // 128 MiB
	}

	pub fn runner_max_response_payload_body_size(&self) -> usize {
		self.runner_max_response_payload_body_size
			.unwrap_or(20 * 1024 * 1024) // 20 MiB
	}

	pub fn runner_update_ping_interval_ms(&self) -> u64 {
		self.runner_update_ping_interval_ms.unwrap_or(3_000)
	}

	pub fn runner_ping_timeout_ms(&self) -> i64 {
		self.runner_ping_timeout_ms.unwrap_or(15_000)
	}

	pub fn runner_event_demuxer_gc_interval_ms(&self) -> u64 {
		self.runner_event_demuxer_gc_interval_ms.unwrap_or(30_000)
	}

	pub fn runner_event_demuxer_max_last_seen_ms(&self) -> u64 {
		self.runner_event_demuxer_max_last_seen_ms.unwrap_or(30_000)
	}

	pub fn envoy_lost_threshold(&self) -> i64 {
		self.envoy_lost_threshold.unwrap_or(15_000)
	}

	pub fn envoy_ping_timeout(&self) -> i64 {
		self.envoy_ping_timeout.unwrap_or(15_000)
	}

	pub fn envoy_event_demuxer_gc_interval(&self) -> u64 {
		self.envoy_event_demuxer_gc_interval.unwrap_or(30_000)
	}

	pub fn envoy_event_demuxer_max_last_seen_threshold(&self) -> u64 {
		self.envoy_event_demuxer_max_last_seen_threshold
			.unwrap_or(30_000)
	}

	pub fn envoy_max_response_payload_size(&self) -> usize {
		self.envoy_max_response_payload_size
			.unwrap_or(20 * 1024 * 1024) // 20 MiB
	}

	pub fn envoy_update_ping_interval(&self) -> u64 {
		self.envoy_update_ping_interval.unwrap_or(3_000)
	}

	pub fn envoy_eligible_threshold(&self) -> i64 {
		self.envoy_eligible_threshold.unwrap_or(10_000)
	}

	pub fn envoy_load_balancer(&self) -> EnvoyLoadBalancer {
		self.envoy_load_balancer.unwrap_or_default()
	}

	pub fn envoy_expire_scheduler_max_concurrent_expires(&self) -> usize {
		self.envoy_expire_scheduler_max_concurrent_expires
			.unwrap_or(32)
	}

	pub fn envoy_expire_scheduler_max_pending(&self) -> usize {
		self.envoy_expire_scheduler_max_pending.unwrap_or(1024)
	}

	pub fn serverless_drain_grace_period(&self) -> u64 {
		self.serverless_drain_grace_period.unwrap_or(10_000)
	}

	pub fn gateway_websocket_rate_limit_requests(&self) -> u64 {
		self.gateway_websocket_rate_limit_requests.unwrap_or(2_000)
	}

	pub fn gateway_websocket_rate_limit_drip_rate_ms(&self) -> u64 {
		self.gateway_websocket_rate_limit_drip_rate_ms.unwrap_or(10)
	}

	pub fn envoy_websocket_rate_limit_requests(&self) -> u64 {
		self.envoy_websocket_rate_limit_requests.unwrap_or(16_384)
	}

	pub fn envoy_websocket_rate_limit_drip_rate_us(&self) -> u64 {
		self.envoy_websocket_rate_limit_drip_rate_us.unwrap_or(200)
	}

	pub fn actor_create_rate_limit_requests(&self) -> u64 {
		self.actor_create_rate_limit_requests.unwrap_or(500)
	}

	pub fn actor_create_rate_limit_drip_rate_ms(&self) -> u64 {
		self.actor_create_rate_limit_drip_rate_ms.unwrap_or(10)
	}

	pub fn preload_max_total_bytes(&self) -> u64 {
		self.preload_max_total_bytes.unwrap_or(1_048_576)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvoyLoadBalancer {
	/// Current default. Finds the highest protocol version, then seeks from a random ping timestamp.
	RandomPingTimestamp {
		#[serde(default = "default_true")]
		use_snapshot_read: bool,
	},
	/// Finds the highest protocol version, then chooses the newest fresh ping timestamp.
	/// This is purely for testing behavior that pins allocations to a single Envoy.
	NewestPingTimestamp {
		#[serde(default = "default_true")]
		use_snapshot_read: bool,
	},
	/// Reads the full highest-version Envoy load-balancer range and chooses a random eligible Envoy.
	RandomFullRange {
		#[serde(default = "default_true")]
		use_snapshot_read: bool,
	},
	/// Hash-ring-based envoy selection backed by `EnvoyHashIdxKey`.
	///
	/// `samples` controls the algorithm:
	/// - `1`: uniform random pick — draws one random ring pivot and returns
	///   the first fresh envoy. **Short-circuits past the `SlotsKey` read**,
	///   so this variant is strictly cheaper than `samples >= 2`. Pick when
	///   actor cost per envoy is uniform or measurements don't justify load
	///   awareness.
	/// - `>= 2`: power-of-K choices. Draws K independent random pivots,
	///   reads `SlotsKey` for each, picks min slots with uniform random
	///   tiebreak. Recommended default.
	Hash {
		#[serde(default = "default_virtual_nodes")]
		virtual_nodes: u8,
		/// Number of independent ring samples per allocation.
		/// `1` = uniform pick (no slot read), `>= 2` = power-of-K-choices.
		#[serde(default = "default_samples")]
		samples: u8,
		/// Maximum stale entries walked per `scan_for_fresh` call before
		/// aborting and returning `None`. Circuit breaker against pathological
		/// drain scenarios. Default 16. Range 1..=256.
		#[serde(default = "default_max_scan")]
		max_scan: u32,
		/// Additive random integer added to each candidate's slot count before
		/// the min-slot comparison. Decorrelates concurrent allocators reading
		/// the same stale `SlotsKey` snapshot. Only relevant when `samples >= 2`.
		/// `0` disables. Range 0..=64. Default 4.
		///
		/// See `slot_jitter` block in
		/// `engine/packages/pegboard/src/workflows/actor2/alloc_serverful/hash.rs`
		/// for how the default was sized.
		#[serde(default = "default_slot_jitter")]
		slot_jitter: u8,
		#[serde(default = "default_true")]
		use_snapshot_read: bool,
	},
}

impl Default for EnvoyLoadBalancer {
	fn default() -> Self {
		// EnvoyLoadBalancer::RandomPingTimestamp {
		// 	use_snapshot_read: true,
		// }
		EnvoyLoadBalancer::Hash {
			virtual_nodes: 8,
			samples: 2,
			max_scan: 16,
			slot_jitter: 4,
			use_snapshot_read: true,
		}
	}
}

fn default_true() -> bool {
	true
}

fn default_virtual_nodes() -> u8 {
	8
}

fn default_samples() -> u8 {
	2
}

fn default_max_scan() -> u32 {
	16
}

fn default_slot_jitter() -> u8 {
	4
}
