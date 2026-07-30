// from: v8.bare, to: v7.bare

#![allow(dead_code, unused_variables)]

use anyhow::Result;

use crate::generated::{v7, v8};

pub fn convert_kv_metadata_v8_to_v7(x: v8::KvMetadata) -> Result<v7::KvMetadata> {
	Ok(v7::KvMetadata {
		version: x.version,
		update_ts: x.update_ts,
	})
}

pub fn convert_kv_list_range_query_v8_to_v7(
	x: v8::KvListRangeQuery,
) -> Result<v7::KvListRangeQuery> {
	Ok(v7::KvListRangeQuery {
		start: x.start,
		end: x.end,
		exclusive: x.exclusive,
	})
}

pub fn convert_kv_list_prefix_query_v8_to_v7(
	x: v8::KvListPrefixQuery,
) -> Result<v7::KvListPrefixQuery> {
	Ok(v7::KvListPrefixQuery { key: x.key })
}

pub fn convert_kv_list_query_v8_to_v7(x: v8::KvListQuery) -> Result<v7::KvListQuery> {
	Ok(match x {
		v8::KvListQuery::KvListAllQuery => v7::KvListQuery::KvListAllQuery,
		v8::KvListQuery::KvListRangeQuery(v) => {
			v7::KvListQuery::KvListRangeQuery(convert_kv_list_range_query_v8_to_v7(v)?)
		}
		v8::KvListQuery::KvListPrefixQuery(v) => {
			v7::KvListQuery::KvListPrefixQuery(convert_kv_list_prefix_query_v8_to_v7(v)?)
		}
	})
}

pub fn convert_kv_get_request_v8_to_v7(x: v8::KvGetRequest) -> Result<v7::KvGetRequest> {
	Ok(v7::KvGetRequest { keys: x.keys })
}

pub fn convert_kv_list_request_v8_to_v7(x: v8::KvListRequest) -> Result<v7::KvListRequest> {
	Ok(v7::KvListRequest {
		query: convert_kv_list_query_v8_to_v7(x.query)?,
		reverse: x.reverse,
		limit: x.limit,
	})
}

pub fn convert_kv_put_request_v8_to_v7(x: v8::KvPutRequest) -> Result<v7::KvPutRequest> {
	Ok(v7::KvPutRequest {
		keys: x.keys,
		values: x.values,
	})
}

pub fn convert_kv_delete_request_v8_to_v7(x: v8::KvDeleteRequest) -> Result<v7::KvDeleteRequest> {
	Ok(v7::KvDeleteRequest { keys: x.keys })
}

pub fn convert_kv_delete_range_request_v8_to_v7(
	x: v8::KvDeleteRangeRequest,
) -> Result<v7::KvDeleteRangeRequest> {
	Ok(v7::KvDeleteRangeRequest {
		start: x.start,
		end: x.end,
	})
}

pub fn convert_kv_error_response_v8_to_v7(x: v8::KvErrorResponse) -> Result<v7::KvErrorResponse> {
	Ok(v7::KvErrorResponse { message: x.message })
}

