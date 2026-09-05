use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use futures::future::BoxFuture;
#[cfg(feature = "wasm-runtime")]
use futures::future::{AbortHandle, Abortable};
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_error::RivetError;
use rivetkit_actor_persist::versioned::{ScheduleTraceContext, ScheduleTraceContextData};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tracing::Instrument;
use uuid::Uuid;

use crate::actor::context::ActorContext;
use crate::actor::internal_storage::queries::*;
use crate::actor::persist::{
	decode_latest_with_embedded_version, encode_latest_with_embedded_version,
};
use crate::error::{ScheduleRuntimeError, client_error_message, client_error_metadata};
use crate::sqlite::{BindParam, ColumnValue, SqliteBatchStatement};
use crate::telemetry::{ActorInvocationTelemetry, ScheduleTraceOrigin};
use crate::time::{SystemTime, UNIX_EPOCH, sleep};

const CRON_ID_PREFIX: &str = "cron:";
const HISTORY_RUNNING: i64 = 0;
const HISTORY_OK: i64 = 1;
const HISTORY_ERROR: i64 = 2;
const HISTORY_SKIPPED: i64 = 3;
pub const DEFAULT_MAX_HISTORY: i64 = 100;
pub const MAX_HISTORY: i64 = 1_000;
pub const MAX_ACTOR_HISTORY: i64 = 10_000;
pub const MIN_INTERVAL_MS: i64 = 5_000;
const DEFAULT_HISTORY_LIMIT: i64 = 20;
const CLAIM_ONE_SHOT_BATCH_SIZE: usize = 128;
const SCHEDULE_TRACE_CONTEXT_META_PREFIX: &str = "schedule_trace_context:";
const SCHEDULE_TRACE_CONTEXT_VERSION: u16 = 1;
pub(crate) const GLOBAL_HISTORY_PRUNE_INTERVAL: usize = 100;
pub(crate) const GLOBAL_HISTORY_RETAINED_ROWS: i64 =
	MAX_ACTOR_HISTORY - GLOBAL_HISTORY_PRUNE_INTERVAL as i64;

fn min_deadline(left: Option<i64>, right: Option<i64>) -> Option<i64> {
	match (left, right) {
		(Some(left), Some(right)) => Some(left.min(right)),
		(Some(value), None) | (None, Some(value)) => Some(value),
		(None, None) => None,
	}
}

pub(super) type InternalKeepAwakeCallback =
	Arc<dyn Fn(BoxFuture<'static, Result<()>>) -> BoxFuture<'static, Result<()>> + Send + Sync>;
pub(super) type LocalAlarmCallback = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
	At,
	Cron,
	Every,
}

