use std::time::Duration;

use crate::actor_context::{StateDeltaPayload, state_deltas_from_payload};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use rivetkit_core::ActorStateTransaction as CoreActorStateTransaction;
use rivetkit_core::sqlite::{
	BindParam, ColumnValue, ExecuteResult as CoreExecuteResult, QueryResult as CoreQueryResult,
	SqliteBatchStatement as CoreSqliteBatchStatement, SqliteDb as CoreSqliteDb,
	SqliteTransaction as CoreSqliteTransaction,
};

use crate::{NapiInvalidArgument, napi_anyhow_error};
#[napi]
#[derive(Clone)]
pub struct JsNativeDatabase {
	db: CoreSqliteDb,
	actor_id: Option<String>,
}

#[napi]
#[derive(Clone)]
pub struct JsSqliteTransaction {
	transaction: CoreSqliteTransaction,
}
#[napi]
#[derive(Clone)]
pub struct JsActorStateTransaction {
	transaction: CoreActorStateTransaction,
}

impl JsActorStateTransaction {
	pub(crate) fn new(transaction: CoreActorStateTransaction) -> Self {
		Self { transaction }
	}
}

impl JsNativeDatabase {
	pub(crate) fn new(db: CoreSqliteDb, actor_id: Option<String>) -> Self {
		tracing::debug!(
			class = "JsNativeDatabase",
			actor_id = actor_id.as_deref().unwrap_or("<unknown>"),
			"constructed napi class"
		);
		Self { db, actor_id }
	}
}

impl Drop for JsNativeDatabase {
	fn drop(&mut self) {
		tracing::debug!(
			class = "JsNativeDatabase",
			actor_id = self.actor_id.as_deref().unwrap_or("<unknown>"),
			"dropped napi class"
		);
	}
}

#[napi(object)]
pub struct JsBindParam {
	pub kind: String,
	pub int_value: Option<i64>,
	pub float_value: Option<f64>,
	pub text_value: Option<String>,
	pub blob_value: Option<Buffer>,
}

#[napi(object)]
pub struct ExecuteResult {
	pub changes: i64,
}

#[napi(object)]
pub struct QueryResult {
	pub columns: Vec<String>,
	pub rows: Vec<Vec<serde_json::Value>>,
}

#[napi(object)]
pub struct NativeExecuteResult {
	pub columns: Vec<String>,
	pub rows: Vec<Vec<serde_json::Value>>,
	pub changes: i64,
	pub last_insert_row_id: Option<i64>,
}

#[napi(object)]
pub struct JsSqliteBatchStatement {
	pub sql: String,
	pub params: Option<Vec<JsBindParam>>,
}

#[napi(object)]
pub struct JsSqliteVfsMetrics {
	pub request_build_ns: f64,
	pub serialize_ns: f64,
	pub transport_ns: f64,
	pub state_update_ns: f64,
	pub total_ns: f64,
	pub commit_count: f64,
	pub page_cache_entries: f64,
	pub page_cache_weighted_size: f64,
	pub page_cache_capacity_pages: f64,
	pub write_buffer_dirty_pages: f64,
	pub db_size_pages: f64,
}

#[napi]
impl JsNativeDatabase {
	#[napi]
	pub fn take_last_kv_error(&self) -> Option<String> {
		self.db.take_last_kv_error()
	}

	#[napi]
	pub fn metrics(&self) -> Option<JsSqliteVfsMetrics> {
		self.db.metrics().map(|metrics| JsSqliteVfsMetrics {
			request_build_ns: metrics.request_build_ns as f64,
			serialize_ns: metrics.serialize_ns as f64,
			transport_ns: metrics.transport_ns as f64,
			state_update_ns: metrics.state_update_ns as f64,
			total_ns: metrics.total_ns as f64,
			commit_count: metrics.commit_count as f64,
			page_cache_entries: metrics.page_cache_entries as f64,
			page_cache_weighted_size: metrics.page_cache_weighted_size as f64,
			page_cache_capacity_pages: metrics.page_cache_capacity_pages as f64,
			write_buffer_dirty_pages: metrics.write_buffer_dirty_pages as f64,
			db_size_pages: metrics.db_size_pages as f64,
		})
	}

	#[napi]
	pub async fn run(
		&self,
		sql: String,
		params: Option<Vec<JsBindParam>>,
	) -> napi::Result<ExecuteResult> {
		let params = params.map(js_bind_params_to_core).transpose()?;
		let result = self
			.db
			.run(sql, params)
			.await
			.map_err(crate::napi_anyhow_error)?;
		Ok(ExecuteResult {
			changes: result.changes,
		})
	}

	#[napi]
	pub async fn query(
		&self,
		sql: String,
		params: Option<Vec<JsBindParam>>,
	) -> napi::Result<QueryResult> {
		let params = params.map(js_bind_params_to_core).transpose()?;
		let result = self
			.db
			.query(sql, params)
			.await
			.map_err(crate::napi_anyhow_error)?;
		Ok(core_query_result_to_js(result))
	}

	#[napi]
	pub async fn execute(
		&self,
		sql: String,
		params: Option<Vec<JsBindParam>>,
	) -> napi::Result<NativeExecuteResult> {
		let params = params.map(js_bind_params_to_core).transpose()?;
		let result = self
			.db
			.execute(sql, params)
			.await
			.map_err(crate::napi_anyhow_error)?;
		Ok(core_execute_result_to_js(result))
	}