pub fn convert_kv_get_response_v8_to_v7(x: v8::KvGetResponse) -> Result<v7::KvGetResponse> {
	Ok(v7::KvGetResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_list_response_v8_to_v7(x: v8::KvListResponse) -> Result<v7::KvListResponse> {
	Ok(v7::KvListResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_request_data_v8_to_v7(x: v8::KvRequestData) -> Result<v7::KvRequestData> {
	Ok(match x {
		v8::KvRequestData::KvGetRequest(v) => {
			v7::KvRequestData::KvGetRequest(convert_kv_get_request_v8_to_v7(v)?)
		}
		v8::KvRequestData::KvListRequest(v) => {
			v7::KvRequestData::KvListRequest(convert_kv_list_request_v8_to_v7(v)?)
		}
		v8::KvRequestData::KvPutRequest(v) => {
			v7::KvRequestData::KvPutRequest(convert_kv_put_request_v8_to_v7(v)?)
		}
		v8::KvRequestData::KvDeleteRequest(v) => {
			v7::KvRequestData::KvDeleteRequest(convert_kv_delete_request_v8_to_v7(v)?)
		}
		v8::KvRequestData::KvDeleteRangeRequest(v) => {
			v7::KvRequestData::KvDeleteRangeRequest(convert_kv_delete_range_request_v8_to_v7(v)?)
		}
		v8::KvRequestData::KvDropRequest => v7::KvRequestData::KvDropRequest,
	})
}

pub fn convert_kv_response_data_v8_to_v7(x: v8::KvResponseData) -> Result<v7::KvResponseData> {
	Ok(match x {
		v8::KvResponseData::KvErrorResponse(v) => {
			v7::KvResponseData::KvErrorResponse(convert_kv_error_response_v8_to_v7(v)?)
		}
		v8::KvResponseData::KvGetResponse(v) => {
			v7::KvResponseData::KvGetResponse(convert_kv_get_response_v8_to_v7(v)?)
		}
		v8::KvResponseData::KvListResponse(v) => {
			v7::KvResponseData::KvListResponse(convert_kv_list_response_v8_to_v7(v)?)
		}
		v8::KvResponseData::KvPutResponse => v7::KvResponseData::KvPutResponse,
		v8::KvResponseData::KvDeleteResponse => v7::KvResponseData::KvDeleteResponse,
		v8::KvResponseData::KvDropResponse => v7::KvResponseData::KvDropResponse,
	})
}

pub fn convert_sqlite_dirty_page_v8_to_v7(x: v8::SqliteDirtyPage) -> Result<v7::SqliteDirtyPage> {
	Ok(v7::SqliteDirtyPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_fetched_page_v8_to_v7(
	x: v8::SqliteFetchedPage,
) -> Result<v7::SqliteFetchedPage> {
	Ok(v7::SqliteFetchedPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_get_pages_request_v8_to_v7(
	x: v8::SqliteGetPagesRequest,
) -> Result<v7::SqliteGetPagesRequest> {
	Ok(v7::SqliteGetPagesRequest {
		actor_id: x.actor_id,
		pgnos: x.pgnos,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_get_pages_ok_v8_to_v7(
	x: v8::SqliteGetPagesOk,
) -> Result<v7::SqliteGetPagesOk> {
	Ok(v7::SqliteGetPagesOk {
		pages: x
			.pages
			.into_iter()
			.map(|v| convert_sqlite_fetched_page_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_error_response_v8_to_v7(
	x: v8::SqliteErrorResponse,
) -> Result<v7::SqliteErrorResponse> {
	Ok(v7::SqliteErrorResponse {
		group: x.group,
		code: x.code,
		message: x.message,
	})
}

pub fn convert_sqlite_get_pages_response_v8_to_v7(
	x: v8::SqliteGetPagesResponse,
) -> Result<v7::SqliteGetPagesResponse> {
	Ok(match x {
		v8::SqliteGetPagesResponse::SqliteGetPagesOk(v) => {
			v7::SqliteGetPagesResponse::SqliteGetPagesOk(convert_sqlite_get_pages_ok_v8_to_v7(v)?)
		}
		v8::SqliteGetPagesResponse::SqliteErrorResponse(v) => {
			v7::SqliteGetPagesResponse::SqliteErrorResponse(convert_sqlite_error_response_v8_to_v7(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_commit_request_v8_to_v7(
	x: v8::SqliteCommitRequest,
) -> Result<v7::SqliteCommitRequest> {
	Ok(v7::SqliteCommitRequest {
		actor_id: x.actor_id,
		dirty_pages: x
			.dirty_pages
			.into_iter()
			.map(|v| convert_sqlite_dirty_page_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
		db_size_pages: x.db_size_pages,
		now_ms: x.now_ms,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_commit_ok_v8_to_v7(x: v8::SqliteCommitOk) -> Result<v7::SqliteCommitOk> {
	Ok(v7::SqliteCommitOk {
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_commit_response_v8_to_v7(
	x: v8::SqliteCommitResponse,
) -> Result<v7::SqliteCommitResponse> {
	Ok(match x {
		v8::SqliteCommitResponse::SqliteCommitOk(v) => {
			v7::SqliteCommitResponse::SqliteCommitOk(convert_sqlite_commit_ok_v8_to_v7(v)?)
		}
		v8::SqliteCommitResponse::SqliteErrorResponse(v) => {
			v7::SqliteCommitResponse::SqliteErrorResponse(convert_sqlite_error_response_v8_to_v7(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_value_integer_v8_to_v7(
	x: v8::SqliteValueInteger,
) -> Result<v7::SqliteValueInteger> {
	Ok(v7::SqliteValueInteger { value: x.value })
}

pub fn convert_sqlite_value_float_v8_to_v7(
	x: v8::SqliteValueFloat,
) -> Result<v7::SqliteValueFloat> {
	Ok(v7::SqliteValueFloat { value: x.value })
}

pub fn convert_sqlite_value_text_v8_to_v7(x: v8::SqliteValueText) -> Result<v7::SqliteValueText> {
	Ok(v7::SqliteValueText { value: x.value })
}

pub fn convert_sqlite_value_blob_v8_to_v7(x: v8::SqliteValueBlob) -> Result<v7::SqliteValueBlob> {
	Ok(v7::SqliteValueBlob { value: x.value })
}

pub fn convert_sqlite_bind_param_v8_to_v7(x: v8::SqliteBindParam) -> Result<v7::SqliteBindParam> {
	Ok(match x {
		v8::SqliteBindParam::SqliteValueNull => v7::SqliteBindParam::SqliteValueNull,
		v8::SqliteBindParam::SqliteValueInteger(v) => {
			v7::SqliteBindParam::SqliteValueInteger(convert_sqlite_value_integer_v8_to_v7(v)?)
		}
		v8::SqliteBindParam::SqliteValueFloat(v) => {
			v7::SqliteBindParam::SqliteValueFloat(convert_sqlite_value_float_v8_to_v7(v)?)
		}
		v8::SqliteBindParam::SqliteValueText(v) => {
			v7::SqliteBindParam::SqliteValueText(convert_sqlite_value_text_v8_to_v7(v)?)
		}
		v8::SqliteBindParam::SqliteValueBlob(v) => {
			v7::SqliteBindParam::SqliteValueBlob(convert_sqlite_value_blob_v8_to_v7(v)?)
		}
	})
}

pub fn convert_sqlite_column_value_v8_to_v7(
	x: v8::SqliteColumnValue,
) -> Result<v7::SqliteColumnValue> {
	Ok(match x {
		v8::SqliteColumnValue::SqliteValueNull => v7::SqliteColumnValue::SqliteValueNull,
		v8::SqliteColumnValue::SqliteValueInteger(v) => {
			v7::SqliteColumnValue::SqliteValueInteger(convert_sqlite_value_integer_v8_to_v7(v)?)
		}
		v8::SqliteColumnValue::SqliteValueFloat(v) => {
			v7::SqliteColumnValue::SqliteValueFloat(convert_sqlite_value_float_v8_to_v7(v)?)
		}
		v8::SqliteColumnValue::SqliteValueText(v) => {
			v7::SqliteColumnValue::SqliteValueText(convert_sqlite_value_text_v8_to_v7(v)?)
		}
		v8::SqliteColumnValue::SqliteValueBlob(v) => {
			v7::SqliteColumnValue::SqliteValueBlob(convert_sqlite_value_blob_v8_to_v7(v)?)
		}
	})
}

pub fn convert_sqlite_query_result_v8_to_v7(
	x: v8::SqliteQueryResult,
) -> Result<v7::SqliteQueryResult> {
	Ok(v7::SqliteQueryResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v8_to_v7(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_execute_result_v8_to_v7(
	x: v8::SqliteExecuteResult,
) -> Result<v7::SqliteExecuteResult> {
	Ok(v7::SqliteExecuteResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v8_to_v7(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
		changes: x.changes,
		last_insert_row_id: x.last_insert_row_id,
	})
}

pub fn convert_sqlite_exec_request_v8_to_v7(
	x: v8::SqliteExecRequest,
) -> Result<v7::SqliteExecRequest> {
	Ok(v7::SqliteExecRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
	})
}

pub fn convert_sqlite_execute_request_v8_to_v7(
	x: v8::SqliteExecuteRequest,
) -> Result<v7::SqliteExecuteRequest> {
	Ok(v7::SqliteExecuteRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v8_to_v7(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_batch_statement_v8_to_v7(
	x: v8::SqliteBatchStatement,
) -> Result<v7::SqliteBatchStatement> {
	Ok(v7::SqliteBatchStatement {
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v8_to_v7(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_execute_batch_request_v8_to_v7(
	x: v8::SqliteExecuteBatchRequest,
) -> Result<v7::SqliteExecuteBatchRequest> {
	Ok(v7::SqliteExecuteBatchRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		statements: x
			.statements
			.into_iter()
			.map(|v| convert_sqlite_batch_statement_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_ok_v8_to_v7(x: v8::SqliteExecOk) -> Result<v7::SqliteExecOk> {
	Ok(v7::SqliteExecOk {
		result: convert_sqlite_query_result_v8_to_v7(x.result)?,
	})
}

pub fn convert_sqlite_execute_ok_v8_to_v7(x: v8::SqliteExecuteOk) -> Result<v7::SqliteExecuteOk> {
	Ok(v7::SqliteExecuteOk {
		result: convert_sqlite_execute_result_v8_to_v7(x.result)?,
	})
}

pub fn convert_sqlite_execute_batch_ok_v8_to_v7(
	x: v8::SqliteExecuteBatchOk,
) -> Result<v7::SqliteExecuteBatchOk> {
	Ok(v7::SqliteExecuteBatchOk {
		results: x
			.results
			.into_iter()
			.map(|v| convert_sqlite_execute_result_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_response_v8_to_v7(
	x: v8::SqliteExecResponse,
) -> Result<v7::SqliteExecResponse> {
	Ok(match x {
		v8::SqliteExecResponse::SqliteExecOk(v) => {
			v7::SqliteExecResponse::SqliteExecOk(convert_sqlite_exec_ok_v8_to_v7(v)?)
		}
		v8::SqliteExecResponse::SqliteErrorResponse(v) => {
			v7::SqliteExecResponse::SqliteErrorResponse(convert_sqlite_error_response_v8_to_v7(v)?)
		}
	})
}

pub fn convert_sqlite_execute_response_v8_to_v7(
	x: v8::SqliteExecuteResponse,
) -> Result<v7::SqliteExecuteResponse> {
	Ok(match x {
		v8::SqliteExecuteResponse::SqliteExecuteOk(v) => {
			v7::SqliteExecuteResponse::SqliteExecuteOk(convert_sqlite_execute_ok_v8_to_v7(v)?)
		}
		v8::SqliteExecuteResponse::SqliteErrorResponse(v) => {
			v7::SqliteExecuteResponse::SqliteErrorResponse(convert_sqlite_error_response_v8_to_v7(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_execute_batch_response_v8_to_v7(
	x: v8::SqliteExecuteBatchResponse,
) -> Result<v7::SqliteExecuteBatchResponse> {
	Ok(match x {
		v8::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(v) => {
			v7::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(
				convert_sqlite_execute_batch_ok_v8_to_v7(v)?,
			)
		}
		v8::SqliteExecuteBatchResponse::SqliteErrorResponse(v) => {
			v7::SqliteExecuteBatchResponse::SqliteErrorResponse(
				convert_sqlite_error_response_v8_to_v7(v)?,
			)
		}
	})
}

pub fn convert_stop_code_v8_to_v7(x: v8::StopCode) -> Result<v7::StopCode> {
	Ok(match x {
		v8::StopCode::Ok => v7::StopCode::Ok,
		v8::StopCode::Error => v7::StopCode::Error,
	})
}

pub fn convert_actor_name_v8_to_v7(x: v8::ActorName) -> Result<v7::ActorName> {
	Ok(v7::ActorName {
		metadata: x.metadata,
	})
}

pub fn convert_actor_config_v8_to_v7(x: v8::ActorConfig) -> Result<v7::ActorConfig> {
	Ok(v7::ActorConfig {
		name: x.name,
		key: x.key,
		create_ts: x.create_ts,
		input: x.input,
	})
}

pub fn convert_actor_checkpoint_v8_to_v7(x: v8::ActorCheckpoint) -> Result<v7::ActorCheckpoint> {
	Ok(v7::ActorCheckpoint {
		actor_id: x.actor_id,
		generation: x.generation,
		index: x.index,
	})
}

pub fn convert_actor_intent_v8_to_v7(x: v8::ActorIntent) -> Result<v7::ActorIntent> {
	Ok(match x {
		v8::ActorIntent::ActorIntentSleep => v7::ActorIntent::ActorIntentSleep,
		v8::ActorIntent::ActorIntentStop => v7::ActorIntent::ActorIntentStop,
	})
}

pub fn convert_actor_state_stopped_v8_to_v7(
	x: v8::ActorStateStopped,
) -> Result<v7::ActorStateStopped> {
	Ok(v7::ActorStateStopped {
		code: convert_stop_code_v8_to_v7(x.code)?,
		message: x.message,
	})
}

pub fn convert_actor_state_v8_to_v7(x: v8::ActorState) -> Result<v7::ActorState> {
	Ok(match x {
		v8::ActorState::ActorStateRunning => v7::ActorState::ActorStateRunning,
		v8::ActorState::ActorStateStopped(v) => {
			v7::ActorState::ActorStateStopped(convert_actor_state_stopped_v8_to_v7(v)?)
		}
	})
}

pub fn convert_event_actor_intent_v8_to_v7(
	x: v8::EventActorIntent,
) -> Result<v7::EventActorIntent> {
	Ok(v7::EventActorIntent {
		intent: convert_actor_intent_v8_to_v7(x.intent)?,
	})
}

pub fn convert_event_actor_state_update_v8_to_v7(
	x: v8::EventActorStateUpdate,
) -> Result<v7::EventActorStateUpdate> {
	Ok(v7::EventActorStateUpdate {
		state: convert_actor_state_v8_to_v7(x.state)?,
	})
}

pub fn convert_event_actor_set_alarm_v8_to_v7(
	x: v8::EventActorSetAlarm,
) -> Result<v7::EventActorSetAlarm> {
	Ok(v7::EventActorSetAlarm {
		alarm_ts: x.alarm_ts,
	})
}

pub fn convert_event_v8_to_v7(x: v8::Event) -> Result<v7::Event> {
	Ok(match x {
		v8::Event::EventActorIntent(v) => {
			v7::Event::EventActorIntent(convert_event_actor_intent_v8_to_v7(v)?)
		}
		v8::Event::EventActorStateUpdate(v) => {
			v7::Event::EventActorStateUpdate(convert_event_actor_state_update_v8_to_v7(v)?)
		}
		v8::Event::EventActorSetAlarm(v) => {
			v7::Event::EventActorSetAlarm(convert_event_actor_set_alarm_v8_to_v7(v)?)
		}
	})
}

pub fn convert_event_wrapper_v8_to_v7(x: v8::EventWrapper) -> Result<v7::EventWrapper> {
	Ok(v7::EventWrapper {
		checkpoint: convert_actor_checkpoint_v8_to_v7(x.checkpoint)?,
		inner: convert_event_v8_to_v7(x.inner)?,
	})
}

pub fn convert_preloaded_kv_entry_v8_to_v7(
	x: v8::PreloadedKvEntry,
) -> Result<v7::PreloadedKvEntry> {
	Ok(v7::PreloadedKvEntry {
		key: x.key,
		value: x.value,
		metadata: convert_kv_metadata_v8_to_v7(x.metadata)?,
	})
}

pub fn convert_preloaded_kv_v8_to_v7(x: v8::PreloadedKv) -> Result<v7::PreloadedKv> {
	Ok(v7::PreloadedKv {
		entries: x
			.entries
			.into_iter()
			.map(|v| convert_preloaded_kv_entry_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
		requested_get_keys: x.requested_get_keys,
		requested_prefixes: x.requested_prefixes,
	})
}

pub fn convert_hibernating_request_v8_to_v7(
	x: v8::HibernatingRequest,
) -> Result<v7::HibernatingRequest> {
	Ok(v7::HibernatingRequest {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
	})
}

pub fn convert_command_start_actor_v8_to_v7(
	x: v8::CommandStartActor,
) -> Result<v7::CommandStartActor> {
	Ok(v7::CommandStartActor {
		config: convert_actor_config_v8_to_v7(x.config)?,
		hibernating_requests: x
			.hibernating_requests
			.into_iter()
			.map(|v| convert_hibernating_request_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
		preloaded_kv: x
			.preloaded_kv
			.map(|v| convert_preloaded_kv_v8_to_v7(v))
			.transpose()?,
	})
}

pub fn convert_stop_actor_reason_v8_to_v7(x: v8::StopActorReason) -> Result<v7::StopActorReason> {
	Ok(match x {
		v8::StopActorReason::SleepIntent => v7::StopActorReason::SleepIntent,
		v8::StopActorReason::StopIntent => v7::StopActorReason::StopIntent,
		v8::StopActorReason::Destroy => v7::StopActorReason::Destroy,
		v8::StopActorReason::GoingAway => v7::StopActorReason::GoingAway,
		v8::StopActorReason::Lost => v7::StopActorReason::Lost,
	})
}

pub fn convert_command_stop_actor_v8_to_v7(
	x: v8::CommandStopActor,
) -> Result<v7::CommandStopActor> {
	Ok(v7::CommandStopActor {
		reason: convert_stop_actor_reason_v8_to_v7(x.reason)?,
	})
}

pub fn convert_command_v8_to_v7(x: v8::Command) -> Result<v7::Command> {
	Ok(match x {
		v8::Command::CommandStartActor(v) => {
			v7::Command::CommandStartActor(convert_command_start_actor_v8_to_v7(v)?)
		}
		v8::Command::CommandStopActor(v) => {
			v7::Command::CommandStopActor(convert_command_stop_actor_v8_to_v7(v)?)
		}
	})
}

pub fn convert_command_wrapper_v8_to_v7(x: v8::CommandWrapper) -> Result<v7::CommandWrapper> {
	Ok(v7::CommandWrapper {
		checkpoint: convert_actor_checkpoint_v8_to_v7(x.checkpoint)?,
		inner: convert_command_v8_to_v7(x.inner)?,
	})
}

pub fn convert_actor_command_key_data_v8_to_v7(
	x: v8::ActorCommandKeyData,
) -> Result<v7::ActorCommandKeyData> {
	Ok(match x {
		v8::ActorCommandKeyData::CommandStartActor(v) => {
			v7::ActorCommandKeyData::CommandStartActor(convert_command_start_actor_v8_to_v7(v)?)
		}
		v8::ActorCommandKeyData::CommandStopActor(v) => {
			v7::ActorCommandKeyData::CommandStopActor(convert_command_stop_actor_v8_to_v7(v)?)
		}
	})
}

pub fn convert_message_id_v8_to_v7(x: v8::MessageId) -> Result<v7::MessageId> {
	Ok(v7::MessageId {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		message_index: x.message_index,
	})
}

pub fn convert_to_envoy_request_start_v8_to_v7(
	x: v8::ToEnvoyRequestStart,
) -> Result<v7::ToEnvoyRequestStart> {
	Ok(v7::ToEnvoyRequestStart {
		actor_id: x.actor_id,
		method: x.method,
		path: x.path,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_envoy_request_chunk_v8_to_v7(
	x: v8::ToEnvoyRequestChunk,
) -> Result<v7::ToEnvoyRequestChunk> {
	Ok(v7::ToEnvoyRequestChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_http_stream_abort_reason_kind_v8_to_v7(
	x: v8::HttpStreamAbortReasonKind,
) -> Result<v7::HttpStreamAbortReasonKind> {
	Ok(match x {
		v8::HttpStreamAbortReasonKind::Unknown => v7::HttpStreamAbortReasonKind::Unknown,
		v8::HttpStreamAbortReasonKind::ClientDisconnect => {
			v7::HttpStreamAbortReasonKind::ClientDisconnect
		}
		v8::HttpStreamAbortReasonKind::HandlerError => v7::HttpStreamAbortReasonKind::HandlerError,
		v8::HttpStreamAbortReasonKind::IdleTimeout => v7::HttpStreamAbortReasonKind::IdleTimeout,
		v8::HttpStreamAbortReasonKind::Overloaded => v7::HttpStreamAbortReasonKind::Overloaded,
		v8::HttpStreamAbortReasonKind::BodyTooLarge => v7::HttpStreamAbortReasonKind::BodyTooLarge,
		v8::HttpStreamAbortReasonKind::OutOfMemory => v7::HttpStreamAbortReasonKind::OutOfMemory,
		v8::HttpStreamAbortReasonKind::Shutdown => v7::HttpStreamAbortReasonKind::Shutdown,
		v8::HttpStreamAbortReasonKind::InternalError => {
			v7::HttpStreamAbortReasonKind::InternalError
		}
	})
}

pub fn convert_http_stream_abort_reason_v8_to_v7(
	x: v8::HttpStreamAbortReason,
) -> Result<v7::HttpStreamAbortReason> {
	Ok(v7::HttpStreamAbortReason {
		kind: convert_http_stream_abort_reason_kind_v8_to_v7(x.kind)?,
		detail: x.detail,
	})
}

pub fn convert_to_envoy_request_abort_v8_to_v7(
	x: v8::ToEnvoyRequestAbort,
) -> Result<v7::ToEnvoyRequestAbort> {
	Ok(v7::ToEnvoyRequestAbort {
		reason: convert_http_stream_abort_reason_v8_to_v7(x.reason)?,
	})
}

pub fn convert_to_rivet_response_abort_v8_to_v7(
	x: v8::ToRivetResponseAbort,
) -> Result<v7::ToRivetResponseAbort> {
	Ok(v7::ToRivetResponseAbort {
		reason: convert_http_stream_abort_reason_v8_to_v7(x.reason)?,
	})
}

pub fn convert_to_rivet_response_start_v8_to_v7(
	x: v8::ToRivetResponseStart,
) -> Result<v7::ToRivetResponseStart> {
	Ok(v7::ToRivetResponseStart {
		status: x.status,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_rivet_response_chunk_v8_to_v7(
	x: v8::ToRivetResponseChunk,
) -> Result<v7::ToRivetResponseChunk> {
	Ok(v7::ToRivetResponseChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_to_envoy_web_socket_open_v8_to_v7(
	x: v8::ToEnvoyWebSocketOpen,
) -> Result<v7::ToEnvoyWebSocketOpen> {
	Ok(v7::ToEnvoyWebSocketOpen {
		actor_id: x.actor_id,
		path: x.path,
		headers: x.headers,
	})
}

pub fn convert_to_envoy_web_socket_message_v8_to_v7(
	x: v8::ToEnvoyWebSocketMessage,
) -> Result<v7::ToEnvoyWebSocketMessage> {
	Ok(v7::ToEnvoyWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_envoy_web_socket_close_v8_to_v7(
	x: v8::ToEnvoyWebSocketClose,
) -> Result<v7::ToEnvoyWebSocketClose> {
	Ok(v7::ToEnvoyWebSocketClose {
		code: x.code,
		reason: x.reason,
	})
}

pub fn convert_to_rivet_web_socket_open_v8_to_v7(
	x: v8::ToRivetWebSocketOpen,
) -> Result<v7::ToRivetWebSocketOpen> {
	Ok(v7::ToRivetWebSocketOpen {
		can_hibernate: x.can_hibernate,
	})
}

pub fn convert_to_rivet_web_socket_message_v8_to_v7(
	x: v8::ToRivetWebSocketMessage,
) -> Result<v7::ToRivetWebSocketMessage> {
	Ok(v7::ToRivetWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_rivet_web_socket_message_ack_v8_to_v7(
	x: v8::ToRivetWebSocketMessageAck,
) -> Result<v7::ToRivetWebSocketMessageAck> {
	Ok(v7::ToRivetWebSocketMessageAck { index: x.index })
}

pub fn convert_to_rivet_web_socket_close_v8_to_v7(
	x: v8::ToRivetWebSocketClose,
) -> Result<v7::ToRivetWebSocketClose> {
	Ok(v7::ToRivetWebSocketClose {
		code: x.code,
		reason: x.reason,
		hibernate: x.hibernate,
	})
}

pub fn convert_to_rivet_tunnel_message_kind_v8_to_v7(
	x: v8::ToRivetTunnelMessageKind,
) -> Result<v7::ToRivetTunnelMessageKind> {
	Ok(match x {
		v8::ToRivetTunnelMessageKind::ToRivetResponseStart(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetResponseStart(
				convert_to_rivet_response_start_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetResponseChunk(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				convert_to_rivet_response_chunk_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetResponseAbort(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				convert_to_rivet_response_abort_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(
				convert_to_rivet_web_socket_open_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(
				convert_to_rivet_web_socket_message_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(
				convert_to_rivet_web_socket_message_ack_v8_to_v7(v)?,
			)
		}
		v8::ToRivetTunnelMessageKind::ToRivetWebSocketClose(v) => {
			v7::ToRivetTunnelMessageKind::ToRivetWebSocketClose(
				convert_to_rivet_web_socket_close_v8_to_v7(v)?,
			)
		}
	})
}

pub fn convert_to_rivet_tunnel_message_v8_to_v7(
	x: v8::ToRivetTunnelMessage,
) -> Result<v7::ToRivetTunnelMessage> {
	Ok(v7::ToRivetTunnelMessage {
		message_id: convert_message_id_v8_to_v7(x.message_id)?,
		message_kind: convert_to_rivet_tunnel_message_kind_v8_to_v7(x.message_kind)?,
	})
}

pub fn convert_to_envoy_tunnel_message_kind_v8_to_v7(
	x: v8::ToEnvoyTunnelMessageKind,
) -> Result<v7::ToEnvoyTunnelMessageKind> {
	Ok(match x {
		v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				convert_to_envoy_request_start_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				convert_to_envoy_request_chunk_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				convert_to_envoy_request_abort_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				convert_to_envoy_web_socket_open_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				convert_to_envoy_web_socket_message_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(v) => {
			v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
				convert_to_envoy_web_socket_close_v8_to_v7(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_tunnel_message_v8_to_v7(
	x: v8::ToEnvoyTunnelMessage,
) -> Result<v7::ToEnvoyTunnelMessage> {
	Ok(v7::ToEnvoyTunnelMessage {
		message_id: convert_message_id_v8_to_v7(x.message_id)?,
		message_kind: convert_to_envoy_tunnel_message_kind_v8_to_v7(x.message_kind)?,
	})
}

pub fn convert_to_envoy_ping_v8_to_v7(x: v8::ToEnvoyPing) -> Result<v7::ToEnvoyPing> {
	Ok(v7::ToEnvoyPing { ts: x.ts })
}

pub fn convert_to_rivet_metadata_v8_to_v7(x: v8::ToRivetMetadata) -> Result<v7::ToRivetMetadata> {
	Ok(v7::ToRivetMetadata {
		prepopulate_actor_names: x
			.prepopulate_actor_names
			.map(|v| {
				v.into_iter()
					.map(|(k, v)| -> Result<_> { Ok((k, convert_actor_name_v8_to_v7(v)?)) })
					.collect::<Result<_>>()
			})
			.transpose()?,
		metadata: x.metadata,
	})
}

pub fn convert_to_rivet_events_v8_to_v7(x: v8::ToRivetEvents) -> Result<v7::ToRivetEvents> {
	Ok(x.into_iter()
		.map(|v| convert_event_wrapper_v8_to_v7(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_rivet_ack_commands_v8_to_v7(
	x: v8::ToRivetAckCommands,
) -> Result<v7::ToRivetAckCommands> {
	Ok(v7::ToRivetAckCommands {
		last_command_checkpoints: x
			.last_command_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_rivet_pong_v8_to_v7(x: v8::ToRivetPong) -> Result<v7::ToRivetPong> {
	Ok(v7::ToRivetPong { ts: x.ts })
}

pub fn convert_to_rivet_kv_request_v8_to_v7(
	x: v8::ToRivetKvRequest,
) -> Result<v7::ToRivetKvRequest> {
	Ok(v7::ToRivetKvRequest {
		actor_id: x.actor_id,
		request_id: x.request_id,
		data: convert_kv_request_data_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_get_pages_request_v8_to_v7(
	x: v8::ToRivetSqliteGetPagesRequest,
) -> Result<v7::ToRivetSqliteGetPagesRequest> {
	Ok(v7::ToRivetSqliteGetPagesRequest {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_request_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_commit_request_v8_to_v7(
	x: v8::ToRivetSqliteCommitRequest,
) -> Result<v7::ToRivetSqliteCommitRequest> {
	Ok(v7::ToRivetSqliteCommitRequest {
		request_id: x.request_id,
		data: convert_sqlite_commit_request_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_exec_request_v8_to_v7(
	x: v8::ToRivetSqliteExecRequest,
) -> Result<v7::ToRivetSqliteExecRequest> {
	Ok(v7::ToRivetSqliteExecRequest {
		request_id: x.request_id,
		data: convert_sqlite_exec_request_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_request_v8_to_v7(
	x: v8::ToRivetSqliteExecuteRequest,
) -> Result<v7::ToRivetSqliteExecuteRequest> {
	Ok(v7::ToRivetSqliteExecuteRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_request_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_batch_request_v8_to_v7(
	x: v8::ToRivetSqliteExecuteBatchRequest,
) -> Result<v7::ToRivetSqliteExecuteBatchRequest> {
	Ok(v7::ToRivetSqliteExecuteBatchRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_request_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_rivet_v8_to_v7(x: v8::ToRivet) -> Result<v7::ToRivet> {
	Ok(match x {
		v8::ToRivet::ToRivetMetadata(v) => {
			v7::ToRivet::ToRivetMetadata(convert_to_rivet_metadata_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetEvents(v) => {
			v7::ToRivet::ToRivetEvents(convert_to_rivet_events_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetAckCommands(v) => {
			v7::ToRivet::ToRivetAckCommands(convert_to_rivet_ack_commands_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetStopping => v7::ToRivet::ToRivetStopping,
		v8::ToRivet::ToRivetPong(v) => v7::ToRivet::ToRivetPong(convert_to_rivet_pong_v8_to_v7(v)?),
		v8::ToRivet::ToRivetKvRequest(v) => {
			v7::ToRivet::ToRivetKvRequest(convert_to_rivet_kv_request_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetTunnelMessage(v) => {
			v7::ToRivet::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetSqliteGetPagesRequest(v) => v7::ToRivet::ToRivetSqliteGetPagesRequest(
			convert_to_rivet_sqlite_get_pages_request_v8_to_v7(v)?,
		),
		v8::ToRivet::ToRivetSqliteCommitRequest(v) => v7::ToRivet::ToRivetSqliteCommitRequest(
			convert_to_rivet_sqlite_commit_request_v8_to_v7(v)?,
		),
		v8::ToRivet::ToRivetSqliteExecRequest(v) => {
			v7::ToRivet::ToRivetSqliteExecRequest(convert_to_rivet_sqlite_exec_request_v8_to_v7(v)?)
		}
		v8::ToRivet::ToRivetSqliteExecuteRequest(v) => v7::ToRivet::ToRivetSqliteExecuteRequest(
			convert_to_rivet_sqlite_execute_request_v8_to_v7(v)?,
		),
		v8::ToRivet::ToRivetSqliteExecuteBatchRequest(v) => {
			v7::ToRivet::ToRivetSqliteExecuteBatchRequest(
				convert_to_rivet_sqlite_execute_batch_request_v8_to_v7(v)?,
			)
		}
	})
}

pub fn convert_protocol_metadata_v8_to_v7(x: v8::ProtocolMetadata) -> Result<v7::ProtocolMetadata> {
	Ok(v7::ProtocolMetadata {
		envoy_lost_threshold: x.envoy_lost_threshold,
		actor_stop_threshold: x.actor_stop_threshold,
		max_response_payload_size: x.max_response_payload_size,
	})
}

pub fn convert_to_envoy_init_v8_to_v7(x: v8::ToEnvoyInit) -> Result<v7::ToEnvoyInit> {
	Ok(v7::ToEnvoyInit {
		metadata: convert_protocol_metadata_v8_to_v7(x.metadata)?,
	})
}

pub fn convert_to_envoy_commands_v8_to_v7(x: v8::ToEnvoyCommands) -> Result<v7::ToEnvoyCommands> {
	Ok(x.into_iter()
		.map(|v| convert_command_wrapper_v8_to_v7(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_envoy_ack_events_v8_to_v7(
	x: v8::ToEnvoyAckEvents,
) -> Result<v7::ToEnvoyAckEvents> {
	Ok(v7::ToEnvoyAckEvents {
		last_event_checkpoints: x
			.last_event_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v8_to_v7(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_envoy_kv_response_v8_to_v7(
	x: v8::ToEnvoyKvResponse,
) -> Result<v7::ToEnvoyKvResponse> {
	Ok(v7::ToEnvoyKvResponse {
		request_id: x.request_id,
		data: convert_kv_response_data_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_get_pages_response_v8_to_v7(
	x: v8::ToEnvoySqliteGetPagesResponse,
) -> Result<v7::ToEnvoySqliteGetPagesResponse> {
	Ok(v7::ToEnvoySqliteGetPagesResponse {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_response_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_commit_response_v8_to_v7(
	x: v8::ToEnvoySqliteCommitResponse,
) -> Result<v7::ToEnvoySqliteCommitResponse> {
	Ok(v7::ToEnvoySqliteCommitResponse {
		request_id: x.request_id,
		data: convert_sqlite_commit_response_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_exec_response_v8_to_v7(
	x: v8::ToEnvoySqliteExecResponse,
) -> Result<v7::ToEnvoySqliteExecResponse> {
	Ok(v7::ToEnvoySqliteExecResponse {
		request_id: x.request_id,
		data: convert_sqlite_exec_response_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_response_v8_to_v7(
	x: v8::ToEnvoySqliteExecuteResponse,
) -> Result<v7::ToEnvoySqliteExecuteResponse> {
	Ok(v7::ToEnvoySqliteExecuteResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_response_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_batch_response_v8_to_v7(
	x: v8::ToEnvoySqliteExecuteBatchResponse,
) -> Result<v7::ToEnvoySqliteExecuteBatchResponse> {
	Ok(v7::ToEnvoySqliteExecuteBatchResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_response_v8_to_v7(x.data)?,
	})
}

pub fn convert_to_envoy_v8_to_v7(x: v8::ToEnvoy) -> Result<v7::ToEnvoy> {
	Ok(match x {
		v8::ToEnvoy::ToEnvoyInit(v) => v7::ToEnvoy::ToEnvoyInit(convert_to_envoy_init_v8_to_v7(v)?),
		v8::ToEnvoy::ToEnvoyCommands(v) => {
			v7::ToEnvoy::ToEnvoyCommands(convert_to_envoy_commands_v8_to_v7(v)?)
		}
		v8::ToEnvoy::ToEnvoyAckEvents(v) => {
			v7::ToEnvoy::ToEnvoyAckEvents(convert_to_envoy_ack_events_v8_to_v7(v)?)
		}
		v8::ToEnvoy::ToEnvoyKvResponse(v) => {
			v7::ToEnvoy::ToEnvoyKvResponse(convert_to_envoy_kv_response_v8_to_v7(v)?)
		}
		v8::ToEnvoy::ToEnvoyTunnelMessage(v) => {
			v7::ToEnvoy::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v8_to_v7(v)?)
		}
		v8::ToEnvoy::ToEnvoyPing(v) => v7::ToEnvoy::ToEnvoyPing(convert_to_envoy_ping_v8_to_v7(v)?),
		v8::ToEnvoy::ToEnvoySqliteGetPagesResponse(v) => {
			v7::ToEnvoy::ToEnvoySqliteGetPagesResponse(
				convert_to_envoy_sqlite_get_pages_response_v8_to_v7(v)?,
			)
		}
		v8::ToEnvoy::ToEnvoySqliteCommitResponse(v) => v7::ToEnvoy::ToEnvoySqliteCommitResponse(
			convert_to_envoy_sqlite_commit_response_v8_to_v7(v)?,
		),
		v8::ToEnvoy::ToEnvoySqliteExecResponse(v) => v7::ToEnvoy::ToEnvoySqliteExecResponse(
			convert_to_envoy_sqlite_exec_response_v8_to_v7(v)?,
		),
		v8::ToEnvoy::ToEnvoySqliteExecuteResponse(v) => v7::ToEnvoy::ToEnvoySqliteExecuteResponse(
			convert_to_envoy_sqlite_execute_response_v8_to_v7(v)?,
		),
		v8::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(v) => {
			v7::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(
				convert_to_envoy_sqlite_execute_batch_response_v8_to_v7(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_conn_ping_v8_to_v7(x: v8::ToEnvoyConnPing) -> Result<v7::ToEnvoyConnPing> {
	Ok(v7::ToEnvoyConnPing {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_envoy_conn_v8_to_v7(x: v8::ToEnvoyConn) -> Result<v7::ToEnvoyConn> {
	Ok(match x {
		v8::ToEnvoyConn::ToEnvoyConnPing(v) => {
			v7::ToEnvoyConn::ToEnvoyConnPing(convert_to_envoy_conn_ping_v8_to_v7(v)?)
		}
		v8::ToEnvoyConn::ToEnvoyConnClose => v7::ToEnvoyConn::ToEnvoyConnClose,
		v8::ToEnvoyConn::ToEnvoyCommands(v) => {
			v7::ToEnvoyConn::ToEnvoyCommands(convert_to_envoy_commands_v8_to_v7(v)?)
		}
		v8::ToEnvoyConn::ToEnvoyAckEvents(v) => {
			v7::ToEnvoyConn::ToEnvoyAckEvents(convert_to_envoy_ack_events_v8_to_v7(v)?)
		}
		v8::ToEnvoyConn::ToEnvoyTunnelMessage(v) => {
			v7::ToEnvoyConn::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v8_to_v7(v)?)
		}
	})
}

pub fn convert_to_gateway_pong_v8_to_v7(x: v8::ToGatewayPong) -> Result<v7::ToGatewayPong> {
	Ok(v7::ToGatewayPong {
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_gateway_v8_to_v7(x: v8::ToGateway) -> Result<v7::ToGateway> {
	Ok(match x {
		v8::ToGateway::ToGatewayPong(v) => {
			v7::ToGateway::ToGatewayPong(convert_to_gateway_pong_v8_to_v7(v)?)
		}
		v8::ToGateway::ToRivetTunnelMessage(v) => {
			v7::ToGateway::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v8_to_v7(v)?)
		}
	})
}

pub fn convert_to_outbound_actor_start_v8_to_v7(
	x: v8::ToOutboundActorStart,
) -> Result<v7::ToOutboundActorStart> {
	Ok(v7::ToOutboundActorStart {
		namespace_id: x.namespace_id,
		pool_name: x.pool_name,
		checkpoint: convert_actor_checkpoint_v8_to_v7(x.checkpoint)?,
		actor_config: convert_actor_config_v8_to_v7(x.actor_config)?,
	})
}

pub fn convert_to_outbound_v8_to_v7(x: v8::ToOutbound) -> Result<v7::ToOutbound> {
	Ok(match x {
		v8::ToOutbound::ToOutboundActorStart(v) => {
			v7::ToOutbound::ToOutboundActorStart(convert_to_outbound_actor_start_v8_to_v7(v)?)
		}
	})
}