impl ScheduleKind {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::At => "at",
			Self::Cron => "cron",
			Self::Every => "every",
		}
	}

	fn as_i64(self) -> i64 {
		match self {
			Self::At => 0,
			Self::Cron => 1,
			Self::Every => 2,
		}
	}

	fn parse(value: i64, schedule_id: &str) -> Result<Self> {
		match value {
			0 => Ok(Self::At),
			1 => Ok(Self::Cron),
			2 => Ok(Self::Every),
			other => Err(ScheduleRuntimeError::InvalidScheduleRow {
				schedule_id: schedule_id.to_owned(),
				reason: format!("unknown kind {other}"),
			}
			.build()),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledEventInfo {
	pub id: String,
	pub action: String,
	pub args: Vec<u8>,
	pub run_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobInfo {
	pub name: String,
	pub kind: ScheduleKind,
	pub action: String,
	pub args: Vec<u8>,
	pub next_run_at: i64,
	pub last_run_at: Option<i64>,
	pub expression: Option<String>,
	pub timezone: Option<String>,
	pub interval_ms: Option<i64>,
	pub max_history: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledFireInfo {
	pub kind: ScheduleKind,
	pub id: String,
	pub name: Option<String>,
	pub scheduled_at: i64,
	pub fired_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduleErrorInfo {
	pub group: String,
	pub code: String,
	pub message: String,
	pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronFire {
	pub action: String,
	pub scheduled_at: i64,
	pub fired_at: i64,
	pub finished_at: Option<i64>,
	pub result: String,
	pub error: Option<ScheduleErrorInfo>,
}

pub(crate) struct DueScheduleDispatch {
	pub event_id: String,
	pub action: String,
	pub args: Vec<u8>,
	pub fire: ScheduledFireInfo,
	pub history_id: Option<i64>,
	pub origin: ScheduleTraceOrigin,
}

#[derive(Clone, Debug)]
struct StoredSchedule {
	event_id: String,
	trigger_at: i64,
	action: String,
	args: Vec<u8>,
	kind: ScheduleKind,
	cron_expression: Option<String>,
	timezone: Option<String>,
	interval_ms: Option<i64>,
	last_started_at: Option<i64>,
	max_history: i64,
}

impl ActorContext {
	#[cfg(any(test, feature = "test-support"))]
	pub fn set_schedule_time_for_tests(&self, timestamp_ms: i64) {
		self.0
			.schedule_now_override
			.store(timestamp_ms, Ordering::SeqCst);
	}

	pub(crate) fn schedule_now_timestamp_ms(&self) -> i64 {
		#[cfg(any(test, feature = "test-support"))]
		{
			let timestamp_ms = self.0.schedule_now_override.load(Ordering::SeqCst);
			if timestamp_ms != i64::MIN {
				return timestamp_ms;
			}
		}
		system_now_timestamp_ms()
	}

	/// Trace origin of the invocation defining this schedule, empty when the
	/// caller is not inside a traced invocation.
	fn schedule_trace_origin(&self) -> ScheduleTraceOrigin {
		self.invocation_telemetry()
			.map(ActorInvocationTelemetry::schedule_trace_origin)
			.unwrap_or_default()
	}

	pub async fn after(
		&self,
		duration: Duration,
		action_name: &str,
		args: &[u8],
	) -> Result<String> {
		let duration_ms = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
		let timestamp_ms = self.schedule_now_timestamp_ms().saturating_add(duration_ms);
		self.at(timestamp_ms, action_name, args).await
	}

	pub async fn at(&self, timestamp_ms: i64, action_name: &str, args: &[u8]) -> Result<String> {
		let origin = self.schedule_trace_origin();
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		self.ensure_schedule_capacity(false).await?;
		let event_id = Uuid::new_v4().to_string();
		let schedule_params = vec![
			BindParam::Text(event_id.clone()),
			BindParam::Integer(timestamp_ms),
			BindParam::Text(action_name.to_owned()),
			args_param(args),
			BindParam::Integer(ScheduleKind::At.as_i64()),
			BindParam::Null,
			BindParam::Null,
			BindParam::Null,
			BindParam::Null,
			BindParam::Integer(0),
		];
		let mut statements = vec![SqliteBatchStatement {
			sql: INSERT_SCHEDULE_EVENT_SQL.to_owned(),
			params: Some(schedule_params),
		}];
		append_schedule_trace_context_upsert(&mut statements, &event_id, origin)?;
		self.sql()
			.execute_batch(statements)
			.await
			.context("insert one-shot schedule")?;
		self.mark_schedule_dirty();
		self.record_schedules_updated();
		self.sync_alarm().await?;
		Ok(event_id)
	}

	pub async fn cancel_schedule(&self, event_id: &str) -> Result<bool> {
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let results = self
			.sql()
			.execute_batch(vec![
				SqliteBatchStatement {
					sql: CANCEL_SCHEDULE_SQL.to_owned(),
					params: Some(vec![
						BindParam::Text(event_id.to_owned()),
						BindParam::Integer(ScheduleKind::At.as_i64()),
					]),
				},
				delete_orphan_schedule_trace_context(event_id),
			])
			.await
			.context("cancel one-shot schedule")?;
		let removed = results.first().is_some_and(|result| result.changes > 0);
		if removed {
			self.mark_schedule_dirty();
			self.record_schedules_updated();
			self.sync_alarm().await?;
		}
		Ok(removed)
	}

	pub async fn get_scheduled_event(&self, event_id: &str) -> Result<Option<ScheduledEventInfo>> {
		let result = self
			.sql()
			.query(
				GET_SCHEDULED_EVENT_SQL,
				Some(vec![
					BindParam::Text(event_id.to_owned()),
					BindParam::Integer(ScheduleKind::At.as_i64()),
				]),
			)
			.await
			.context("get one-shot schedule")?;
		result
			.rows
			.first()
			.map(|row| read_stored_schedule(row))
			.transpose()
			.map(|row| {
				row.map(|event| ScheduledEventInfo {
					id: event.event_id,
					action: event.action,
					args: event.args,
					run_at: event.trigger_at,
				})
			})
	}

	pub async fn list_scheduled_events(&self) -> Result<Vec<ScheduledEventInfo>> {
		let result = self
			.sql()
			.query(
				LIST_SCHEDULED_EVENTS_SQL,
				Some(vec![BindParam::Integer(ScheduleKind::At.as_i64())]),
			)
			.await
			.context("list one-shot schedules")?;
		result
			.rows
			.iter()
			.map(|row| read_stored_schedule(row))
			.map(|event| {
				event.map(|event| ScheduledEventInfo {
					id: event.event_id,
					action: event.action,
					args: event.args,
					run_at: event.trigger_at,
				})
			})
			.collect()
	}

	pub async fn cron_set(
		&self,
		name: &str,
		expression: &str,
		timezone: Option<&str>,
		action_name: &str,
		args: &[u8],
		max_history: Option<i64>,
	) -> Result<()> {
		let origin = self.schedule_trace_origin();
		validate_name(name)?;
		let timezone = timezone.unwrap_or("UTC");
		let timezone_parsed = parse_timezone(timezone)?;
		let cron = parse_cron(expression)?;
		let max_history = validate_max_history(max_history)?;
		let now_ms = self.schedule_now_timestamp_ms();
		let event_id = cron_event_id(name);
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let existing = self.load_schedule(&event_id).await?;
		self.ensure_schedule_capacity(existing.is_some()).await?;
		let cadence_unchanged = existing.as_ref().is_some_and(|existing| {
			existing.kind == ScheduleKind::Cron
				&& existing.cron_expression.as_deref() == Some(expression)
				&& existing.timezone.as_deref() == Some(timezone)
		});
		let trigger_at = if cadence_unchanged {
			existing.as_ref().expect("checked above").trigger_at
		} else {
			next_cron_timestamp_ms(&cron, timezone_parsed, now_ms)?
		};
		self.upsert_recurring(
			&event_id,
			trigger_at,
			action_name,
			args,
			ScheduleKind::Cron,
			Some(expression),
			Some(timezone),
			None,
			max_history,
			origin,
		)
		.await?;
		self.prune_schedule_history(&event_id, max_history).await?;
		self.mark_schedule_dirty();
		self.record_schedules_updated();
		self.sync_alarm().await
	}

	pub async fn cron_every(
		&self,
		name: &str,
		interval_ms: i64,
		action_name: &str,
		args: &[u8],
		max_history: Option<i64>,
	) -> Result<()> {
		let origin = self.schedule_trace_origin();
		validate_name(name)?;
		if interval_ms < MIN_INTERVAL_MS {
			return Err(ScheduleRuntimeError::InvalidInterval {
				interval_ms,
				minimum_ms: MIN_INTERVAL_MS,
			}
			.build());
		}
		let max_history = validate_max_history(max_history)?;
		let event_id = cron_event_id(name);
		let now_ms = self.schedule_now_timestamp_ms();
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let existing = self.load_schedule(&event_id).await?;
		self.ensure_schedule_capacity(existing.is_some()).await?;
		let cadence_unchanged = existing.as_ref().is_some_and(|existing| {
			existing.kind == ScheduleKind::Every && existing.interval_ms == Some(interval_ms)
		});
		let trigger_at = if cadence_unchanged {
			existing.as_ref().expect("checked above").trigger_at
		} else {
			now_ms.saturating_add(interval_ms)
		};
		self.upsert_recurring(
			&event_id,
			trigger_at,
			action_name,
			args,
			ScheduleKind::Every,
			None,
			None,
			Some(interval_ms),
			max_history,
			origin,
		)
		.await?;
		self.prune_schedule_history(&event_id, max_history).await?;
		self.mark_schedule_dirty();
		self.record_schedules_updated();
		self.sync_alarm().await
	}

	#[allow(clippy::too_many_arguments)]
	async fn upsert_recurring(
		&self,
		event_id: &str,
		trigger_at: i64,
		action_name: &str,
		args: &[u8],
		kind: ScheduleKind,
		cron_expression: Option<&str>,
		timezone: Option<&str>,
		interval_ms: Option<i64>,
		max_history: i64,
		origin: ScheduleTraceOrigin,
	) -> Result<()> {
		let mut statements = vec![SqliteBatchStatement {
			sql: UPSERT_RECURRING_SCHEDULE_SQL.to_owned(),
			params: Some(vec![
				BindParam::Text(event_id.to_owned()),
				BindParam::Integer(trigger_at),
				BindParam::Text(action_name.to_owned()),
				args_param(args),
				BindParam::Integer(kind.as_i64()),
				optional_text_param(cron_expression),
				optional_text_param(timezone),
				optional_i64_param(interval_ms),
				BindParam::Null,
				BindParam::Integer(max_history),
			]),
		}];
		append_schedule_trace_context_upsert(&mut statements, event_id, origin)?;
		self.sql()
			.execute_batch(statements)
			.await
			.context("upsert recurring schedule")?;
		Ok(())
	}

	pub async fn cron_delete(&self, name: &str) -> Result<bool> {
		validate_name(name)?;
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let event_id = cron_event_id(name);
		let results = self
			.sql()
			.execute_batch(vec![
				SqliteBatchStatement {
					sql: DELETE_CRON_HISTORY_SQL.to_owned(),
					params: Some(vec![
						BindParam::Text(event_id.clone()),
						BindParam::Text(event_id.clone()),
						BindParam::Integer(ScheduleKind::At.as_i64()),
					]),
				},
				SqliteBatchStatement {
					sql: DELETE_CRON_SQL.to_owned(),
					params: Some(vec![
						BindParam::Text(event_id.clone()),
						BindParam::Integer(ScheduleKind::At.as_i64()),
					]),
				},
				delete_orphan_schedule_trace_context(&event_id),
			])
			.await
			.context("delete recurring schedule and history")?;
		let removed = results
			.get(1)
			.is_some_and(|event_result| event_result.changes > 0);
		if removed {
			self.mark_schedule_dirty();
			self.record_schedules_updated();
			self.sync_alarm().await?;
		}
		Ok(removed)
	}

	pub(crate) async fn cron_delete_if_action(&self, name: &str, action: &str) -> Result<bool> {
		validate_name(name)?;
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let event_id = cron_event_id(name);
		let results = self
			.sql()
			.execute_batch(vec![
				SqliteBatchStatement {
					sql: DELETE_CRON_IF_ACTION_SQL.to_owned(),
					params: Some(vec![
						BindParam::Text(event_id.clone()),
						BindParam::Integer(ScheduleKind::At.as_i64()),
						BindParam::Text(action.to_owned()),
					]),
				},
				delete_orphan_schedule_trace_context(&event_id),
			])
			.await
			.context("delete recurring schedule with matching action")?;
		let removed = results.first().is_some_and(|result| result.changes > 0);
		if removed {
			self.mark_schedule_dirty();
			self.record_schedules_updated();
			self.sync_alarm().await?;
		}
		Ok(removed)
	}

	pub async fn cron_get(&self, name: &str) -> Result<Option<CronJobInfo>> {
		validate_name(name)?;
		self.load_schedule(&cron_event_id(name))
			.await?
			.map(stored_to_cron_info)
			.transpose()
	}

	pub async fn cron_list(&self) -> Result<Vec<CronJobInfo>> {
		let result = self
			.sql()
			.query(
				LIST_CRONS_SQL,
				Some(vec![BindParam::Integer(ScheduleKind::At.as_i64())]),
			)
			.await
			.context("list recurring schedules")?;
		result
			.rows
			.iter()
			.map(|row| read_stored_schedule(row))
			.map(|row| row.and_then(stored_to_cron_info))
			.collect()
	}

	pub async fn cron_history(&self, name: &str, limit: Option<i64>) -> Result<Vec<CronFire>> {
		validate_name(name)?;
		let limit = limit.unwrap_or(DEFAULT_HISTORY_LIMIT);
		if !(1..=MAX_HISTORY).contains(&limit) {
			return Err(ScheduleRuntimeError::InvalidMaxHistory {
				max_history: limit,
				maximum: MAX_HISTORY,
			}
			.build());
		}
		let result = self
			.sql()
			.query(
				CRON_HISTORY_SQL,
				Some(vec![
					BindParam::Text(cron_event_id(name)),
					BindParam::Integer(limit),
				]),
			)
			.await
			.context("read recurring schedule history")?;
		result.rows.iter().map(|row| read_cron_fire(row)).collect()
	}

	async fn load_schedule(&self, event_id: &str) -> Result<Option<StoredSchedule>> {
		let result = self
			.sql()
			.query(
				LOAD_SCHEDULE_SQL,
				Some(vec![BindParam::Text(event_id.to_owned())]),
			)
			.await
			.context("load schedule")?;
		result
			.rows
			.first()
			.map(|row| read_stored_schedule(row))
			.transpose()
	}

	async fn ensure_schedule_capacity(&self, replacing_existing: bool) -> Result<()> {
		if replacing_existing {
			return Ok(());
		}
		let result = self
			.sql()
			.query(COUNT_SCHEDULES_SQL, None)
			.await
			.context("count pending schedules")?;
		let count = result
			.rows
			.first()
			.map(|row| read_i64(row, 0, "schedule count"))
			.transpose()?
			.unwrap_or_default();
		if count >= i64::from(self.0.max_schedules) {
			return Err(ScheduleRuntimeError::MaxSchedulesExceeded {
				maximum: self.0.max_schedules,
			}
			.build());
		}
		Ok(())
	}

	pub(crate) async fn take_due_schedule_dispatches(&self) -> Result<Vec<DueScheduleDispatch>> {
		if !self
			.0
			.schedule_alarm_dispatch_enabled
			.load(Ordering::SeqCst)
		{
			return Ok(Vec::new());
		}
		let now_ms = self.schedule_now_timestamp_ms();
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let result = self
			.sql()
			.query(
				TAKE_DUE_SCHEDULES_SQL,
				Some(vec![BindParam::Integer(now_ms)]),
			)
			.await
			.context("load due schedules")?;
		let due_schedules = result
			.rows
			.iter()
			.map(|row| read_due_schedule(row))
			.collect::<Result<Vec<_>>>()?;
		let claim_statements = due_schedules
			.iter()
			.filter(|(event, _)| event.kind == ScheduleKind::At)
			.collect::<Vec<_>>()
			.chunks(CLAIM_ONE_SHOT_BATCH_SIZE)
			.flat_map(|events| {
				let mut params = Vec::with_capacity(events.len() * 2 + 1);
				params.push(BindParam::Integer(ScheduleKind::At.as_i64()));
				for (event, _) in events {
					params.push(BindParam::Text(event.event_id.clone()));
					params.push(BindParam::Integer(event.trigger_at));
				}
				let delete_contexts = SqliteBatchStatement {
					sql: delete_schedule_trace_contexts_sql(events.len()),
					params: Some(
						events
							.iter()
							.map(|(event, _)| {
								BindParam::Text(schedule_trace_context_key(&event.event_id))
							})
							.collect(),
					),
				};
				[
					delete_contexts,
					SqliteBatchStatement {
						sql: claim_one_shots_sql(events.len()),
						params: Some(params),
					},
				]
			})
			.collect::<Vec<_>>();
		if !claim_statements.is_empty() {
			self.sql()
				.execute_batch(claim_statements)
				.await
				.context("claim due one-shot schedules")?;
		}
		let mut dispatches = Vec::new();
		for (event, origin) in due_schedules {
			if event.kind == ScheduleKind::At {
				dispatches.push(DueScheduleDispatch {
					event_id: event.event_id.clone(),
					action: event.action,
					args: event.args,
					fire: ScheduledFireInfo {
						kind: ScheduleKind::At,
						id: event.event_id,
						name: None,
						scheduled_at: event.trigger_at,
						fired_at: now_ms,
					},
					history_id: None,
					origin,
				});
				continue;
			}

			let next_trigger_at = next_recurring_trigger(&event, now_ms)?;
			let is_running = self
				.0
				.schedule_running
				.insert_sync(event.event_id.clone())
				.is_err();
			let mut statements = vec![SqliteBatchStatement {
				sql: if is_running {
					ADVANCE_SKIPPED_SCHEDULE_SQL.to_owned()
				} else {
					ADVANCE_SCHEDULE_SQL.to_owned()
				},
				params: Some(if is_running {
					vec![
						BindParam::Integer(next_trigger_at),
						BindParam::Text(event.event_id.clone()),
					]
				} else {
					vec![
						BindParam::Integer(next_trigger_at),
						BindParam::Integer(now_ms),
						BindParam::Text(event.event_id.clone()),
					]
				}),
			}];
			let history_result = if is_running {
				HISTORY_SKIPPED
			} else {
				HISTORY_RUNNING
			};
			let prune_global_history = event.max_history > 0 && self.should_prune_global_history();
			let history_index = append_history_statements(
				&mut statements,
				&event,
				now_ms,
				history_result,
				prune_global_history,
			)?;
			let results = match self.sql().execute_batch(statements).await {
				Ok(results) => results,
				Err(error) => {
					if !is_running {
						self.0.schedule_running.remove_sync(&event.event_id);
					}
					return Err(error).context("advance due recurring schedule");
				}
			};
			if event.max_history > 0 {
				self.record_schedule_history_inserted();
			}
			if is_running {
				continue;
			}
			let history_id = history_index
				.and_then(|index| results.get(index))
				.and_then(|result| result.last_insert_row_id);
			let name = cron_name(&event.event_id)?.to_owned();
			dispatches.push(DueScheduleDispatch {
				event_id: event.event_id.clone(),
				action: event.action,
				args: event.args,
				fire: ScheduledFireInfo {
					kind: event.kind,
					id: name.clone(),
					name: Some(name),
					scheduled_at: event.trigger_at,
					fired_at: now_ms,
				},
				history_id,
				origin,
			});
		}
		self.mark_schedule_dirty();
		if !result.rows.is_empty() {
			self.record_schedules_updated();
		}
		// The due rows are already claimed/advanced at this point. Alarm resync is
		// best effort so a transient sync failure cannot discard valid dispatches.
		self.sync_alarm_logged().await;
		Ok(dispatches)
	}

	pub(crate) async fn finish_schedule_dispatch(
		&self,
		event_id: &str,
		history_id: Option<i64>,
		error: Option<&anyhow::Error>,
	) {
		self.0.schedule_running.remove_sync(event_id);
		let Some(history_id) = history_id else {
			return;
		};
		let finished_at = self.schedule_now_timestamp_ms();
		let (result, error) = match error {
			Some(error) => (HISTORY_ERROR, Some(sanitize_error(error))),
			None => (HISTORY_OK, None),
		};
		let error_metadata = error
			.as_ref()
			.and_then(|error| error.metadata.as_ref())
			.and_then(|metadata| encode_error_metadata(metadata).ok());
		match self
			.sql()
			.execute(
				FINISH_HISTORY_SQL,
				Some(vec![
					BindParam::Integer(finished_at),
					BindParam::Integer(result),
					optional_owned_text_param(error.as_ref().map(|error| error.group.clone())),
					optional_owned_text_param(error.as_ref().map(|error| error.code.clone())),
					optional_owned_text_param(error.as_ref().map(|error| error.message.clone())),
					error_metadata
						.map(BindParam::Blob)
						.unwrap_or(BindParam::Null),
					BindParam::Integer(history_id),
					BindParam::Integer(HISTORY_RUNNING),
				]),
			)
			.await
		{
			Ok(_) => self.record_schedules_updated(),
			Err(error) => {
				tracing::error!(?error, history_id, "failed to finish schedule history row");
			}
		}
	}

	pub(crate) async fn recover_interrupted_schedule_history(&self) -> Result<()> {
		let error = ScheduleErrorInfo {
			group: "schedule".to_owned(),
			code: "interrupted".to_owned(),
			message: "Scheduled action was interrupted before completion.".to_owned(),
			metadata: None,
		};
		self.sql()
			.execute(
				RECOVER_HISTORY_SQL,
				Some(vec![
					BindParam::Integer(self.schedule_now_timestamp_ms()),
					BindParam::Integer(HISTORY_ERROR),
					BindParam::Text(error.group),
					BindParam::Text(error.code),
					BindParam::Text(error.message),
					BindParam::Null,
				]),
			)
			.await
			.context("recover interrupted schedule history")?;
		self.record_schedules_updated();
		Ok(())
	}

	async fn prune_schedule_history(&self, event_id: &str, max_history: i64) -> Result<()> {
		self.sql()
			.execute_batch(history_prune_statements(event_id, max_history, false))
			.await
			.context("prune schedule history")?;
		Ok(())
	}

	fn should_prune_global_history(&self) -> bool {
		self.0.schedule_history_insert_count.load(Ordering::Relaxed) % GLOBAL_HISTORY_PRUNE_INTERVAL
			== 0
	}

	fn record_schedule_history_inserted(&self) {
		self.0
			.schedule_history_insert_count
			.fetch_add(1, Ordering::Relaxed);
	}

	fn mark_schedule_dirty(&self) {
		self.0
			.schedule_dirty_since_push
			.store(true, Ordering::SeqCst);
	}

	/// Sets the durable Unix-millisecond deadline that should ensure the foreign
	/// run handler is active, starting it if inactive. This deadline shares one
	/// physical alarm with scheduled actions, but remains independently
	/// addressable and clearable.
	pub async fn set_run_wake_at(&self, wake_at: Option<i64>) -> Result<()> {
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		self.persist_run_wake_at(wake_at).await?;
		self.0.run_wake_revision.fetch_add(1, Ordering::SeqCst);
		self.mark_schedule_dirty();
		self.sync_alarm().await
	}

	pub(crate) async fn consume_due_run_wake(&self) -> Result<Option<(i64, u64)>> {
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		let Some(wake_at) = self.run_wake_at() else {
			return Ok(None);
		};
		if wake_at > self.schedule_now_timestamp_ms() {
			return Ok(None);
		}
		self.persist_run_wake_at(None).await?;
		let revision = self.0.run_wake_revision.fetch_add(1, Ordering::SeqCst) + 1;
		self.mark_schedule_dirty();
		Ok(Some((wake_at, revision)))
	}

	#[doc(hidden)]
	pub async fn restore_run_wake_at_if_unchanged(
		&self,
		wake_at: i64,
		consumed_revision: u64,
	) -> Result<bool> {
		let _mutation = self.0.schedule_mutation_lock.lock().await;
		if self.0.run_wake_revision.load(Ordering::SeqCst) != consumed_revision {
			return Ok(false);
		}
		self.persist_run_wake_at(Some(wake_at)).await?;
		self.0.run_wake_revision.fetch_add(1, Ordering::SeqCst);
		self.mark_schedule_dirty();
		self.sync_alarm().await?;
		Ok(true)
	}

	async fn next_schedule_timestamp(&self, future_only: bool) -> Result<Option<i64>> {
		let (sql, params) = if future_only {
			(
				NEXT_FUTURE_SCHEDULE_SQL,
				Some(vec![BindParam::Integer(self.schedule_now_timestamp_ms())]),
			)
		} else {
			(NEXT_SCHEDULE_SQL, None)
		};
		let result = self.sql().query(sql, params).await?;
		match result.rows.first().and_then(|row| row.first()) {
			None | Some(ColumnValue::Null) => Ok(None),
			Some(ColumnValue::Integer(timestamp)) => Ok(Some(*timestamp)),
			Some(_) => Err(ScheduleRuntimeError::InvalidScheduleRow {
				schedule_id: "<minimum>".to_owned(),
				reason: "MIN(trigger_at) was not an integer".to_owned(),
			}
			.build()),
		}
	}

	async fn sync_alarm(&self) -> Result<()> {
		#[cfg(test)]
		if self
			.0
			.schedule_sync_alarm_failures
			.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
				remaining.checked_sub(1)
			})
			.is_ok()
		{
			anyhow::bail!("injected schedule alarm sync failure");
		}
		let next_alarm = min_deadline(
			self.next_schedule_timestamp(false).await?,
			self.run_wake_at(),
		);
		self.sync_alarm_timestamp(next_alarm)
	}

	async fn sync_future_alarm(&self) -> Result<()> {
		let next_alarm = min_deadline(
			self.next_schedule_timestamp(true).await?,
			self.run_wake_at(),
		);
		self.sync_alarm_timestamp(next_alarm)
	}

	fn sync_alarm_timestamp(&self, next_alarm: Option<i64>) -> Result<()> {
		let should_push = self
			.0
			.schedule_dirty_since_push
			.swap(false, Ordering::SeqCst);
		self.arm_local_alarm(next_alarm);
		if !should_push {
			return Ok(());
		}
		if next_alarm.is_some() && self.last_pushed_alarm() == next_alarm {
			return Ok(());
		}
		let Some(envoy_handle) = self.0.schedule_envoy_handle.lock().clone() else {
			self.mark_schedule_dirty();
			tracing::warn!(
				actor_id = self.actor_id(),
				"schedule alarm sync skipped because envoy handle is not configured"
			);
			return Ok(());
		};
		let generation = *self.0.schedule_generation.lock();
		self.set_alarm_tracked(envoy_handle, next_alarm, generation);
		Ok(())
	}

	pub(crate) async fn sync_alarm_logged(&self) {
		if let Err(error) = self.sync_alarm().await {
			tracing::error!(actor_id = %self.actor_id(), ?error, "failed to sync scheduled actor alarm");
		}
	}

	pub(crate) async fn sync_future_alarm_logged(&self) {
		if let Err(error) = self.sync_future_alarm().await {
			tracing::error!(actor_id = %self.actor_id(), ?error, "failed to sync future scheduled actor alarm");
		}
	}

	pub(crate) fn set_schedule_alarm(&self, timestamp_ms: Option<i64>) -> Result<()> {
		let envoy_handle = self.0.schedule_envoy_handle.lock().clone().ok_or_else(|| {
			crate::error::ActorRuntime::NotConfigured {
				component: "schedule alarm handle".to_owned(),
			}
			.build()
		})?;
		let generation = *self.0.schedule_generation.lock();
		self.set_alarm_tracked(envoy_handle, timestamp_ms, generation);
		Ok(())
	}

	pub(crate) fn configure_schedule_envoy(
		&self,
		envoy_handle: EnvoyHandle,
		generation: Option<u32>,
	) {
		*self.0.schedule_envoy_handle.lock() = Some(envoy_handle);
		*self.0.schedule_generation.lock() = generation;
	}

	pub(crate) fn set_internal_keep_awake(&self, callback: Option<InternalKeepAwakeCallback>) {
		*self.0.schedule_internal_keep_awake.lock() = callback;
	}

	pub(crate) fn set_local_alarm_callback(&self, callback: Option<LocalAlarmCallback>) {
		*self.0.schedule_local_alarm_callback.lock() = callback;
	}

	pub(crate) fn cancel_local_alarm_timeouts(&self) {
		self.0
			.schedule_local_alarm_epoch
			.fetch_add(1, Ordering::SeqCst);
		if let Some(handle) = self.0.schedule_local_alarm_task.lock().take() {
			handle.abort();
		}
	}

	pub(crate) fn cancel_driver_alarm_logged(&self) {
		self.cancel_local_alarm_timeouts();
		#[cfg(test)]
		self.0
			.schedule_driver_alarm_cancel_count
			.fetch_add(1, Ordering::SeqCst);
		let Some(envoy_handle) = self.0.schedule_envoy_handle.lock().clone() else {
			return;
		};
		let generation = *self.0.schedule_generation.lock();
		self.set_alarm_tracked(envoy_handle, None, generation);
	}

	#[cfg(test)]
	pub(crate) fn test_driver_alarm_cancel_count(&self) -> usize {
		self.0
			.schedule_driver_alarm_cancel_count
			.load(Ordering::SeqCst)
	}

	#[cfg(test)]
	pub(crate) fn fail_next_schedule_alarm_sync_for_tests(&self) {
		self.0
			.schedule_sync_alarm_failures
			.fetch_add(1, Ordering::SeqCst);
	}

	pub(crate) async fn wait_for_pending_alarm_writes(&self) {
		let pending = {
			let mut guard = self.0.schedule_pending_alarm_writes.lock();
			std::mem::take(&mut *guard)
		};
		for ack_rx in pending {
			let _ = ack_rx.await;
		}
	}

	fn set_alarm_tracked(
		&self,
		envoy_handle: EnvoyHandle,
		timestamp_ms: Option<i64>,
		generation: Option<u32>,
	) {
		let push_epoch = self
			.0
			.schedule_alarm_push_epoch
			.fetch_add(1, Ordering::SeqCst)
			.wrapping_add(1);
		let (ack_tx, ack_rx) = oneshot::channel();
		envoy_handle.set_alarm_with_ack(
			self.actor_id().to_owned(),
			timestamp_ms,
			generation,
			Some(ack_tx),
		);
		self.load_last_pushed_alarm(timestamp_ms);
		if let Ok(handle) = Handle::try_current() {
			let state_ctx = self.clone();
			let (persist_done_tx, persist_done_rx) = oneshot::channel();
			handle.spawn(
				async move {
					let _ = ack_rx.await;
					let is_current = state_ctx.0.schedule_alarm_push_epoch.load(Ordering::SeqCst)
						== push_epoch && state_ctx.last_pushed_alarm() == timestamp_ms;
					if is_current
						&& let Err(error) = state_ctx.persist_last_pushed_alarm(timestamp_ms).await
					{
						tracing::error!(
							?error,
							?timestamp_ms,
							"failed to persist last pushed actor alarm"
						);
					}
					let _ = persist_done_tx.send(());
				}
				.in_current_span(),
			);
			self.0
				.schedule_pending_alarm_writes
				.lock()
				.push(persist_done_rx);
			return;
		}
		self.0.schedule_pending_alarm_writes.lock().push(ack_rx);
	}

	fn arm_local_alarm(&self, next_alarm: Option<i64>) {
		self.cancel_local_alarm_timeouts();
		let Some(next_alarm) = next_alarm else {
			return;
		};
		if self.0.schedule_local_alarm_callback.lock().is_none() {
			return;
		}
		#[cfg(not(feature = "wasm-runtime"))]
		let tokio_handle = match Handle::try_current() {
			Ok(handle) => handle,
			Err(_) => return,
		};
		let delay_ms = next_alarm
			.saturating_sub(self.schedule_now_timestamp_ms())
			.max(0) as u64;
		let local_alarm_epoch = self.0.schedule_local_alarm_epoch.load(Ordering::SeqCst);
		let schedule = self.clone();
		let task = async move {
			sleep(Duration::from_millis(delay_ms)).await;
			if schedule.0.schedule_local_alarm_epoch.load(Ordering::SeqCst) != local_alarm_epoch {
				return;
			}
			let Some(callback) = schedule.0.schedule_local_alarm_callback.lock().clone() else {
				return;
			};
			callback().await;
		}
		.in_current_span();
		#[cfg(not(feature = "wasm-runtime"))]
		let handle = tokio_handle.spawn(task);
		#[cfg(feature = "wasm-runtime")]
		let handle = {
			let (handle, registration) = AbortHandle::new_pair();
			// Public Wasm callbacks run on the wasm-bindgen executor, outside the
			// Tokio LocalSet that owns the actor loop. Keep callback-triggered
			// alarm timers on that same executor so they can be armed safely.
			wasm_bindgen_futures::spawn_local(async move {
				let _ = Abortable::new(task, registration).await;
			});
			handle
		};
		*self.0.schedule_local_alarm_task.lock() = Some(handle);
	}

	pub(crate) fn suspend_alarm_dispatch(&self) {
		self.0
			.schedule_alarm_dispatch_enabled
			.store(false, Ordering::SeqCst);
	}
}

fn validate_name(name: &str) -> Result<()> {
	let reason = if name.is_empty() {
		Some("must not be empty")
	} else if name.len() > 128 {
		Some("must be at most 128 UTF-8 bytes")
	} else {
		None
	};
	match reason {
		Some(reason) => Err(ScheduleRuntimeError::InvalidName {
			reason: reason.to_owned(),
		}
		.build()),
		None => Ok(()),
	}
}

fn validate_max_history(max_history: Option<i64>) -> Result<i64> {
	let max_history = max_history.unwrap_or(DEFAULT_MAX_HISTORY);
	if !(0..=MAX_HISTORY).contains(&max_history) {
		return Err(ScheduleRuntimeError::InvalidMaxHistory {
			max_history,
			maximum: MAX_HISTORY,
		}
		.build());
	}
	Ok(max_history)
}

fn parse_timezone(timezone: &str) -> Result<Tz> {
	timezone.parse().map_err(|_| {
		ScheduleRuntimeError::InvalidTimezone {
			timezone: timezone.to_owned(),
		}
		.build()
	})
}

fn parse_cron(expression: &str) -> Result<Cron> {
	if expression.split_whitespace().count() != 5 {
		return Err(ScheduleRuntimeError::InvalidCronExpression {
			reason: "expected exactly five fields".to_owned(),
		}
		.build());
	}
	Cron::new(expression).parse().map_err(|error| {
		ScheduleRuntimeError::InvalidCronExpression {
			reason: error.to_string(),
		}
		.build()
	})
}

fn next_cron_timestamp_ms(cron: &Cron, timezone: Tz, after_ms: i64) -> Result<i64> {
	let after_utc = DateTime::<Utc>::from_timestamp_millis(after_ms).ok_or_else(|| {
		ScheduleRuntimeError::InvalidCronExpression {
			reason: "start time is outside the supported range".to_owned(),
		}
		.build()
	})?;
	let mut cursor = after_utc.with_timezone(&timezone);
	loop {
		let next = cron.find_next_occurrence(&cursor, false).map_err(|error| {
			ScheduleRuntimeError::InvalidCronExpression {
				reason: error.to_string(),
			}
			.build()
		})?;
		if cron.is_time_matching(&next).unwrap_or(false) {
			return Ok(next.timestamp_millis());
		}
		// Croner advances nonexistent DST wall times to the end of the gap. V1
		// semantics skip that occurrence, so search again from the adjusted time.
		cursor = next;
	}
}

fn next_recurring_trigger(event: &StoredSchedule, now_ms: i64) -> Result<i64> {
	match event.kind {
		ScheduleKind::Cron => {
			let expression = event
				.cron_expression
				.as_deref()
				.ok_or_else(|| invalid_row(&event.event_id, "cron expression is missing"))?;
			let timezone = event
				.timezone
				.as_deref()
				.ok_or_else(|| invalid_row(&event.event_id, "timezone is missing"))?;
			next_cron_timestamp_ms(&parse_cron(expression)?, parse_timezone(timezone)?, now_ms)
		}
		ScheduleKind::Every => {
			let interval_ms = event
				.interval_ms
				.ok_or_else(|| invalid_row(&event.event_id, "interval is missing"))?;
			if interval_ms < MIN_INTERVAL_MS {
				return Err(invalid_row(&event.event_id, "interval is below minimum"));
			}
			let elapsed = now_ms.saturating_sub(event.trigger_at).max(0);
			let steps = elapsed / interval_ms + 1;
			Ok(event
				.trigger_at
				.saturating_add(interval_ms.saturating_mul(steps)))
		}
		ScheduleKind::At => Err(invalid_row(
			&event.event_id,
			"one-shot passed to recurring calculation",
		)),
	}
}

fn append_history_statements(
	statements: &mut Vec<SqliteBatchStatement>,
	event: &StoredSchedule,
	now_ms: i64,
	result: i64,
	prune_global_history: bool,
) -> Result<Option<usize>> {
	if event.max_history == 0 {
		return Ok(None);
	}
	let index = statements.len();
	statements.push(SqliteBatchStatement {
		sql: INSERT_SCHEDULE_HISTORY_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(event.event_id.clone()),
			BindParam::Text(event.action.clone()),
			BindParam::Integer(event.trigger_at),
			BindParam::Integer(now_ms),
			if result == HISTORY_SKIPPED {
				BindParam::Integer(now_ms)
			} else {
				BindParam::Null
			},
			BindParam::Integer(result),
			BindParam::Null,
			BindParam::Null,
			BindParam::Null,
			BindParam::Null,
		]),
	});
	statements.extend(history_prune_statements(
		&event.event_id,
		event.max_history,
		prune_global_history,
	));
	Ok(Some(index))
}

fn history_prune_statements(
	event_id: &str,
	max_history: i64,
	prune_global_history: bool,
) -> Vec<SqliteBatchStatement> {
	let mut statements = vec![SqliteBatchStatement {
		sql: PRUNE_SCHEDULE_HISTORY_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(event_id.to_owned()),
			BindParam::Integer(max_history),
		]),
	}];
	if prune_global_history {
		statements.push(SqliteBatchStatement {
			sql: PRUNE_GLOBAL_HISTORY_SQL.to_owned(),
			params: Some(vec![BindParam::Integer(GLOBAL_HISTORY_RETAINED_ROWS)]),
		});
	}
	statements
}