	#[napi]
	pub async fn execute_batch(
		&self,
		statements: Vec<JsSqliteBatchStatement>,
	) -> napi::Result<Vec<NativeExecuteResult>> {
		let statements = js_batch_statements_to_core(statements)?;
		let results = self
			.db
			.execute_batch(statements)
			.await
			.map_err(crate::napi_anyhow_error)?;
		Ok(results.into_iter().map(core_execute_result_to_js).collect())
	}

	#[napi]
	pub async fn exec(&self, sql: String) -> napi::Result<QueryResult> {
		let result = self.db.exec(sql).await.map_err(crate::napi_anyhow_error)?;
		Ok(core_query_result_to_js(result))
	}

	#[napi]
	pub async fn close(&self) -> napi::Result<()> {
		self.db.close().await.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn begin_transaction(
		&self,
		timeout_ms: Option<f64>,
		name: Option<String>,
	) -> napi::Result<JsSqliteTransaction> {
		let timeout = timeout_ms.map(transaction_timeout).transpose()?;
		let transaction = self
			.db
			.begin_named_transaction(name.as_deref(), timeout)
			.await
			.map_err(crate::napi_anyhow_error)?;
		Ok(JsSqliteTransaction { transaction })
	}
}

#[napi]
impl JsSqliteTransaction {
	#[napi]
	pub async fn execute(
		&self,
		sql: String,
		params: Option<Vec<JsBindParam>>,
	) -> napi::Result<NativeExecuteResult> {
		let params = params.map(js_bind_params_to_core).transpose()?;
		self.transaction
			.execute(sql, params)
			.await
			.map(core_execute_result_to_js)
			.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn exec(&self, sql: String) -> napi::Result<QueryResult> {
		self.transaction
			.exec(sql)
			.await
			.map(core_query_result_to_js)
			.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn commit(&self) -> napi::Result<()> {
		self.transaction
			.commit()
			.await
			.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn rollback(&self) -> napi::Result<()> {
		self.transaction
			.rollback()
			.await
			.map_err(crate::napi_anyhow_error)
	}
}

#[napi]
impl JsActorStateTransaction {
	#[napi]
	pub async fn execute(
		&self,
		sql: String,
		params: Option<Vec<JsBindParam>>,
	) -> napi::Result<NativeExecuteResult> {
		let params = params.map(js_bind_params_to_core).transpose()?;
		self.transaction
			.execute(sql, params)
			.await
			.map(core_execute_result_to_js)
			.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn commit(&self, payload: StateDeltaPayload) -> napi::Result<()> {
		self.transaction
			.commit(state_deltas_from_payload(payload))
			.await
			.map_err(crate::napi_anyhow_error)
	}

	#[napi]
	pub async fn rollback(&self) -> napi::Result<()> {
		self.transaction
			.rollback()
			.await
			.map_err(crate::napi_anyhow_error)
	}
}

pub(crate) fn transaction_timeout(timeout_ms: f64) -> napi::Result<Duration> {
	if !timeout_ms.is_finite() || timeout_ms <= 0.0 {
		return Err(napi_anyhow_error(
			NapiInvalidArgument {
				argument: "timeout".to_owned(),
				reason: "must be a positive finite number of milliseconds".to_owned(),
			}
			.build(),
		));
	}
	Duration::try_from_secs_f64(timeout_ms / 1_000.0).map_err(|_| {
		napi_anyhow_error(
			NapiInvalidArgument {
				argument: "timeout".to_owned(),
				reason: "is too large to represent".to_owned(),
			}
			.build(),
		)
	})
}

fn js_bind_params_to_core(params: Vec<JsBindParam>) -> napi::Result<Vec<BindParam>> {
	params
		.into_iter()
		.map(|param| match param.kind.as_str() {
			"null" => Ok(BindParam::Null),
			"int" => Ok(BindParam::Integer(param.int_value.unwrap_or_default())),
			"float" => Ok(BindParam::Float(param.float_value.unwrap_or_default())),
			"text" => Ok(BindParam::Text(param.text_value.unwrap_or_default())),
			"blob" => Ok(BindParam::Blob(
				param
					.blob_value
					.map(|value| value.as_ref().to_vec())
					.unwrap_or_default(),
			)),
			other => Err(napi_anyhow_error(
				NapiInvalidArgument {
					argument: "kind".to_owned(),
					reason: format!("unsupported bind param kind `{other}`"),
				}
				.build(),
			)),
		})
		.collect()
}

fn js_batch_statements_to_core(
	statements: Vec<JsSqliteBatchStatement>,
) -> napi::Result<Vec<CoreSqliteBatchStatement>> {
	statements
		.into_iter()
		.map(|statement| {
			let params = statement.params.map(js_bind_params_to_core).transpose()?;
			Ok(CoreSqliteBatchStatement {
				sql: statement.sql,
				params,
			})
		})
		.collect()
}

fn core_query_result_to_js(result: CoreQueryResult) -> QueryResult {
	QueryResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(column_value_to_json).collect())
			.collect(),
	}
}

fn core_execute_result_to_js(result: CoreExecuteResult) -> NativeExecuteResult {
	NativeExecuteResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(column_value_to_json).collect())
			.collect(),
		changes: result.changes,
		last_insert_row_id: result.last_insert_row_id,
	}
}

fn column_value_to_json(value: ColumnValue) -> serde_json::Value {
	match value {
		ColumnValue::Null => serde_json::Value::Null,
		ColumnValue::Integer(value) => serde_json::Value::from(value),
		ColumnValue::Float(value) => serde_json::Value::from(value),
		ColumnValue::Text(value) => serde_json::Value::String(value),
		ColumnValue::Blob(value) => {
			serde_json::Value::Array(value.into_iter().map(serde_json::Value::from).collect())
		}
	}
}
