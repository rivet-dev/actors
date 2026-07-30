// from: v7.bare, to: v8.bare

#![allow(dead_code, unused_variables)]

use anyhow::Result;

use crate::generated::{v7, v8};

pub fn convert_kv_metadata_v7_to_v8(x: v7::KvMetadata) -> Result<v8::KvMetadata> {
	Ok(v8::KvMetadata {
		version: x.version,
		update_ts: x.update_ts,
	})
}

pub fn convert_kv_list_range_query_v7_to_v8(
	x: v7::KvListRangeQuery,
) -> Result<v8::KvListRangeQuery> {
	Ok(v8::KvListRangeQuery {
		start: x.start,
		end: x.end,
		exclusive: x.exclusive,
	})
}

pub fn convert_kv_list_prefix_query_v7_to_v8(
	x: v7::KvListPrefixQuery,
) -> Result<v8::KvListPrefixQuery> {
	Ok(v8::KvListPrefixQuery { key: x.key })
}

pub fn convert_kv_list_query_v7_to_v8(x: v7::KvListQuery) -> Result<v8::KvListQuery> {
	Ok(match x {
		v7::KvListQuery::KvListAllQuery => v8::KvListQuery::KvListAllQuery,
		v7::KvListQuery::KvListRangeQuery(v) => {
			v8::KvListQuery::KvListRangeQuery(convert_kv_list_range_query_v7_to_v8(v)?)
		}
		v7::KvListQuery::KvListPrefixQuery(v) => {
			v8::KvListQuery::KvListPrefixQuery(convert_kv_list_prefix_query_v7_to_v8(v)?)
		}
	})
}

pub fn convert_kv_get_request_v7_to_v8(x: v7::KvGetRequest) -> Result<v8::KvGetRequest> {
	Ok(v8::KvGetRequest { keys: x.keys })
}

pub fn convert_kv_list_request_v7_to_v8(x: v7::KvListRequest) -> Result<v8::KvListRequest> {
	Ok(v8::KvListRequest {
		query: convert_kv_list_query_v7_to_v8(x.query)?,
		reverse: x.reverse,
		limit: x.limit,
	})
}

pub fn convert_kv_put_request_v7_to_v8(x: v7::KvPutRequest) -> Result<v8::KvPutRequest> {
	Ok(v8::KvPutRequest {
		keys: x.keys,
		values: x.values,
	})
}

pub fn convert_kv_delete_request_v7_to_v8(x: v7::KvDeleteRequest) -> Result<v8::KvDeleteRequest> {
	Ok(v8::KvDeleteRequest { keys: x.keys })
}

pub fn convert_kv_delete_range_request_v7_to_v8(
	x: v7::KvDeleteRangeRequest,
) -> Result<v8::KvDeleteRangeRequest> {
	Ok(v8::KvDeleteRangeRequest {
		start: x.start,
		end: x.end,
	})
}

pub fn convert_kv_error_response_v7_to_v8(x: v7::KvErrorResponse) -> Result<v8::KvErrorResponse> {
	Ok(v8::KvErrorResponse { message: x.message })
}