fn stored_to_cron_info(event: StoredSchedule) -> Result<CronJobInfo> {
	if event.kind == ScheduleKind::At {
		return Err(invalid_row(&event.event_id, "expected recurring schedule"));
	}
	Ok(CronJobInfo {
		name: cron_name(&event.event_id)?.to_owned(),
		kind: event.kind,
		action: event.action,
		args: event.args,
		next_run_at: event.trigger_at,
		last_run_at: event.last_started_at,
		expression: event.cron_expression,
		timezone: event.timezone,
		interval_ms: event.interval_ms,
		max_history: event.max_history,
	})
}

fn read_stored_schedule(row: &[ColumnValue]) -> Result<StoredSchedule> {
	let event_id = read_text(row, 0, "event_id")?;
	let kind = ScheduleKind::parse(read_i64(row, 4, "kind")?, &event_id)?;
	let event = StoredSchedule {
		event_id: event_id.clone(),
		trigger_at: read_i64(row, 1, "trigger_at")?,
		action: read_text(row, 2, "action")?,
		args: read_optional_blob(row, 3, "args")?.unwrap_or_default(),
		kind,
		cron_expression: read_optional_text(row, 5, "cron_expression")?,
		timezone: read_optional_text(row, 6, "timezone")?,
		interval_ms: read_optional_i64(row, 7, "interval_ms")?,
		last_started_at: read_optional_i64(row, 8, "last_started_at")?,
		max_history: read_i64(row, 9, "max_history")?,
	};
	match event.kind {
		ScheduleKind::At
			if event.cron_expression.is_some()
				|| event.timezone.is_some()
				|| event.interval_ms.is_some()
				|| event.max_history != 0 =>
		{
			Err(invalid_row(&event_id, "invalid one-shot columns"))
		}
		ScheduleKind::Cron
			if event.cron_expression.is_none()
				|| event.timezone.is_none()
				|| event.interval_ms.is_some() =>
		{
			Err(invalid_row(&event_id, "invalid cron columns"))
		}
		ScheduleKind::Every
			if event.cron_expression.is_some()
				|| event.timezone.is_some()
				|| event
					.interval_ms
					.is_none_or(|value| value < MIN_INTERVAL_MS) =>
		{
			Err(invalid_row(&event_id, "invalid interval columns"))
		}
		_ if !(0..=MAX_HISTORY).contains(&event.max_history) => {
			Err(invalid_row(&event_id, "max_history is out of range"))
		}
		_ => Ok(event),
	}
}

