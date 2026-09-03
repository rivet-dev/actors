use super::*;

mod moved_tests {
	use std::time::Duration;

	use super::{ActorConfig, ActorConfigInput, SqliteProfilingConfig, SqliteProfilingConfigInput};

	#[test]
	fn actor_config_from_input_applies_overrides() {
		let config = ActorConfig::from_input(ActorConfigInput {
			name: Some("demo".to_owned()),
			on_migrate_timeout_ms: Some(30_000),
			sleep_grace_period_ms: Some(12_000),
			max_queue_size: Some(42),
			max_schedules: Some(84),
			..ActorConfigInput::default()
		});

		assert_eq!(config.name.as_deref(), Some("demo"));
		assert_eq!(config.on_migrate_timeout, Duration::from_secs(30));
		assert_eq!(config.sleep_grace_period, Duration::from_secs(12));
		assert!(config.sleep_grace_period_overridden);
		assert_eq!(config.max_queue_size, 42);
		assert_eq!(config.max_schedules, 84);
	}

	#[test]
	fn actor_config_from_input_keeps_defaults_for_missing_fields() {
		let config = ActorConfig::from_input(ActorConfigInput::default());
		let default = ActorConfig::default();

		assert_eq!(config.name, default.name);
		assert_eq!(config.icon, default.icon);
		assert_eq!(config.state_save_interval, default.state_save_interval);
		assert_eq!(config.create_vars_timeout, default.create_vars_timeout);
		assert_eq!(
			config.create_conn_state_timeout,
			default.create_conn_state_timeout,
		);
		assert_eq!(
			config.on_before_connect_timeout,
			default.on_before_connect_timeout,
		);
		assert_eq!(config.on_connect_timeout, default.on_connect_timeout);
		assert_eq!(config.on_migrate_timeout, default.on_migrate_timeout);
		assert_eq!(config.action_timeout, default.action_timeout);
		assert_eq!(config.sleep_timeout, default.sleep_timeout);
		assert_eq!(config.no_sleep, default.no_sleep);
		assert_eq!(config.sleep_grace_period, default.sleep_grace_period);
		assert_eq!(
			config.sleep_grace_period_overridden,
			default.sleep_grace_period_overridden,
		);
		assert_eq!(
			config.connection_liveness_timeout,
			default.connection_liveness_timeout,
		);
		assert_eq!(
			config.connection_liveness_interval,
			default.connection_liveness_interval,
		);
		assert_eq!(config.max_queue_size, default.max_queue_size);
		assert_eq!(config.max_schedules, default.max_schedules);
		assert_eq!(
			config.max_queue_message_size,
			default.max_queue_message_size,
		);
		assert_eq!(
			config.max_incoming_message_size,
			default.max_incoming_message_size,
		);
		assert_eq!(
			config.max_outgoing_message_size,
			default.max_outgoing_message_size,
		);
		assert!(matches!(
			config.can_hibernate_websocket,
			super::CanHibernateWebSocket::Bool(false),
		));
		assert!(config.overrides.is_none());
		assert!(config.sqlite_profiling.enabled);
		assert_eq!(
			config.sqlite_profiling.max_tracked_statement_fingerprints,
			128
		);
		assert_eq!(
			config.sqlite_profiling.max_tracked_transaction_fingerprints,
			8
		);
		assert_eq!(config.sqlite_profiling.max_prometheus_series, 25_000);
		assert_eq!(config.sqlite_profiling.max_get_pages_requests_per_trace, 16);
		assert_eq!(config.sqlite_profiling.slow_operation_threshold_ms, 10);
		assert_eq!(config.sqlite_profiling.baseline_sample_rate, 0.001);
	}

	#[test]
	fn actor_config_applies_and_validates_sqlite_profiling_overrides() {
		let config = ActorConfig::from_input(ActorConfigInput {
			sqlite_profiling: Some(SqliteProfilingConfigInput {
				enabled: Some(false),
				max_tracked_statement_fingerprints: Some(7),
				baseline_sample_rate: Some(0.25),
				..Default::default()
			}),
			..Default::default()
		});

		assert!(!config.sqlite_profiling.enabled);
		assert_eq!(
			config.sqlite_profiling.max_tracked_statement_fingerprints,
			7
		);
		assert_eq!(config.sqlite_profiling.baseline_sample_rate, 0.25);
		config.validate().expect("profiling config should be valid");

		let invalid = ActorConfig {
			sqlite_profiling: SqliteProfilingConfig {
				baseline_sample_rate: 1.5,
				..Default::default()
			},
			..Default::default()
		};
		assert!(invalid.validate().is_err());
	}

	#[test]
	fn actor_config_effective_sleep_grace_period_uses_default() {
		let config = ActorConfig::default();

		assert_eq!(
			config.effective_sleep_grace_period(),
			Duration::from_secs(15),
		);
	}

	#[test]
	fn actor_config_effective_sleep_grace_period_uses_explicit_value() {
		let config = ActorConfig {
			sleep_grace_period: Duration::from_secs(20),
			sleep_grace_period_overridden: true,
			..ActorConfig::default()
		};

		assert_eq!(
			config.effective_sleep_grace_period(),
			Duration::from_secs(20),
		);
	}
}