pub fn convert_kv_get_response_v7_to_v8(x: v7::KvGetResponse) -> Result<v8::KvGetResponse> {
	Ok(v8::KvGetResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_list_response_v7_to_v8(x: v7::KvListResponse) -> Result<v8::KvListResponse> {
	Ok(v8::KvListResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_request_data_v7_to_v8(x: v7::KvRequestData) -> Result<v8::KvRequestData> {
	Ok(match x {
		v7::KvRequestData::KvGetRequest(v) => {
			v8::KvRequestData::KvGetRequest(convert_kv_get_request_v7_to_v8(v)?)
		}
		v7::KvRequestData::KvListRequest(v) => {
			v8::KvRequestData::KvListRequest(convert_kv_list_request_v7_to_v8(v)?)
		}
		v7::KvRequestData::KvPutRequest(v) => {
			v8::KvRequestData::KvPutRequest(convert_kv_put_request_v7_to_v8(v)?)
		}
		v7::KvRequestData::KvDeleteRequest(v) => {
			v8::KvRequestData::KvDeleteRequest(convert_kv_delete_request_v7_to_v8(v)?)
		}
		v7::KvRequestData::KvDeleteRangeRequest(v) => {
			v8::KvRequestData::KvDeleteRangeRequest(convert_kv_delete_range_request_v7_to_v8(v)?)
		}
		v7::KvRequestData::KvDropRequest => v8::KvRequestData::KvDropRequest,
	})
}

pub fn convert_kv_response_data_v7_to_v8(x: v7::KvResponseData) -> Result<v8::KvResponseData> {
	Ok(match x {
		v7::KvResponseData::KvErrorResponse(v) => {
			v8::KvResponseData::KvErrorResponse(convert_kv_error_response_v7_to_v8(v)?)
		}
		v7::KvResponseData::KvGetResponse(v) => {
			v8::KvResponseData::KvGetResponse(convert_kv_get_response_v7_to_v8(v)?)
		}
		v7::KvResponseData::KvListResponse(v) => {
			v8::KvResponseData::KvListResponse(convert_kv_list_response_v7_to_v8(v)?)
		}
		v7::KvResponseData::KvPutResponse => v8::KvResponseData::KvPutResponse,
		v7::KvResponseData::KvDeleteResponse => v8::KvResponseData::KvDeleteResponse,
		v7::KvResponseData::KvDropResponse => v8::KvResponseData::KvDropResponse,
	})
}

pub fn convert_sqlite_dirty_page_v7_to_v8(x: v7::SqliteDirtyPage) -> Result<v8::SqliteDirtyPage> {
	Ok(v8::SqliteDirtyPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_fetched_page_v7_to_v8(
	x: v7::SqliteFetchedPage,
) -> Result<v8::SqliteFetchedPage> {
	Ok(v8::SqliteFetchedPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_get_pages_request_v7_to_v8(
	x: v7::SqliteGetPagesRequest,
) -> Result<v8::SqliteGetPagesRequest> {
	Ok(v8::SqliteGetPagesRequest {
		actor_id: x.actor_id,
		pgnos: x.pgnos,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_get_pages_ok_v7_to_v8(
	x: v7::SqliteGetPagesOk,
) -> Result<v8::SqliteGetPagesOk> {
	Ok(v8::SqliteGetPagesOk {
		pages: x
			.pages
			.into_iter()
			.map(|v| convert_sqlite_fetched_page_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_error_response_v7_to_v8(
	x: v7::SqliteErrorResponse,
) -> Result<v8::SqliteErrorResponse> {
	Ok(v8::SqliteErrorResponse {
		group: x.group,
		code: x.code,
		message: x.message,
	})
}

pub fn convert_sqlite_get_pages_response_v7_to_v8(
	x: v7::SqliteGetPagesResponse,
) -> Result<v8::SqliteGetPagesResponse> {
	Ok(match x {
		v7::SqliteGetPagesResponse::SqliteGetPagesOk(v) => {
			v8::SqliteGetPagesResponse::SqliteGetPagesOk(convert_sqlite_get_pages_ok_v7_to_v8(v)?)
		}
		v7::SqliteGetPagesResponse::SqliteErrorResponse(v) => {
			v8::SqliteGetPagesResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v8(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_commit_request_v7_to_v8(
	x: v7::SqliteCommitRequest,
) -> Result<v8::SqliteCommitRequest> {
	Ok(v8::SqliteCommitRequest {
		actor_id: x.actor_id,
		dirty_pages: x
			.dirty_pages
			.into_iter()
			.map(|v| convert_sqlite_dirty_page_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
		db_size_pages: x.db_size_pages,
		now_ms: x.now_ms,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_commit_ok_v7_to_v8(x: v7::SqliteCommitOk) -> Result<v8::SqliteCommitOk> {
	Ok(v8::SqliteCommitOk {
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_commit_response_v7_to_v8(
	x: v7::SqliteCommitResponse,
) -> Result<v8::SqliteCommitResponse> {
	Ok(match x {
		v7::SqliteCommitResponse::SqliteCommitOk(v) => {
			v8::SqliteCommitResponse::SqliteCommitOk(convert_sqlite_commit_ok_v7_to_v8(v)?)
		}
		v7::SqliteCommitResponse::SqliteErrorResponse(v) => {
			v8::SqliteCommitResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v8(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_value_integer_v7_to_v8(
	x: v7::SqliteValueInteger,
) -> Result<v8::SqliteValueInteger> {
	Ok(v8::SqliteValueInteger { value: x.value })
}

pub fn convert_sqlite_value_float_v7_to_v8(
	x: v7::SqliteValueFloat,
) -> Result<v8::SqliteValueFloat> {
	Ok(v8::SqliteValueFloat { value: x.value })
}

pub fn convert_sqlite_value_text_v7_to_v8(x: v7::SqliteValueText) -> Result<v8::SqliteValueText> {
	Ok(v8::SqliteValueText { value: x.value })
}

pub fn convert_sqlite_value_blob_v7_to_v8(x: v7::SqliteValueBlob) -> Result<v8::SqliteValueBlob> {
	Ok(v8::SqliteValueBlob { value: x.value })
}

pub fn convert_sqlite_bind_param_v7_to_v8(x: v7::SqliteBindParam) -> Result<v8::SqliteBindParam> {
	Ok(match x {
		v7::SqliteBindParam::SqliteValueNull => v8::SqliteBindParam::SqliteValueNull,
		v7::SqliteBindParam::SqliteValueInteger(v) => {
			v8::SqliteBindParam::SqliteValueInteger(convert_sqlite_value_integer_v7_to_v8(v)?)
		}
		v7::SqliteBindParam::SqliteValueFloat(v) => {
			v8::SqliteBindParam::SqliteValueFloat(convert_sqlite_value_float_v7_to_v8(v)?)
		}
		v7::SqliteBindParam::SqliteValueText(v) => {
			v8::SqliteBindParam::SqliteValueText(convert_sqlite_value_text_v7_to_v8(v)?)
		}
		v7::SqliteBindParam::SqliteValueBlob(v) => {
			v8::SqliteBindParam::SqliteValueBlob(convert_sqlite_value_blob_v7_to_v8(v)?)
		}
	})
}

pub fn convert_sqlite_column_value_v7_to_v8(
	x: v7::SqliteColumnValue,
) -> Result<v8::SqliteColumnValue> {
	Ok(match x {
		v7::SqliteColumnValue::SqliteValueNull => v8::SqliteColumnValue::SqliteValueNull,
		v7::SqliteColumnValue::SqliteValueInteger(v) => {
			v8::SqliteColumnValue::SqliteValueInteger(convert_sqlite_value_integer_v7_to_v8(v)?)
		}
		v7::SqliteColumnValue::SqliteValueFloat(v) => {
			v8::SqliteColumnValue::SqliteValueFloat(convert_sqlite_value_float_v7_to_v8(v)?)
		}
		v7::SqliteColumnValue::SqliteValueText(v) => {
			v8::SqliteColumnValue::SqliteValueText(convert_sqlite_value_text_v7_to_v8(v)?)
		}
		v7::SqliteColumnValue::SqliteValueBlob(v) => {
			v8::SqliteColumnValue::SqliteValueBlob(convert_sqlite_value_blob_v7_to_v8(v)?)
		}
	})
}

pub fn convert_sqlite_query_result_v7_to_v8(
	x: v7::SqliteQueryResult,
) -> Result<v8::SqliteQueryResult> {
	Ok(v8::SqliteQueryResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v7_to_v8(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_execute_result_v7_to_v8(
	x: v7::SqliteExecuteResult,
) -> Result<v8::SqliteExecuteResult> {
	Ok(v8::SqliteExecuteResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v7_to_v8(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
		changes: x.changes,
		last_insert_row_id: x.last_insert_row_id,
	})
}

pub fn convert_sqlite_exec_request_v7_to_v8(
	x: v7::SqliteExecRequest,
) -> Result<v8::SqliteExecRequest> {
	Ok(v8::SqliteExecRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
	})
}

pub fn convert_sqlite_execute_request_v7_to_v8(
	x: v7::SqliteExecuteRequest,
) -> Result<v8::SqliteExecuteRequest> {
	Ok(v8::SqliteExecuteRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v7_to_v8(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_batch_statement_v7_to_v8(
	x: v7::SqliteBatchStatement,
) -> Result<v8::SqliteBatchStatement> {
	Ok(v8::SqliteBatchStatement {
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v7_to_v8(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_execute_batch_request_v7_to_v8(
	x: v7::SqliteExecuteBatchRequest,
) -> Result<v8::SqliteExecuteBatchRequest> {
	Ok(v8::SqliteExecuteBatchRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		statements: x
			.statements
			.into_iter()
			.map(|v| convert_sqlite_batch_statement_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_ok_v7_to_v8(x: v7::SqliteExecOk) -> Result<v8::SqliteExecOk> {
	Ok(v8::SqliteExecOk {
		result: convert_sqlite_query_result_v7_to_v8(x.result)?,
	})
}

pub fn convert_sqlite_execute_ok_v7_to_v8(x: v7::SqliteExecuteOk) -> Result<v8::SqliteExecuteOk> {
	Ok(v8::SqliteExecuteOk {
		result: convert_sqlite_execute_result_v7_to_v8(x.result)?,
	})
}

pub fn convert_sqlite_execute_batch_ok_v7_to_v8(
	x: v7::SqliteExecuteBatchOk,
) -> Result<v8::SqliteExecuteBatchOk> {
	Ok(v8::SqliteExecuteBatchOk {
		results: x
			.results
			.into_iter()
			.map(|v| convert_sqlite_execute_result_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_response_v7_to_v8(
	x: v7::SqliteExecResponse,
) -> Result<v8::SqliteExecResponse> {
	Ok(match x {
		v7::SqliteExecResponse::SqliteExecOk(v) => {
			v8::SqliteExecResponse::SqliteExecOk(convert_sqlite_exec_ok_v7_to_v8(v)?)
		}
		v7::SqliteExecResponse::SqliteErrorResponse(v) => {
			v8::SqliteExecResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v8(v)?)
		}
	})
}

pub fn convert_sqlite_execute_response_v7_to_v8(
	x: v7::SqliteExecuteResponse,
) -> Result<v8::SqliteExecuteResponse> {
	Ok(match x {
		v7::SqliteExecuteResponse::SqliteExecuteOk(v) => {
			v8::SqliteExecuteResponse::SqliteExecuteOk(convert_sqlite_execute_ok_v7_to_v8(v)?)
		}
		v7::SqliteExecuteResponse::SqliteErrorResponse(v) => {
			v8::SqliteExecuteResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v8(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_execute_batch_response_v7_to_v8(
	x: v7::SqliteExecuteBatchResponse,
) -> Result<v8::SqliteExecuteBatchResponse> {
	Ok(match x {
		v7::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(v) => {
			v8::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(
				convert_sqlite_execute_batch_ok_v7_to_v8(v)?,
			)
		}
		v7::SqliteExecuteBatchResponse::SqliteErrorResponse(v) => {
			v8::SqliteExecuteBatchResponse::SqliteErrorResponse(
				convert_sqlite_error_response_v7_to_v8(v)?,
			)
		}
	})
}

pub fn convert_stop_code_v7_to_v8(x: v7::StopCode) -> Result<v8::StopCode> {
	Ok(match x {
		v7::StopCode::Ok => v8::StopCode::Ok,
		v7::StopCode::Error => v8::StopCode::Error,
	})
}

pub fn convert_actor_name_v7_to_v8(x: v7::ActorName) -> Result<v8::ActorName> {
	Ok(v8::ActorName {
		metadata: x.metadata,
	})
}

pub fn convert_actor_config_v7_to_v8(x: v7::ActorConfig) -> Result<v8::ActorConfig> {
	Ok(v8::ActorConfig {
		name: x.name,
		key: x.key,
		create_ts: x.create_ts,
		input: x.input,
	})
}

pub fn convert_actor_checkpoint_v7_to_v8(x: v7::ActorCheckpoint) -> Result<v8::ActorCheckpoint> {
	Ok(v8::ActorCheckpoint {
		actor_id: x.actor_id,
		generation: x.generation,
		index: x.index,
	})
}

pub fn convert_actor_intent_v7_to_v8(x: v7::ActorIntent) -> Result<v8::ActorIntent> {
	Ok(match x {
		v7::ActorIntent::ActorIntentSleep => v8::ActorIntent::ActorIntentSleep,
		v7::ActorIntent::ActorIntentStop => v8::ActorIntent::ActorIntentStop,
	})
}

pub fn convert_actor_state_stopped_v7_to_v8(
	x: v7::ActorStateStopped,
) -> Result<v8::ActorStateStopped> {
	Ok(v8::ActorStateStopped {
		code: convert_stop_code_v7_to_v8(x.code)?,
		message: x.message,
	})
}

pub fn convert_actor_state_v7_to_v8(x: v7::ActorState) -> Result<v8::ActorState> {
	Ok(match x {
		v7::ActorState::ActorStateRunning => v8::ActorState::ActorStateRunning,
		v7::ActorState::ActorStateStopped(v) => {
			v8::ActorState::ActorStateStopped(convert_actor_state_stopped_v7_to_v8(v)?)
		}
	})
}

pub fn convert_event_actor_intent_v7_to_v8(
	x: v7::EventActorIntent,
) -> Result<v8::EventActorIntent> {
	Ok(v8::EventActorIntent {
		intent: convert_actor_intent_v7_to_v8(x.intent)?,
	})
}

pub fn convert_event_actor_state_update_v7_to_v8(
	x: v7::EventActorStateUpdate,
) -> Result<v8::EventActorStateUpdate> {
	Ok(v8::EventActorStateUpdate {
		state: convert_actor_state_v7_to_v8(x.state)?,
	})
}

pub fn convert_event_actor_set_alarm_v7_to_v8(
	x: v7::EventActorSetAlarm,
) -> Result<v8::EventActorSetAlarm> {
	Ok(v8::EventActorSetAlarm {
		alarm_ts: x.alarm_ts,
	})
}

pub fn convert_event_v7_to_v8(x: v7::Event) -> Result<v8::Event> {
	Ok(match x {
		v7::Event::EventActorIntent(v) => {
			v8::Event::EventActorIntent(convert_event_actor_intent_v7_to_v8(v)?)
		}
		v7::Event::EventActorStateUpdate(v) => {
			v8::Event::EventActorStateUpdate(convert_event_actor_state_update_v7_to_v8(v)?)
		}
		v7::Event::EventActorSetAlarm(v) => {
			v8::Event::EventActorSetAlarm(convert_event_actor_set_alarm_v7_to_v8(v)?)
		}
	})
}

pub fn convert_event_wrapper_v7_to_v8(x: v7::EventWrapper) -> Result<v8::EventWrapper> {
	Ok(v8::EventWrapper {
		checkpoint: convert_actor_checkpoint_v7_to_v8(x.checkpoint)?,
		inner: convert_event_v7_to_v8(x.inner)?,
	})
}

pub fn convert_preloaded_kv_entry_v7_to_v8(
	x: v7::PreloadedKvEntry,
) -> Result<v8::PreloadedKvEntry> {
	Ok(v8::PreloadedKvEntry {
		key: x.key,
		value: x.value,
		metadata: convert_kv_metadata_v7_to_v8(x.metadata)?,
	})
}

pub fn convert_preloaded_kv_v7_to_v8(x: v7::PreloadedKv) -> Result<v8::PreloadedKv> {
	Ok(v8::PreloadedKv {
		entries: x
			.entries
			.into_iter()
			.map(|v| convert_preloaded_kv_entry_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
		requested_get_keys: x.requested_get_keys,
		requested_prefixes: x.requested_prefixes,
	})
}

pub fn convert_hibernating_request_v7_to_v8(
	x: v7::HibernatingRequest,
) -> Result<v8::HibernatingRequest> {
	Ok(v8::HibernatingRequest {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
	})
}

pub fn convert_command_start_actor_v7_to_v8(
	x: v7::CommandStartActor,
) -> Result<v8::CommandStartActor> {
	Ok(v8::CommandStartActor {
		config: convert_actor_config_v7_to_v8(x.config)?,
		hibernating_requests: x
			.hibernating_requests
			.into_iter()
			.map(|v| convert_hibernating_request_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
		preloaded_kv: x
			.preloaded_kv
			.map(|v| convert_preloaded_kv_v7_to_v8(v))
			.transpose()?,
	})
}

pub fn convert_stop_actor_reason_v7_to_v8(x: v7::StopActorReason) -> Result<v8::StopActorReason> {
	Ok(match x {
		v7::StopActorReason::SleepIntent => v8::StopActorReason::SleepIntent,
		v7::StopActorReason::StopIntent => v8::StopActorReason::StopIntent,
		v7::StopActorReason::Destroy => v8::StopActorReason::Destroy,
		v7::StopActorReason::GoingAway => v8::StopActorReason::GoingAway,
		v7::StopActorReason::Lost => v8::StopActorReason::Lost,
	})
}

pub fn convert_command_stop_actor_v7_to_v8(
	x: v7::CommandStopActor,
) -> Result<v8::CommandStopActor> {
	Ok(v8::CommandStopActor {
		reason: convert_stop_actor_reason_v7_to_v8(x.reason)?,
	})
}

pub fn convert_command_v7_to_v8(x: v7::Command) -> Result<v8::Command> {
	Ok(match x {
		v7::Command::CommandStartActor(v) => {
			v8::Command::CommandStartActor(convert_command_start_actor_v7_to_v8(v)?)
		}
		v7::Command::CommandStopActor(v) => {
			v8::Command::CommandStopActor(convert_command_stop_actor_v7_to_v8(v)?)
		}
	})
}

pub fn convert_command_wrapper_v7_to_v8(x: v7::CommandWrapper) -> Result<v8::CommandWrapper> {
	Ok(v8::CommandWrapper {
		checkpoint: convert_actor_checkpoint_v7_to_v8(x.checkpoint)?,
		inner: convert_command_v7_to_v8(x.inner)?,
	})
}

pub fn convert_actor_command_key_data_v7_to_v8(
	x: v7::ActorCommandKeyData,
) -> Result<v8::ActorCommandKeyData> {
	Ok(match x {
		v7::ActorCommandKeyData::CommandStartActor(v) => {
			v8::ActorCommandKeyData::CommandStartActor(convert_command_start_actor_v7_to_v8(v)?)
		}
		v7::ActorCommandKeyData::CommandStopActor(v) => {
			v8::ActorCommandKeyData::CommandStopActor(convert_command_stop_actor_v7_to_v8(v)?)
		}
	})
}

pub fn convert_message_id_v7_to_v8(x: v7::MessageId) -> Result<v8::MessageId> {
	Ok(v8::MessageId {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		message_index: x.message_index,
	})
}

pub fn convert_to_envoy_request_start_v7_to_v8(
	x: v7::ToEnvoyRequestStart,
) -> Result<v8::ToEnvoyRequestStart> {
	Ok(v8::ToEnvoyRequestStart {
		actor_id: x.actor_id,
		method: x.method,
		path: x.path,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_envoy_request_chunk_v7_to_v8(
	x: v7::ToEnvoyRequestChunk,
) -> Result<v8::ToEnvoyRequestChunk> {
	Ok(v8::ToEnvoyRequestChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_http_stream_abort_reason_kind_v7_to_v8(
	x: v7::HttpStreamAbortReasonKind,
) -> Result<v8::HttpStreamAbortReasonKind> {
	Ok(match x {
		v7::HttpStreamAbortReasonKind::Unknown => v8::HttpStreamAbortReasonKind::Unknown,
		v7::HttpStreamAbortReasonKind::ClientDisconnect => {
			v8::HttpStreamAbortReasonKind::ClientDisconnect
		}
		v7::HttpStreamAbortReasonKind::HandlerError => v8::HttpStreamAbortReasonKind::HandlerError,
		v7::HttpStreamAbortReasonKind::IdleTimeout => v8::HttpStreamAbortReasonKind::IdleTimeout,
		v7::HttpStreamAbortReasonKind::Overloaded => v8::HttpStreamAbortReasonKind::Overloaded,
		v7::HttpStreamAbortReasonKind::BodyTooLarge => v8::HttpStreamAbortReasonKind::BodyTooLarge,
		v7::HttpStreamAbortReasonKind::OutOfMemory => v8::HttpStreamAbortReasonKind::OutOfMemory,
		v7::HttpStreamAbortReasonKind::Shutdown => v8::HttpStreamAbortReasonKind::Shutdown,
		v7::HttpStreamAbortReasonKind::InternalError => {
			v8::HttpStreamAbortReasonKind::InternalError
		}
	})
}

pub fn convert_http_stream_abort_reason_v7_to_v8(
	x: v7::HttpStreamAbortReason,
) -> Result<v8::HttpStreamAbortReason> {
	Ok(v8::HttpStreamAbortReason {
		kind: convert_http_stream_abort_reason_kind_v7_to_v8(x.kind)?,
		detail: x.detail,
	})
}

pub fn convert_to_envoy_request_abort_v7_to_v8(
	x: v7::ToEnvoyRequestAbort,
) -> Result<v8::ToEnvoyRequestAbort> {
	Ok(v8::ToEnvoyRequestAbort {
		reason: convert_http_stream_abort_reason_v7_to_v8(x.reason)?,
	})
}

pub fn convert_to_rivet_response_abort_v7_to_v8(
	x: v7::ToRivetResponseAbort,
) -> Result<v8::ToRivetResponseAbort> {
	Ok(v8::ToRivetResponseAbort {
		reason: convert_http_stream_abort_reason_v7_to_v8(x.reason)?,
	})
}

pub fn convert_to_rivet_response_start_v7_to_v8(
	x: v7::ToRivetResponseStart,
) -> Result<v8::ToRivetResponseStart> {
	Ok(v8::ToRivetResponseStart {
		status: x.status,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_rivet_response_chunk_v7_to_v8(
	x: v7::ToRivetResponseChunk,
) -> Result<v8::ToRivetResponseChunk> {
	Ok(v8::ToRivetResponseChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_to_envoy_web_socket_open_v7_to_v8(
	x: v7::ToEnvoyWebSocketOpen,
) -> Result<v8::ToEnvoyWebSocketOpen> {
	Ok(v8::ToEnvoyWebSocketOpen {
		actor_id: x.actor_id,
		path: x.path,
		headers: x.headers,
	})
}

pub fn convert_to_envoy_web_socket_message_v7_to_v8(
	x: v7::ToEnvoyWebSocketMessage,
) -> Result<v8::ToEnvoyWebSocketMessage> {
	Ok(v8::ToEnvoyWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_envoy_web_socket_close_v7_to_v8(
	x: v7::ToEnvoyWebSocketClose,
) -> Result<v8::ToEnvoyWebSocketClose> {
	Ok(v8::ToEnvoyWebSocketClose {
		code: x.code,
		reason: x.reason,
	})
}

pub fn convert_to_rivet_web_socket_open_v7_to_v8(
	x: v7::ToRivetWebSocketOpen,
) -> Result<v8::ToRivetWebSocketOpen> {
	Ok(v8::ToRivetWebSocketOpen {
		can_hibernate: x.can_hibernate,
	})
}

pub fn convert_to_rivet_web_socket_message_v7_to_v8(
	x: v7::ToRivetWebSocketMessage,
) -> Result<v8::ToRivetWebSocketMessage> {
	Ok(v8::ToRivetWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_rivet_web_socket_message_ack_v7_to_v8(
	x: v7::ToRivetWebSocketMessageAck,
) -> Result<v8::ToRivetWebSocketMessageAck> {
	Ok(v8::ToRivetWebSocketMessageAck { index: x.index })
}

pub fn convert_to_rivet_web_socket_close_v7_to_v8(
	x: v7::ToRivetWebSocketClose,
) -> Result<v8::ToRivetWebSocketClose> {
	Ok(v8::ToRivetWebSocketClose {
		code: x.code,
		reason: x.reason,
		hibernate: x.hibernate,
	})
}

pub fn convert_to_rivet_tunnel_message_kind_v7_to_v8(
	x: v7::ToRivetTunnelMessageKind,
) -> Result<v8::ToRivetTunnelMessageKind> {
	Ok(match x {
		v7::ToRivetTunnelMessageKind::ToRivetResponseStart(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetResponseStart(
				convert_to_rivet_response_start_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetResponseChunk(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				convert_to_rivet_response_chunk_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetResponseAbort(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				convert_to_rivet_response_abort_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(
				convert_to_rivet_web_socket_open_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(
				convert_to_rivet_web_socket_message_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(
				convert_to_rivet_web_socket_message_ack_v7_to_v8(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketClose(v) => {
			v8::ToRivetTunnelMessageKind::ToRivetWebSocketClose(
				convert_to_rivet_web_socket_close_v7_to_v8(v)?,
			)
		}
	})
}

pub fn convert_to_rivet_tunnel_message_v7_to_v8(
	x: v7::ToRivetTunnelMessage,
) -> Result<v8::ToRivetTunnelMessage> {
	Ok(v8::ToRivetTunnelMessage {
		message_id: convert_message_id_v7_to_v8(x.message_id)?,
		message_kind: convert_to_rivet_tunnel_message_kind_v7_to_v8(x.message_kind)?,
	})
}

pub fn convert_to_envoy_tunnel_message_kind_v7_to_v8(
	x: v7::ToEnvoyTunnelMessageKind,
) -> Result<v8::ToEnvoyTunnelMessageKind> {
	Ok(match x {
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				convert_to_envoy_request_start_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				convert_to_envoy_request_chunk_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				convert_to_envoy_request_abort_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				convert_to_envoy_web_socket_open_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				convert_to_envoy_web_socket_message_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(v) => {
			v8::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
				convert_to_envoy_web_socket_close_v7_to_v8(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_tunnel_message_v7_to_v8(
	x: v7::ToEnvoyTunnelMessage,
) -> Result<v8::ToEnvoyTunnelMessage> {
	Ok(v8::ToEnvoyTunnelMessage {
		message_id: convert_message_id_v7_to_v8(x.message_id)?,
		message_kind: convert_to_envoy_tunnel_message_kind_v7_to_v8(x.message_kind)?,
	})
}

pub fn convert_to_envoy_ping_v7_to_v8(x: v7::ToEnvoyPing) -> Result<v8::ToEnvoyPing> {
	Ok(v8::ToEnvoyPing { ts: x.ts })
}

pub fn convert_to_rivet_metadata_v7_to_v8(x: v7::ToRivetMetadata) -> Result<v8::ToRivetMetadata> {
	Ok(v8::ToRivetMetadata {
		prepopulate_actor_names: x
			.prepopulate_actor_names
			.map(|v| {
				v.into_iter()
					.map(|(k, v)| -> Result<_> { Ok((k, convert_actor_name_v7_to_v8(v)?)) })
					.collect::<Result<_>>()
			})
			.transpose()?,
		metadata: x.metadata,
	})
}

pub fn convert_to_rivet_events_v7_to_v8(x: v7::ToRivetEvents) -> Result<v8::ToRivetEvents> {
	Ok(x.into_iter()
		.map(|v| convert_event_wrapper_v7_to_v8(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_rivet_ack_commands_v7_to_v8(
	x: v7::ToRivetAckCommands,
) -> Result<v8::ToRivetAckCommands> {
	Ok(v8::ToRivetAckCommands {
		last_command_checkpoints: x
			.last_command_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_rivet_pong_v7_to_v8(x: v7::ToRivetPong) -> Result<v8::ToRivetPong> {
	Ok(v8::ToRivetPong { ts: x.ts })
}

pub fn convert_to_rivet_kv_request_v7_to_v8(
	x: v7::ToRivetKvRequest,
) -> Result<v8::ToRivetKvRequest> {
	Ok(v8::ToRivetKvRequest {
		actor_id: x.actor_id,
		request_id: x.request_id,
		data: convert_kv_request_data_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_get_pages_request_v7_to_v8(
	x: v7::ToRivetSqliteGetPagesRequest,
) -> Result<v8::ToRivetSqliteGetPagesRequest> {
	Ok(v8::ToRivetSqliteGetPagesRequest {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_request_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_commit_request_v7_to_v8(
	x: v7::ToRivetSqliteCommitRequest,
) -> Result<v8::ToRivetSqliteCommitRequest> {
	Ok(v8::ToRivetSqliteCommitRequest {
		request_id: x.request_id,
		data: convert_sqlite_commit_request_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_exec_request_v7_to_v8(
	x: v7::ToRivetSqliteExecRequest,
) -> Result<v8::ToRivetSqliteExecRequest> {
	Ok(v8::ToRivetSqliteExecRequest {
		request_id: x.request_id,
		data: convert_sqlite_exec_request_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_request_v7_to_v8(
	x: v7::ToRivetSqliteExecuteRequest,
) -> Result<v8::ToRivetSqliteExecuteRequest> {
	Ok(v8::ToRivetSqliteExecuteRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_request_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_batch_request_v7_to_v8(
	x: v7::ToRivetSqliteExecuteBatchRequest,
) -> Result<v8::ToRivetSqliteExecuteBatchRequest> {
	Ok(v8::ToRivetSqliteExecuteBatchRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_request_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_rivet_v7_to_v8(x: v7::ToRivet) -> Result<v8::ToRivet> {
	Ok(match x {
		v7::ToRivet::ToRivetMetadata(v) => {
			v8::ToRivet::ToRivetMetadata(convert_to_rivet_metadata_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetEvents(v) => {
			v8::ToRivet::ToRivetEvents(convert_to_rivet_events_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetAckCommands(v) => {
			v8::ToRivet::ToRivetAckCommands(convert_to_rivet_ack_commands_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetStopping => v8::ToRivet::ToRivetStopping,
		v7::ToRivet::ToRivetPong(v) => v8::ToRivet::ToRivetPong(convert_to_rivet_pong_v7_to_v8(v)?),
		v7::ToRivet::ToRivetKvRequest(v) => {
			v8::ToRivet::ToRivetKvRequest(convert_to_rivet_kv_request_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetTunnelMessage(v) => {
			v8::ToRivet::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetSqliteGetPagesRequest(v) => v8::ToRivet::ToRivetSqliteGetPagesRequest(
			convert_to_rivet_sqlite_get_pages_request_v7_to_v8(v)?,
		),
		v7::ToRivet::ToRivetSqliteCommitRequest(v) => v8::ToRivet::ToRivetSqliteCommitRequest(
			convert_to_rivet_sqlite_commit_request_v7_to_v8(v)?,
		),
		v7::ToRivet::ToRivetSqliteExecRequest(v) => {
			v8::ToRivet::ToRivetSqliteExecRequest(convert_to_rivet_sqlite_exec_request_v7_to_v8(v)?)
		}
		v7::ToRivet::ToRivetSqliteExecuteRequest(v) => v8::ToRivet::ToRivetSqliteExecuteRequest(
			convert_to_rivet_sqlite_execute_request_v7_to_v8(v)?,
		),
		v7::ToRivet::ToRivetSqliteExecuteBatchRequest(v) => {
			v8::ToRivet::ToRivetSqliteExecuteBatchRequest(
				convert_to_rivet_sqlite_execute_batch_request_v7_to_v8(v)?,
			)
		}
	})
}

pub fn convert_protocol_metadata_v7_to_v8(x: v7::ProtocolMetadata) -> Result<v8::ProtocolMetadata> {
	Ok(v8::ProtocolMetadata {
		envoy_lost_threshold: x.envoy_lost_threshold,
		actor_stop_threshold: x.actor_stop_threshold,
		max_response_payload_size: x.max_response_payload_size,
	})
}

pub fn convert_to_envoy_init_v7_to_v8(x: v7::ToEnvoyInit) -> Result<v8::ToEnvoyInit> {
	Ok(v8::ToEnvoyInit {
		metadata: convert_protocol_metadata_v7_to_v8(x.metadata)?,
	})
}

pub fn convert_to_envoy_commands_v7_to_v8(x: v7::ToEnvoyCommands) -> Result<v8::ToEnvoyCommands> {
	Ok(x.into_iter()
		.map(|v| convert_command_wrapper_v7_to_v8(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_envoy_ack_events_v7_to_v8(
	x: v7::ToEnvoyAckEvents,
) -> Result<v8::ToEnvoyAckEvents> {
	Ok(v8::ToEnvoyAckEvents {
		last_event_checkpoints: x
			.last_event_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v7_to_v8(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_envoy_kv_response_v7_to_v8(
	x: v7::ToEnvoyKvResponse,
) -> Result<v8::ToEnvoyKvResponse> {
	Ok(v8::ToEnvoyKvResponse {
		request_id: x.request_id,
		data: convert_kv_response_data_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_get_pages_response_v7_to_v8(
	x: v7::ToEnvoySqliteGetPagesResponse,
) -> Result<v8::ToEnvoySqliteGetPagesResponse> {
	Ok(v8::ToEnvoySqliteGetPagesResponse {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_response_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_commit_response_v7_to_v8(
	x: v7::ToEnvoySqliteCommitResponse,
) -> Result<v8::ToEnvoySqliteCommitResponse> {
	Ok(v8::ToEnvoySqliteCommitResponse {
		request_id: x.request_id,
		data: convert_sqlite_commit_response_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_exec_response_v7_to_v8(
	x: v7::ToEnvoySqliteExecResponse,
) -> Result<v8::ToEnvoySqliteExecResponse> {
	Ok(v8::ToEnvoySqliteExecResponse {
		request_id: x.request_id,
		data: convert_sqlite_exec_response_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_response_v7_to_v8(
	x: v7::ToEnvoySqliteExecuteResponse,
) -> Result<v8::ToEnvoySqliteExecuteResponse> {
	Ok(v8::ToEnvoySqliteExecuteResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_response_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_batch_response_v7_to_v8(
	x: v7::ToEnvoySqliteExecuteBatchResponse,
) -> Result<v8::ToEnvoySqliteExecuteBatchResponse> {
	Ok(v8::ToEnvoySqliteExecuteBatchResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_response_v7_to_v8(x.data)?,
	})
}

pub fn convert_to_envoy_v7_to_v8(x: v7::ToEnvoy) -> Result<v8::ToEnvoy> {
	Ok(match x {
		v7::ToEnvoy::ToEnvoyInit(v) => v8::ToEnvoy::ToEnvoyInit(convert_to_envoy_init_v7_to_v8(v)?),
		v7::ToEnvoy::ToEnvoyCommands(v) => {
			v8::ToEnvoy::ToEnvoyCommands(convert_to_envoy_commands_v7_to_v8(v)?)
		}
		v7::ToEnvoy::ToEnvoyAckEvents(v) => {
			v8::ToEnvoy::ToEnvoyAckEvents(convert_to_envoy_ack_events_v7_to_v8(v)?)
		}
		v7::ToEnvoy::ToEnvoyKvResponse(v) => {
			v8::ToEnvoy::ToEnvoyKvResponse(convert_to_envoy_kv_response_v7_to_v8(v)?)
		}
		v7::ToEnvoy::ToEnvoyTunnelMessage(v) => {
			v8::ToEnvoy::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v7_to_v8(v)?)
		}
		v7::ToEnvoy::ToEnvoyPing(v) => v8::ToEnvoy::ToEnvoyPing(convert_to_envoy_ping_v7_to_v8(v)?),
		v7::ToEnvoy::ToEnvoySqliteGetPagesResponse(v) => {
			v8::ToEnvoy::ToEnvoySqliteGetPagesResponse(
				convert_to_envoy_sqlite_get_pages_response_v7_to_v8(v)?,
			)
		}
		v7::ToEnvoy::ToEnvoySqliteCommitResponse(v) => v8::ToEnvoy::ToEnvoySqliteCommitResponse(
			convert_to_envoy_sqlite_commit_response_v7_to_v8(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecResponse(v) => v8::ToEnvoy::ToEnvoySqliteExecResponse(
			convert_to_envoy_sqlite_exec_response_v7_to_v8(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecuteResponse(v) => v8::ToEnvoy::ToEnvoySqliteExecuteResponse(
			convert_to_envoy_sqlite_execute_response_v7_to_v8(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(v) => {
			v8::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(
				convert_to_envoy_sqlite_execute_batch_response_v7_to_v8(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_conn_ping_v7_to_v8(x: v7::ToEnvoyConnPing) -> Result<v8::ToEnvoyConnPing> {
	Ok(v8::ToEnvoyConnPing {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_envoy_conn_v7_to_v8(x: v7::ToEnvoyConn) -> Result<v8::ToEnvoyConn> {
	Ok(match x {
		v7::ToEnvoyConn::ToEnvoyConnPing(v) => {
			v8::ToEnvoyConn::ToEnvoyConnPing(convert_to_envoy_conn_ping_v7_to_v8(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyConnClose => v8::ToEnvoyConn::ToEnvoyConnClose,
		v7::ToEnvoyConn::ToEnvoyCommands(v) => {
			v8::ToEnvoyConn::ToEnvoyCommands(convert_to_envoy_commands_v7_to_v8(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyAckEvents(v) => {
			v8::ToEnvoyConn::ToEnvoyAckEvents(convert_to_envoy_ack_events_v7_to_v8(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyTunnelMessage(v) => {
			v8::ToEnvoyConn::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v7_to_v8(v)?)
		}
	})
}

pub fn convert_to_gateway_pong_v7_to_v8(x: v7::ToGatewayPong) -> Result<v8::ToGatewayPong> {
	Ok(v8::ToGatewayPong {
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_gateway_v7_to_v8(x: v7::ToGateway) -> Result<v8::ToGateway> {
	Ok(match x {
		v7::ToGateway::ToGatewayPong(v) => {
			v8::ToGateway::ToGatewayPong(convert_to_gateway_pong_v7_to_v8(v)?)
		}
		v7::ToGateway::ToRivetTunnelMessage(v) => {
			v8::ToGateway::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v7_to_v8(v)?)
		}
	})
}

pub fn convert_to_outbound_actor_start_v7_to_v8(
	x: v7::ToOutboundActorStart,
) -> Result<v8::ToOutboundActorStart> {
	Ok(v8::ToOutboundActorStart {
		namespace_id: x.namespace_id,
		pool_name: x.pool_name,
		checkpoint: convert_actor_checkpoint_v7_to_v8(x.checkpoint)?,
		actor_config: convert_actor_config_v7_to_v8(x.actor_config)?,
	})
}

pub fn convert_to_outbound_v7_to_v8(x: v7::ToOutbound) -> Result<v8::ToOutbound> {
	Ok(match x {
		v7::ToOutbound::ToOutboundActorStart(v) => {
			v8::ToOutbound::ToOutboundActorStart(convert_to_outbound_actor_start_v7_to_v8(v)?)
		}
	})
}