fn read_due_schedule(row: &[ColumnValue]) -> Result<(StoredSchedule, ScheduleTraceOrigin)> {
	let event = read_stored_schedule(row)?;
	let origin = read_optional_blob(row, 10, "schedule trace context")?
		.and_then(|payload| {
			decode_latest_with_embedded_version::<ScheduleTraceContext>(
				&payload,
				"schedule trace context",
			)
			.inspect_err(|error| {
				tracing::warn!(
					event_id = %event.event_id,
					?error,
					"ignoring undecodable schedule trace context"
				);
			})
			.ok()
		})
		.map_or_else(ScheduleTraceOrigin::default, |context| {
			ScheduleTraceOrigin {
				ray_id: context.ray_id,
				traceparent: context.traceparent,
				tracestate: context.tracestate,
			}
		});
	Ok((event, origin))
}

/// Stores the defining invocation's trace context beside a schedule row, or
/// clears a stale one when the definer carried no context.
fn append_schedule_trace_context_upsert(
	statements: &mut Vec<SqliteBatchStatement>,
	event_id: &str,
	origin: ScheduleTraceOrigin,
) -> Result<()> {
	let key = schedule_trace_context_key(event_id);
	if origin.ray_id.is_none() && origin.traceparent.is_none() && origin.tracestate.is_none() {
		statements.push(SqliteBatchStatement {
			sql: DELETE_SCHEDULE_TRACE_CONTEXT_SQL.to_owned(),
			params: Some(vec![BindParam::Text(key)]),
		});
		return Ok(());
	}
	let payload = encode_latest_with_embedded_version::<ScheduleTraceContext>(
		ScheduleTraceContextData {
			ray_id: origin.ray_id,
			traceparent: origin.traceparent,
			tracestate: origin.tracestate,
		},
		SCHEDULE_TRACE_CONTEXT_VERSION,
		"schedule trace context",
	)?;
	statements.push(SqliteBatchStatement {
		sql: UPSERT_SCHEDULE_TRACE_CONTEXT_SQL.to_owned(),
		params: Some(vec![BindParam::Text(key), BindParam::Blob(payload)]),
	});
	Ok(())
}

/// Removes a schedule's trace context once its row is gone. Runs after the
/// row delete in the same batch so a mismatched delete leaves the context alone.
fn delete_orphan_schedule_trace_context(event_id: &str) -> SqliteBatchStatement {
	SqliteBatchStatement {
		sql: DELETE_ORPHAN_SCHEDULE_TRACE_CONTEXT_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(schedule_trace_context_key(event_id)),
			BindParam::Text(event_id.to_owned()),
		]),
	}
}

fn schedule_trace_context_key(event_id: &str) -> String {
	format!("{SCHEDULE_TRACE_CONTEXT_META_PREFIX}{event_id}")
}

fn read_cron_fire(row: &[ColumnValue]) -> Result<CronFire> {
	let error_group = read_optional_text(row, 5, "error_group")?;
	let error_code = read_optional_text(row, 6, "error_code")?;
	let error_message = read_optional_text(row, 7, "error_message")?;
	let error_metadata = read_optional_blob(row, 8, "error_metadata")?
		.map(|value| decode_error_metadata(&value))
		.transpose()?;
	let error = match (error_group, error_code, error_message) {
		(None, None, None) => None,
		(Some(group), Some(code), Some(message)) => Some(ScheduleErrorInfo {
			group,
			code,
			message,
			metadata: error_metadata,
		}),
		_ => return Err(invalid_row("<history>", "error columns are incomplete")),
	};
	Ok(CronFire {
		action: read_text(row, 0, "action")?,
		scheduled_at: read_i64(row, 1, "scheduled_at")?,
		fired_at: read_i64(row, 2, "fired_at")?,
		finished_at: read_optional_i64(row, 3, "finished_at")?,
		result: history_result_name(read_i64(row, 4, "result")?)?.to_owned(),
		error,
	})
}

fn sanitize_error(error: &anyhow::Error) -> ScheduleErrorInfo {
	let extracted = RivetError::extract(error);
	let metadata = extracted.metadata();
	ScheduleErrorInfo {
		group: extracted.group().to_owned(),
		code: extracted.code().to_owned(),
		message: client_error_message(extracted.group(), extracted.code(), extracted.message())
			.to_owned(),
		metadata: client_error_metadata(extracted.group(), extracted.code(), metadata.as_ref())
			.cloned(),
	}
}

fn encode_error_metadata(metadata: &serde_json::Value) -> Result<Vec<u8>> {
	let mut output = Vec::new();
	ciborium::into_writer(metadata, &mut output)
		.context("encode schedule history error metadata")?;
	Ok(output)
}

fn decode_error_metadata(value: &[u8]) -> Result<serde_json::Value> {
	ciborium::from_reader(value).context("decode schedule history error metadata")
}

fn history_result_name(value: i64) -> Result<&'static str> {
	match value {
		HISTORY_RUNNING => Ok("running"),
		HISTORY_OK => Ok("ok"),
		HISTORY_ERROR => Ok("error"),
		HISTORY_SKIPPED => Ok("skipped"),
		_ => Err(invalid_row("<history>", "unknown result")),
	}
}

fn cron_event_id(name: &str) -> String {
	format!("{CRON_ID_PREFIX}{name}")
}

fn cron_name(event_id: &str) -> Result<&str> {
	event_id
		.strip_prefix(CRON_ID_PREFIX)
		.ok_or_else(|| invalid_row(event_id, "recurring id is missing cron prefix"))
}

fn invalid_row(schedule_id: &str, reason: &str) -> anyhow::Error {
	ScheduleRuntimeError::InvalidScheduleRow {
		schedule_id: schedule_id.to_owned(),
		reason: reason.to_owned(),
	}
	.build()
}

fn args_param(args: &[u8]) -> BindParam {
	if args.is_empty() {
		BindParam::Null
	} else {
		BindParam::Blob(args.to_vec())
	}
}

fn optional_text_param(value: Option<&str>) -> BindParam {
	value
		.map(|value| BindParam::Text(value.to_owned()))
		.unwrap_or(BindParam::Null)
}

fn optional_owned_text_param(value: Option<String>) -> BindParam {
	value.map(BindParam::Text).unwrap_or(BindParam::Null)
}

fn optional_i64_param(value: Option<i64>) -> BindParam {
	value.map(BindParam::Integer).unwrap_or(BindParam::Null)
}

fn read_text(row: &[ColumnValue], index: usize, label: &str) -> Result<String> {
	match row.get(index) {
		Some(ColumnValue::Text(value)) => Ok(value.clone()),
		_ => Err(invalid_row("<decode>", &format!("{label} is not text"))),
	}
}

fn read_optional_text(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<String>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Text(value)) => Ok(Some(value.clone())),
		_ => Err(invalid_row("<decode>", &format!("{label} is not text"))),
	}
}

fn read_i64(row: &[ColumnValue], index: usize, label: &str) -> Result<i64> {
	match row.get(index) {
		Some(ColumnValue::Integer(value)) => Ok(*value),
		_ => Err(invalid_row(
			"<decode>",
			&format!("{label} is not an integer"),
		)),
	}
}

fn read_optional_i64(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<i64>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Integer(value)) => Ok(Some(*value)),
		_ => Err(invalid_row(
			"<decode>",
			&format!("{label} is not an integer"),
		)),
	}
}

fn read_optional_blob(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<Vec<u8>>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Blob(value)) => Ok(Some(value.clone())),
		_ => Err(invalid_row("<decode>", &format!("{label} is not a blob"))),
	}
}

fn system_now_timestamp_ms() -> i64 {
	let duration = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default();
	i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
#[path = "../../tests/schedule.rs"]
mod tests;
