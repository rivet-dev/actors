// from: v7.bare, to: v6.bare

#![allow(dead_code, unused_variables)]

use anyhow::Result;

use crate::generated::{v6, v7};

pub fn convert_kv_metadata_v7_to_v6(x: v7::KvMetadata) -> Result<v6::KvMetadata> {
	Ok(v6::KvMetadata {
		version: x.version,
		update_ts: x.update_ts,
	})
}

pub fn convert_kv_list_range_query_v7_to_v6(
	x: v7::KvListRangeQuery,
) -> Result<v6::KvListRangeQuery> {
	Ok(v6::KvListRangeQuery {
		start: x.start,
		end: x.end,
		exclusive: x.exclusive,
	})
}

pub fn convert_kv_list_prefix_query_v7_to_v6(
	x: v7::KvListPrefixQuery,
) -> Result<v6::KvListPrefixQuery> {
	Ok(v6::KvListPrefixQuery { key: x.key })
}

pub fn convert_kv_list_query_v7_to_v6(x: v7::KvListQuery) -> Result<v6::KvListQuery> {
	Ok(match x {
		v7::KvListQuery::KvListAllQuery => v6::KvListQuery::KvListAllQuery,
		v7::KvListQuery::KvListRangeQuery(v) => {
			v6::KvListQuery::KvListRangeQuery(convert_kv_list_range_query_v7_to_v6(v)?)
		}
		v7::KvListQuery::KvListPrefixQuery(v) => {
			v6::KvListQuery::KvListPrefixQuery(convert_kv_list_prefix_query_v7_to_v6(v)?)
		}
	})
}

pub fn convert_kv_get_request_v7_to_v6(x: v7::KvGetRequest) -> Result<v6::KvGetRequest> {
	Ok(v6::KvGetRequest { keys: x.keys })
}

pub fn convert_kv_list_request_v7_to_v6(x: v7::KvListRequest) -> Result<v6::KvListRequest> {
	Ok(v6::KvListRequest {
		query: convert_kv_list_query_v7_to_v6(x.query)?,
		reverse: x.reverse,
		limit: x.limit,
	})
}

pub fn convert_kv_put_request_v7_to_v6(x: v7::KvPutRequest) -> Result<v6::KvPutRequest> {
	Ok(v6::KvPutRequest {
		keys: x.keys,
		values: x.values,
	})
}

pub fn convert_kv_delete_request_v7_to_v6(x: v7::KvDeleteRequest) -> Result<v6::KvDeleteRequest> {
	Ok(v6::KvDeleteRequest { keys: x.keys })
}

pub fn convert_kv_delete_range_request_v7_to_v6(
	x: v7::KvDeleteRangeRequest,
) -> Result<v6::KvDeleteRangeRequest> {
	Ok(v6::KvDeleteRangeRequest {
		start: x.start,
		end: x.end,
	})
}

pub fn convert_kv_error_response_v7_to_v6(x: v7::KvErrorResponse) -> Result<v6::KvErrorResponse> {
	Ok(v6::KvErrorResponse { message: x.message })
}

pub fn convert_kv_get_response_v7_to_v6(x: v7::KvGetResponse) -> Result<v6::KvGetResponse> {
	Ok(v6::KvGetResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_list_response_v7_to_v6(x: v7::KvListResponse) -> Result<v6::KvListResponse> {
	Ok(v6::KvListResponse {
		keys: x.keys,
		values: x.values,
		metadata: x
			.metadata
			.into_iter()
			.map(|v| convert_kv_metadata_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_kv_request_data_v7_to_v6(x: v7::KvRequestData) -> Result<v6::KvRequestData> {
	Ok(match x {
		v7::KvRequestData::KvGetRequest(v) => {
			v6::KvRequestData::KvGetRequest(convert_kv_get_request_v7_to_v6(v)?)
		}
		v7::KvRequestData::KvListRequest(v) => {
			v6::KvRequestData::KvListRequest(convert_kv_list_request_v7_to_v6(v)?)
		}
		v7::KvRequestData::KvPutRequest(v) => {
			v6::KvRequestData::KvPutRequest(convert_kv_put_request_v7_to_v6(v)?)
		}
		v7::KvRequestData::KvDeleteRequest(v) => {
			v6::KvRequestData::KvDeleteRequest(convert_kv_delete_request_v7_to_v6(v)?)
		}
		v7::KvRequestData::KvDeleteRangeRequest(v) => {
			v6::KvRequestData::KvDeleteRangeRequest(convert_kv_delete_range_request_v7_to_v6(v)?)
		}
		v7::KvRequestData::KvDropRequest => v6::KvRequestData::KvDropRequest,
	})
}

pub fn convert_kv_response_data_v7_to_v6(x: v7::KvResponseData) -> Result<v6::KvResponseData> {
	Ok(match x {
		v7::KvResponseData::KvErrorResponse(v) => {
			v6::KvResponseData::KvErrorResponse(convert_kv_error_response_v7_to_v6(v)?)
		}
		v7::KvResponseData::KvGetResponse(v) => {
			v6::KvResponseData::KvGetResponse(convert_kv_get_response_v7_to_v6(v)?)
		}
		v7::KvResponseData::KvListResponse(v) => {
			v6::KvResponseData::KvListResponse(convert_kv_list_response_v7_to_v6(v)?)
		}
		v7::KvResponseData::KvPutResponse => v6::KvResponseData::KvPutResponse,
		v7::KvResponseData::KvDeleteResponse => v6::KvResponseData::KvDeleteResponse,
		v7::KvResponseData::KvDropResponse => v6::KvResponseData::KvDropResponse,
	})
}

pub fn convert_sqlite_dirty_page_v7_to_v6(x: v7::SqliteDirtyPage) -> Result<v6::SqliteDirtyPage> {
	Ok(v6::SqliteDirtyPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_fetched_page_v7_to_v6(
	x: v7::SqliteFetchedPage,
) -> Result<v6::SqliteFetchedPage> {
	Ok(v6::SqliteFetchedPage {
		pgno: x.pgno,
		bytes: x.bytes,
	})
}

pub fn convert_sqlite_get_pages_request_v7_to_v6(
	x: v7::SqliteGetPagesRequest,
) -> Result<v6::SqliteGetPagesRequest> {
	Ok(v6::SqliteGetPagesRequest {
		actor_id: x.actor_id,
		pgnos: x.pgnos,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_get_pages_ok_v7_to_v6(
	x: v7::SqliteGetPagesOk,
) -> Result<v6::SqliteGetPagesOk> {
	Ok(v6::SqliteGetPagesOk {
		pages: x
			.pages
			.into_iter()
			.map(|v| convert_sqlite_fetched_page_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_error_response_v7_to_v6(
	x: v7::SqliteErrorResponse,
) -> Result<v6::SqliteErrorResponse> {
	Ok(v6::SqliteErrorResponse {
		group: x.group,
		code: x.code,
		message: x.message,
	})
}

pub fn convert_sqlite_get_pages_response_v7_to_v6(
	x: v7::SqliteGetPagesResponse,
) -> Result<v6::SqliteGetPagesResponse> {
	Ok(match x {
		v7::SqliteGetPagesResponse::SqliteGetPagesOk(v) => {
			v6::SqliteGetPagesResponse::SqliteGetPagesOk(convert_sqlite_get_pages_ok_v7_to_v6(v)?)
		}
		v7::SqliteGetPagesResponse::SqliteErrorResponse(v) => {
			v6::SqliteGetPagesResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v6(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_commit_request_v7_to_v6(
	x: v7::SqliteCommitRequest,
) -> Result<v6::SqliteCommitRequest> {
	Ok(v6::SqliteCommitRequest {
		actor_id: x.actor_id,
		dirty_pages: x
			.dirty_pages
			.into_iter()
			.map(|v| convert_sqlite_dirty_page_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
		db_size_pages: x.db_size_pages,
		now_ms: x.now_ms,
		expected_generation: x.expected_generation,
		expected_head_txid: x.expected_head_txid,
	})
}

pub fn convert_sqlite_commit_ok_v7_to_v6(x: v7::SqliteCommitOk) -> Result<v6::SqliteCommitOk> {
	Ok(v6::SqliteCommitOk {
		head_txid: x.head_txid,
	})
}

pub fn convert_sqlite_commit_response_v7_to_v6(
	x: v7::SqliteCommitResponse,
) -> Result<v6::SqliteCommitResponse> {
	Ok(match x {
		v7::SqliteCommitResponse::SqliteCommitOk(v) => {
			v6::SqliteCommitResponse::SqliteCommitOk(convert_sqlite_commit_ok_v7_to_v6(v)?)
		}
		v7::SqliteCommitResponse::SqliteErrorResponse(v) => {
			v6::SqliteCommitResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v6(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_value_integer_v7_to_v6(
	x: v7::SqliteValueInteger,
) -> Result<v6::SqliteValueInteger> {
	Ok(v6::SqliteValueInteger { value: x.value })
}

pub fn convert_sqlite_value_float_v7_to_v6(
	x: v7::SqliteValueFloat,
) -> Result<v6::SqliteValueFloat> {
	Ok(v6::SqliteValueFloat { value: x.value })
}

pub fn convert_sqlite_value_text_v7_to_v6(x: v7::SqliteValueText) -> Result<v6::SqliteValueText> {
	Ok(v6::SqliteValueText { value: x.value })
}

pub fn convert_sqlite_value_blob_v7_to_v6(x: v7::SqliteValueBlob) -> Result<v6::SqliteValueBlob> {
	Ok(v6::SqliteValueBlob { value: x.value })
}

pub fn convert_sqlite_bind_param_v7_to_v6(x: v7::SqliteBindParam) -> Result<v6::SqliteBindParam> {
	Ok(match x {
		v7::SqliteBindParam::SqliteValueNull => v6::SqliteBindParam::SqliteValueNull,
		v7::SqliteBindParam::SqliteValueInteger(v) => {
			v6::SqliteBindParam::SqliteValueInteger(convert_sqlite_value_integer_v7_to_v6(v)?)
		}
		v7::SqliteBindParam::SqliteValueFloat(v) => {
			v6::SqliteBindParam::SqliteValueFloat(convert_sqlite_value_float_v7_to_v6(v)?)
		}
		v7::SqliteBindParam::SqliteValueText(v) => {
			v6::SqliteBindParam::SqliteValueText(convert_sqlite_value_text_v7_to_v6(v)?)
		}
		v7::SqliteBindParam::SqliteValueBlob(v) => {
			v6::SqliteBindParam::SqliteValueBlob(convert_sqlite_value_blob_v7_to_v6(v)?)
		}
	})
}

pub fn convert_sqlite_column_value_v7_to_v6(
	x: v7::SqliteColumnValue,
) -> Result<v6::SqliteColumnValue> {
	Ok(match x {
		v7::SqliteColumnValue::SqliteValueNull => v6::SqliteColumnValue::SqliteValueNull,
		v7::SqliteColumnValue::SqliteValueInteger(v) => {
			v6::SqliteColumnValue::SqliteValueInteger(convert_sqlite_value_integer_v7_to_v6(v)?)
		}
		v7::SqliteColumnValue::SqliteValueFloat(v) => {
			v6::SqliteColumnValue::SqliteValueFloat(convert_sqlite_value_float_v7_to_v6(v)?)
		}
		v7::SqliteColumnValue::SqliteValueText(v) => {
			v6::SqliteColumnValue::SqliteValueText(convert_sqlite_value_text_v7_to_v6(v)?)
		}
		v7::SqliteColumnValue::SqliteValueBlob(v) => {
			v6::SqliteColumnValue::SqliteValueBlob(convert_sqlite_value_blob_v7_to_v6(v)?)
		}
	})
}

pub fn convert_sqlite_query_result_v7_to_v6(
	x: v7::SqliteQueryResult,
) -> Result<v6::SqliteQueryResult> {
	Ok(v6::SqliteQueryResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v7_to_v6(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_execute_result_v7_to_v6(
	x: v7::SqliteExecuteResult,
) -> Result<v6::SqliteExecuteResult> {
	Ok(v6::SqliteExecuteResult {
		columns: x.columns,
		rows: x
			.rows
			.into_iter()
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_column_value_v7_to_v6(v))
					.collect::<Result<Vec<_>>>()
			})
			.collect::<Result<Vec<_>>>()?,
		changes: x.changes,
		last_insert_row_id: x.last_insert_row_id,
	})
}

pub fn convert_sqlite_exec_request_v7_to_v6(
	x: v7::SqliteExecRequest,
) -> Result<v6::SqliteExecRequest> {
	Ok(v6::SqliteExecRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
	})
}

pub fn convert_sqlite_execute_request_v7_to_v6(
	x: v7::SqliteExecuteRequest,
) -> Result<v6::SqliteExecuteRequest> {
	Ok(v6::SqliteExecuteRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v7_to_v6(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_batch_statement_v7_to_v6(
	x: v7::SqliteBatchStatement,
) -> Result<v6::SqliteBatchStatement> {
	Ok(v6::SqliteBatchStatement {
		sql: x.sql,
		params: x
			.params
			.map(|v| {
				v.into_iter()
					.map(|v| convert_sqlite_bind_param_v7_to_v6(v))
					.collect::<Result<Vec<_>>>()
			})
			.transpose()?,
	})
}

pub fn convert_sqlite_execute_batch_request_v7_to_v6(
	x: v7::SqliteExecuteBatchRequest,
) -> Result<v6::SqliteExecuteBatchRequest> {
	Ok(v6::SqliteExecuteBatchRequest {
		namespace_id: x.namespace_id,
		actor_id: x.actor_id,
		generation: x.generation,
		statements: x
			.statements
			.into_iter()
			.map(|v| convert_sqlite_batch_statement_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_ok_v7_to_v6(x: v7::SqliteExecOk) -> Result<v6::SqliteExecOk> {
	Ok(v6::SqliteExecOk {
		result: convert_sqlite_query_result_v7_to_v6(x.result)?,
	})
}

pub fn convert_sqlite_execute_ok_v7_to_v6(x: v7::SqliteExecuteOk) -> Result<v6::SqliteExecuteOk> {
	Ok(v6::SqliteExecuteOk {
		result: convert_sqlite_execute_result_v7_to_v6(x.result)?,
	})
}

pub fn convert_sqlite_execute_batch_ok_v7_to_v6(
	x: v7::SqliteExecuteBatchOk,
) -> Result<v6::SqliteExecuteBatchOk> {
	Ok(v6::SqliteExecuteBatchOk {
		results: x
			.results
			.into_iter()
			.map(|v| convert_sqlite_execute_result_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_sqlite_exec_response_v7_to_v6(
	x: v7::SqliteExecResponse,
) -> Result<v6::SqliteExecResponse> {
	Ok(match x {
		v7::SqliteExecResponse::SqliteExecOk(v) => {
			v6::SqliteExecResponse::SqliteExecOk(convert_sqlite_exec_ok_v7_to_v6(v)?)
		}
		v7::SqliteExecResponse::SqliteErrorResponse(v) => {
			v6::SqliteExecResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v6(v)?)
		}
	})
}

pub fn convert_sqlite_execute_response_v7_to_v6(
	x: v7::SqliteExecuteResponse,
) -> Result<v6::SqliteExecuteResponse> {
	Ok(match x {
		v7::SqliteExecuteResponse::SqliteExecuteOk(v) => {
			v6::SqliteExecuteResponse::SqliteExecuteOk(convert_sqlite_execute_ok_v7_to_v6(v)?)
		}
		v7::SqliteExecuteResponse::SqliteErrorResponse(v) => {
			v6::SqliteExecuteResponse::SqliteErrorResponse(convert_sqlite_error_response_v7_to_v6(
				v,
			)?)
		}
	})
}

pub fn convert_sqlite_execute_batch_response_v7_to_v6(
	x: v7::SqliteExecuteBatchResponse,
) -> Result<v6::SqliteExecuteBatchResponse> {
	Ok(match x {
		v7::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(v) => {
			v6::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(
				convert_sqlite_execute_batch_ok_v7_to_v6(v)?,
			)
		}
		v7::SqliteExecuteBatchResponse::SqliteErrorResponse(v) => {
			v6::SqliteExecuteBatchResponse::SqliteErrorResponse(
				convert_sqlite_error_response_v7_to_v6(v)?,
			)
		}
	})
}

pub fn convert_stop_code_v7_to_v6(x: v7::StopCode) -> Result<v6::StopCode> {
	Ok(match x {
		v7::StopCode::Ok => v6::StopCode::Ok,
		v7::StopCode::Error => v6::StopCode::Error,
	})
}

pub fn convert_actor_name_v7_to_v6(x: v7::ActorName) -> Result<v6::ActorName> {
	Ok(v6::ActorName {
		metadata: x.metadata,
	})
}

pub fn convert_actor_config_v7_to_v6(x: v7::ActorConfig) -> Result<v6::ActorConfig> {
	Ok(v6::ActorConfig {
		name: x.name,
		key: x.key,
		create_ts: x.create_ts,
		input: x.input,
	})
}

pub fn convert_actor_checkpoint_v7_to_v6(x: v7::ActorCheckpoint) -> Result<v6::ActorCheckpoint> {
	Ok(v6::ActorCheckpoint {
		actor_id: x.actor_id,
		generation: x.generation,
		index: x.index,
	})
}

pub fn convert_actor_intent_v7_to_v6(x: v7::ActorIntent) -> Result<v6::ActorIntent> {
	Ok(match x {
		v7::ActorIntent::ActorIntentSleep => v6::ActorIntent::ActorIntentSleep,
		v7::ActorIntent::ActorIntentStop => v6::ActorIntent::ActorIntentStop,
	})
}

pub fn convert_actor_state_stopped_v7_to_v6(
	x: v7::ActorStateStopped,
) -> Result<v6::ActorStateStopped> {
	Ok(v6::ActorStateStopped {
		code: convert_stop_code_v7_to_v6(x.code)?,
		message: x.message,
	})
}

pub fn convert_actor_state_v7_to_v6(x: v7::ActorState) -> Result<v6::ActorState> {
	Ok(match x {
		v7::ActorState::ActorStateRunning => v6::ActorState::ActorStateRunning,
		v7::ActorState::ActorStateStopped(v) => {
			v6::ActorState::ActorStateStopped(convert_actor_state_stopped_v7_to_v6(v)?)
		}
	})
}

pub fn convert_event_actor_intent_v7_to_v6(
	x: v7::EventActorIntent,
) -> Result<v6::EventActorIntent> {
	Ok(v6::EventActorIntent {
		intent: convert_actor_intent_v7_to_v6(x.intent)?,
	})
}

pub fn convert_event_actor_state_update_v7_to_v6(
	x: v7::EventActorStateUpdate,
) -> Result<v6::EventActorStateUpdate> {
	Ok(v6::EventActorStateUpdate {
		state: convert_actor_state_v7_to_v6(x.state)?,
	})
}

pub fn convert_event_actor_set_alarm_v7_to_v6(
	x: v7::EventActorSetAlarm,
) -> Result<v6::EventActorSetAlarm> {
	Ok(v6::EventActorSetAlarm {
		alarm_ts: x.alarm_ts,
	})
}

pub fn convert_event_v7_to_v6(x: v7::Event) -> Result<v6::Event> {
	Ok(match x {
		v7::Event::EventActorIntent(v) => {
			v6::Event::EventActorIntent(convert_event_actor_intent_v7_to_v6(v)?)
		}
		v7::Event::EventActorStateUpdate(v) => {
			v6::Event::EventActorStateUpdate(convert_event_actor_state_update_v7_to_v6(v)?)
		}
		v7::Event::EventActorSetAlarm(v) => {
			v6::Event::EventActorSetAlarm(convert_event_actor_set_alarm_v7_to_v6(v)?)
		}
	})
}

pub fn convert_event_wrapper_v7_to_v6(x: v7::EventWrapper) -> Result<v6::EventWrapper> {
	Ok(v6::EventWrapper {
		checkpoint: convert_actor_checkpoint_v7_to_v6(x.checkpoint)?,
		inner: convert_event_v7_to_v6(x.inner)?,
	})
}

pub fn convert_preloaded_kv_entry_v7_to_v6(
	x: v7::PreloadedKvEntry,
) -> Result<v6::PreloadedKvEntry> {
	Ok(v6::PreloadedKvEntry {
		key: x.key,
		value: x.value,
		metadata: convert_kv_metadata_v7_to_v6(x.metadata)?,
	})
}

pub fn convert_preloaded_kv_v7_to_v6(x: v7::PreloadedKv) -> Result<v6::PreloadedKv> {
	Ok(v6::PreloadedKv {
		entries: x
			.entries
			.into_iter()
			.map(|v| convert_preloaded_kv_entry_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
		requested_get_keys: x.requested_get_keys,
		requested_prefixes: x.requested_prefixes,
	})
}

pub fn convert_hibernating_request_v7_to_v6(
	x: v7::HibernatingRequest,
) -> Result<v6::HibernatingRequest> {
	Ok(v6::HibernatingRequest {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
	})
}

pub fn convert_command_start_actor_v7_to_v6(
	x: v7::CommandStartActor,
) -> Result<v6::CommandStartActor> {
	Ok(v6::CommandStartActor {
		config: convert_actor_config_v7_to_v6(x.config)?,
		hibernating_requests: x
			.hibernating_requests
			.into_iter()
			.map(|v| convert_hibernating_request_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
		preloaded_kv: x
			.preloaded_kv
			.map(|v| convert_preloaded_kv_v7_to_v6(v))
			.transpose()?,
	})
}

pub fn convert_stop_actor_reason_v7_to_v6(x: v7::StopActorReason) -> Result<v6::StopActorReason> {
	Ok(match x {
		v7::StopActorReason::SleepIntent => v6::StopActorReason::SleepIntent,
		v7::StopActorReason::StopIntent => v6::StopActorReason::StopIntent,
		v7::StopActorReason::Destroy => v6::StopActorReason::Destroy,
		v7::StopActorReason::GoingAway => v6::StopActorReason::GoingAway,
		v7::StopActorReason::Lost => v6::StopActorReason::Lost,
	})
}

pub fn convert_command_stop_actor_v7_to_v6(
	x: v7::CommandStopActor,
) -> Result<v6::CommandStopActor> {
	Ok(v6::CommandStopActor {
		reason: convert_stop_actor_reason_v7_to_v6(x.reason)?,
	})
}

pub fn convert_command_v7_to_v6(x: v7::Command) -> Result<v6::Command> {
	Ok(match x {
		v7::Command::CommandStartActor(v) => {
			v6::Command::CommandStartActor(convert_command_start_actor_v7_to_v6(v)?)
		}
		v7::Command::CommandStopActor(v) => {
			v6::Command::CommandStopActor(convert_command_stop_actor_v7_to_v6(v)?)
		}
	})
}

pub fn convert_command_wrapper_v7_to_v6(x: v7::CommandWrapper) -> Result<v6::CommandWrapper> {
	Ok(v6::CommandWrapper {
		checkpoint: convert_actor_checkpoint_v7_to_v6(x.checkpoint)?,
		inner: convert_command_v7_to_v6(x.inner)?,
	})
}

pub fn convert_actor_command_key_data_v7_to_v6(
	x: v7::ActorCommandKeyData,
) -> Result<v6::ActorCommandKeyData> {
	Ok(match x {
		v7::ActorCommandKeyData::CommandStartActor(v) => {
			v6::ActorCommandKeyData::CommandStartActor(convert_command_start_actor_v7_to_v6(v)?)
		}
		v7::ActorCommandKeyData::CommandStopActor(v) => {
			v6::ActorCommandKeyData::CommandStopActor(convert_command_stop_actor_v7_to_v6(v)?)
		}
	})
}

pub fn convert_message_id_v7_to_v6(x: v7::MessageId) -> Result<v6::MessageId> {
	Ok(v6::MessageId {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		message_index: x.message_index,
	})
}

pub fn convert_to_envoy_request_start_v7_to_v6(
	x: v7::ToEnvoyRequestStart,
) -> Result<v6::ToEnvoyRequestStart> {
	Ok(v6::ToEnvoyRequestStart {
		actor_id: x.actor_id,
		method: x.method,
		path: x.path,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_envoy_request_chunk_v7_to_v6(
	x: v7::ToEnvoyRequestChunk,
) -> Result<v6::ToEnvoyRequestChunk> {
	Ok(v6::ToEnvoyRequestChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_to_rivet_response_start_v7_to_v6(
	x: v7::ToRivetResponseStart,
) -> Result<v6::ToRivetResponseStart> {
	Ok(v6::ToRivetResponseStart {
		status: x.status,
		headers: x.headers,
		body: x.body,
		stream: x.stream,
	})
}

pub fn convert_to_rivet_response_chunk_v7_to_v6(
	x: v7::ToRivetResponseChunk,
) -> Result<v6::ToRivetResponseChunk> {
	Ok(v6::ToRivetResponseChunk {
		body: x.body,
		finish: x.finish,
	})
}

pub fn convert_to_envoy_web_socket_open_v7_to_v6(
	x: v7::ToEnvoyWebSocketOpen,
) -> Result<v6::ToEnvoyWebSocketOpen> {
	Ok(v6::ToEnvoyWebSocketOpen {
		actor_id: x.actor_id,
		path: x.path,
		headers: x.headers,
	})
}

pub fn convert_to_envoy_web_socket_message_v7_to_v6(
	x: v7::ToEnvoyWebSocketMessage,
) -> Result<v6::ToEnvoyWebSocketMessage> {
	Ok(v6::ToEnvoyWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_envoy_web_socket_close_v7_to_v6(
	x: v7::ToEnvoyWebSocketClose,
) -> Result<v6::ToEnvoyWebSocketClose> {
	Ok(v6::ToEnvoyWebSocketClose {
		code: x.code,
		reason: x.reason,
	})
}

pub fn convert_to_rivet_web_socket_open_v7_to_v6(
	x: v7::ToRivetWebSocketOpen,
) -> Result<v6::ToRivetWebSocketOpen> {
	Ok(v6::ToRivetWebSocketOpen {
		can_hibernate: x.can_hibernate,
	})
}

pub fn convert_to_rivet_web_socket_message_v7_to_v6(
	x: v7::ToRivetWebSocketMessage,
) -> Result<v6::ToRivetWebSocketMessage> {
	Ok(v6::ToRivetWebSocketMessage {
		data: x.data,
		binary: x.binary,
	})
}

pub fn convert_to_rivet_web_socket_message_ack_v7_to_v6(
	x: v7::ToRivetWebSocketMessageAck,
) -> Result<v6::ToRivetWebSocketMessageAck> {
	Ok(v6::ToRivetWebSocketMessageAck { index: x.index })
}

pub fn convert_to_rivet_web_socket_close_v7_to_v6(
	x: v7::ToRivetWebSocketClose,
) -> Result<v6::ToRivetWebSocketClose> {
	Ok(v6::ToRivetWebSocketClose {
		code: x.code,
		reason: x.reason,
		hibernate: x.hibernate,
	})
}

pub fn convert_to_rivet_tunnel_message_kind_v7_to_v6(
	x: v7::ToRivetTunnelMessageKind,
) -> Result<v6::ToRivetTunnelMessageKind> {
	Ok(match x {
		v7::ToRivetTunnelMessageKind::ToRivetResponseStart(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetResponseStart(
				convert_to_rivet_response_start_v7_to_v6(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetResponseChunk(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				convert_to_rivet_response_chunk_v7_to_v6(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetResponseAbort(_) => {
			v6::ToRivetTunnelMessageKind::ToRivetResponseAbort
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(
				convert_to_rivet_web_socket_open_v7_to_v6(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(
				convert_to_rivet_web_socket_message_v7_to_v6(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(
				convert_to_rivet_web_socket_message_ack_v7_to_v6(v)?,
			)
		}
		v7::ToRivetTunnelMessageKind::ToRivetWebSocketClose(v) => {
			v6::ToRivetTunnelMessageKind::ToRivetWebSocketClose(
				convert_to_rivet_web_socket_close_v7_to_v6(v)?,
			)
		}
	})
}

pub fn convert_to_rivet_tunnel_message_v7_to_v6(
	x: v7::ToRivetTunnelMessage,
) -> Result<v6::ToRivetTunnelMessage> {
	Ok(v6::ToRivetTunnelMessage {
		message_id: convert_message_id_v7_to_v6(x.message_id)?,
		message_kind: convert_to_rivet_tunnel_message_kind_v7_to_v6(x.message_kind)?,
	})
}

pub fn convert_to_envoy_tunnel_message_kind_v7_to_v6(
	x: v7::ToEnvoyTunnelMessageKind,
) -> Result<v6::ToEnvoyTunnelMessageKind> {
	Ok(match x {
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(v) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				convert_to_envoy_request_start_v7_to_v6(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(v) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				convert_to_envoy_request_chunk_v7_to_v6(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(_) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(v) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				convert_to_envoy_web_socket_open_v7_to_v6(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(v) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				convert_to_envoy_web_socket_message_v7_to_v6(v)?,
			)
		}
		v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(v) => {
			v6::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
				convert_to_envoy_web_socket_close_v7_to_v6(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_tunnel_message_v7_to_v6(
	x: v7::ToEnvoyTunnelMessage,
) -> Result<v6::ToEnvoyTunnelMessage> {
	Ok(v6::ToEnvoyTunnelMessage {
		message_id: convert_message_id_v7_to_v6(x.message_id)?,
		message_kind: convert_to_envoy_tunnel_message_kind_v7_to_v6(x.message_kind)?,
	})
}

pub fn convert_to_envoy_ping_v7_to_v6(x: v7::ToEnvoyPing) -> Result<v6::ToEnvoyPing> {
	Ok(v6::ToEnvoyPing { ts: x.ts })
}

pub fn convert_to_rivet_metadata_v7_to_v6(x: v7::ToRivetMetadata) -> Result<v6::ToRivetMetadata> {
	Ok(v6::ToRivetMetadata {
		prepopulate_actor_names: x
			.prepopulate_actor_names
			.map(|v| {
				v.into_iter()
					.map(|(k, v)| -> Result<_> { Ok((k, convert_actor_name_v7_to_v6(v)?)) })
					.collect::<Result<_>>()
			})
			.transpose()?,
		metadata: x.metadata,
	})
}

pub fn convert_to_rivet_events_v7_to_v6(x: v7::ToRivetEvents) -> Result<v6::ToRivetEvents> {
	Ok(x.into_iter()
		.map(|v| convert_event_wrapper_v7_to_v6(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_rivet_ack_commands_v7_to_v6(
	x: v7::ToRivetAckCommands,
) -> Result<v6::ToRivetAckCommands> {
	Ok(v6::ToRivetAckCommands {
		last_command_checkpoints: x
			.last_command_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_rivet_pong_v7_to_v6(x: v7::ToRivetPong) -> Result<v6::ToRivetPong> {
	Ok(v6::ToRivetPong { ts: x.ts })
}

pub fn convert_to_rivet_kv_request_v7_to_v6(
	x: v7::ToRivetKvRequest,
) -> Result<v6::ToRivetKvRequest> {
	Ok(v6::ToRivetKvRequest {
		actor_id: x.actor_id,
		request_id: x.request_id,
		data: convert_kv_request_data_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_get_pages_request_v7_to_v6(
	x: v7::ToRivetSqliteGetPagesRequest,
) -> Result<v6::ToRivetSqliteGetPagesRequest> {
	Ok(v6::ToRivetSqliteGetPagesRequest {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_request_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_commit_request_v7_to_v6(
	x: v7::ToRivetSqliteCommitRequest,
) -> Result<v6::ToRivetSqliteCommitRequest> {
	Ok(v6::ToRivetSqliteCommitRequest {
		request_id: x.request_id,
		data: convert_sqlite_commit_request_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_exec_request_v7_to_v6(
	x: v7::ToRivetSqliteExecRequest,
) -> Result<v6::ToRivetSqliteExecRequest> {
	Ok(v6::ToRivetSqliteExecRequest {
		request_id: x.request_id,
		data: convert_sqlite_exec_request_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_request_v7_to_v6(
	x: v7::ToRivetSqliteExecuteRequest,
) -> Result<v6::ToRivetSqliteExecuteRequest> {
	Ok(v6::ToRivetSqliteExecuteRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_request_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_sqlite_execute_batch_request_v7_to_v6(
	x: v7::ToRivetSqliteExecuteBatchRequest,
) -> Result<v6::ToRivetSqliteExecuteBatchRequest> {
	Ok(v6::ToRivetSqliteExecuteBatchRequest {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_request_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_rivet_v7_to_v6(x: v7::ToRivet) -> Result<v6::ToRivet> {
	Ok(match x {
		v7::ToRivet::ToRivetMetadata(v) => {
			v6::ToRivet::ToRivetMetadata(convert_to_rivet_metadata_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetEvents(v) => {
			v6::ToRivet::ToRivetEvents(convert_to_rivet_events_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetAckCommands(v) => {
			v6::ToRivet::ToRivetAckCommands(convert_to_rivet_ack_commands_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetStopping => v6::ToRivet::ToRivetStopping,
		v7::ToRivet::ToRivetPong(v) => v6::ToRivet::ToRivetPong(convert_to_rivet_pong_v7_to_v6(v)?),
		v7::ToRivet::ToRivetKvRequest(v) => {
			v6::ToRivet::ToRivetKvRequest(convert_to_rivet_kv_request_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetTunnelMessage(v) => {
			v6::ToRivet::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetSqliteGetPagesRequest(v) => v6::ToRivet::ToRivetSqliteGetPagesRequest(
			convert_to_rivet_sqlite_get_pages_request_v7_to_v6(v)?,
		),
		v7::ToRivet::ToRivetSqliteCommitRequest(v) => v6::ToRivet::ToRivetSqliteCommitRequest(
			convert_to_rivet_sqlite_commit_request_v7_to_v6(v)?,
		),
		v7::ToRivet::ToRivetSqliteExecRequest(v) => {
			v6::ToRivet::ToRivetSqliteExecRequest(convert_to_rivet_sqlite_exec_request_v7_to_v6(v)?)
		}
		v7::ToRivet::ToRivetSqliteExecuteRequest(v) => v6::ToRivet::ToRivetSqliteExecuteRequest(
			convert_to_rivet_sqlite_execute_request_v7_to_v6(v)?,
		),
		v7::ToRivet::ToRivetSqliteExecuteBatchRequest(v) => {
			v6::ToRivet::ToRivetSqliteExecuteBatchRequest(
				convert_to_rivet_sqlite_execute_batch_request_v7_to_v6(v)?,
			)
		}
	})
}

pub fn convert_protocol_metadata_v7_to_v6(x: v7::ProtocolMetadata) -> Result<v6::ProtocolMetadata> {
	Ok(v6::ProtocolMetadata {
		envoy_lost_threshold: x.envoy_lost_threshold,
		actor_stop_threshold: x.actor_stop_threshold,
		max_response_payload_size: x.max_response_payload_size,
	})
}

pub fn convert_to_envoy_init_v7_to_v6(x: v7::ToEnvoyInit) -> Result<v6::ToEnvoyInit> {
	Ok(v6::ToEnvoyInit {
		metadata: convert_protocol_metadata_v7_to_v6(x.metadata)?,
	})
}

pub fn convert_to_envoy_commands_v7_to_v6(x: v7::ToEnvoyCommands) -> Result<v6::ToEnvoyCommands> {
	Ok(x.into_iter()
		.map(|v| convert_command_wrapper_v7_to_v6(v))
		.collect::<Result<Vec<_>>>()?)
}

pub fn convert_to_envoy_ack_events_v7_to_v6(
	x: v7::ToEnvoyAckEvents,
) -> Result<v6::ToEnvoyAckEvents> {
	Ok(v6::ToEnvoyAckEvents {
		last_event_checkpoints: x
			.last_event_checkpoints
			.into_iter()
			.map(|v| convert_actor_checkpoint_v7_to_v6(v))
			.collect::<Result<Vec<_>>>()?,
	})
}

pub fn convert_to_envoy_kv_response_v7_to_v6(
	x: v7::ToEnvoyKvResponse,
) -> Result<v6::ToEnvoyKvResponse> {
	Ok(v6::ToEnvoyKvResponse {
		request_id: x.request_id,
		data: convert_kv_response_data_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_get_pages_response_v7_to_v6(
	x: v7::ToEnvoySqliteGetPagesResponse,
) -> Result<v6::ToEnvoySqliteGetPagesResponse> {
	Ok(v6::ToEnvoySqliteGetPagesResponse {
		request_id: x.request_id,
		data: convert_sqlite_get_pages_response_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_commit_response_v7_to_v6(
	x: v7::ToEnvoySqliteCommitResponse,
) -> Result<v6::ToEnvoySqliteCommitResponse> {
	Ok(v6::ToEnvoySqliteCommitResponse {
		request_id: x.request_id,
		data: convert_sqlite_commit_response_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_exec_response_v7_to_v6(
	x: v7::ToEnvoySqliteExecResponse,
) -> Result<v6::ToEnvoySqliteExecResponse> {
	Ok(v6::ToEnvoySqliteExecResponse {
		request_id: x.request_id,
		data: convert_sqlite_exec_response_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_response_v7_to_v6(
	x: v7::ToEnvoySqliteExecuteResponse,
) -> Result<v6::ToEnvoySqliteExecuteResponse> {
	Ok(v6::ToEnvoySqliteExecuteResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_response_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_sqlite_execute_batch_response_v7_to_v6(
	x: v7::ToEnvoySqliteExecuteBatchResponse,
) -> Result<v6::ToEnvoySqliteExecuteBatchResponse> {
	Ok(v6::ToEnvoySqliteExecuteBatchResponse {
		request_id: x.request_id,
		data: convert_sqlite_execute_batch_response_v7_to_v6(x.data)?,
	})
}

pub fn convert_to_envoy_v7_to_v6(x: v7::ToEnvoy) -> Result<v6::ToEnvoy> {
	Ok(match x {
		v7::ToEnvoy::ToEnvoyInit(v) => v6::ToEnvoy::ToEnvoyInit(convert_to_envoy_init_v7_to_v6(v)?),
		v7::ToEnvoy::ToEnvoyCommands(v) => {
			v6::ToEnvoy::ToEnvoyCommands(convert_to_envoy_commands_v7_to_v6(v)?)
		}
		v7::ToEnvoy::ToEnvoyAckEvents(v) => {
			v6::ToEnvoy::ToEnvoyAckEvents(convert_to_envoy_ack_events_v7_to_v6(v)?)
		}
		v7::ToEnvoy::ToEnvoyKvResponse(v) => {
			v6::ToEnvoy::ToEnvoyKvResponse(convert_to_envoy_kv_response_v7_to_v6(v)?)
		}
		v7::ToEnvoy::ToEnvoyTunnelMessage(v) => {
			v6::ToEnvoy::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v7_to_v6(v)?)
		}
		v7::ToEnvoy::ToEnvoyPing(v) => v6::ToEnvoy::ToEnvoyPing(convert_to_envoy_ping_v7_to_v6(v)?),
		v7::ToEnvoy::ToEnvoySqliteGetPagesResponse(v) => {
			v6::ToEnvoy::ToEnvoySqliteGetPagesResponse(
				convert_to_envoy_sqlite_get_pages_response_v7_to_v6(v)?,
			)
		}
		v7::ToEnvoy::ToEnvoySqliteCommitResponse(v) => v6::ToEnvoy::ToEnvoySqliteCommitResponse(
			convert_to_envoy_sqlite_commit_response_v7_to_v6(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecResponse(v) => v6::ToEnvoy::ToEnvoySqliteExecResponse(
			convert_to_envoy_sqlite_exec_response_v7_to_v6(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecuteResponse(v) => v6::ToEnvoy::ToEnvoySqliteExecuteResponse(
			convert_to_envoy_sqlite_execute_response_v7_to_v6(v)?,
		),
		v7::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(v) => {
			v6::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(
				convert_to_envoy_sqlite_execute_batch_response_v7_to_v6(v)?,
			)
		}
	})
}

pub fn convert_to_envoy_conn_ping_v7_to_v6(x: v7::ToEnvoyConnPing) -> Result<v6::ToEnvoyConnPing> {
	Ok(v6::ToEnvoyConnPing {
		gateway_id: x.gateway_id,
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_envoy_conn_v7_to_v6(x: v7::ToEnvoyConn) -> Result<v6::ToEnvoyConn> {
	Ok(match x {
		v7::ToEnvoyConn::ToEnvoyConnPing(v) => {
			v6::ToEnvoyConn::ToEnvoyConnPing(convert_to_envoy_conn_ping_v7_to_v6(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyConnClose => v6::ToEnvoyConn::ToEnvoyConnClose,
		v7::ToEnvoyConn::ToEnvoyCommands(v) => {
			v6::ToEnvoyConn::ToEnvoyCommands(convert_to_envoy_commands_v7_to_v6(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyAckEvents(v) => {
			v6::ToEnvoyConn::ToEnvoyAckEvents(convert_to_envoy_ack_events_v7_to_v6(v)?)
		}
		v7::ToEnvoyConn::ToEnvoyTunnelMessage(v) => {
			v6::ToEnvoyConn::ToEnvoyTunnelMessage(convert_to_envoy_tunnel_message_v7_to_v6(v)?)
		}
	})
}

pub fn convert_to_gateway_pong_v7_to_v6(x: v7::ToGatewayPong) -> Result<v6::ToGatewayPong> {
	Ok(v6::ToGatewayPong {
		request_id: x.request_id,
		ts: x.ts,
	})
}

pub fn convert_to_gateway_v7_to_v6(x: v7::ToGateway) -> Result<v6::ToGateway> {
	Ok(match x {
		v7::ToGateway::ToGatewayPong(v) => {
			v6::ToGateway::ToGatewayPong(convert_to_gateway_pong_v7_to_v6(v)?)
		}
		v7::ToGateway::ToRivetTunnelMessage(v) => {
			v6::ToGateway::ToRivetTunnelMessage(convert_to_rivet_tunnel_message_v7_to_v6(v)?)
		}
	})
}

pub fn convert_to_outbound_actor_start_v7_to_v6(
	x: v7::ToOutboundActorStart,
) -> Result<v6::ToOutboundActorStart> {
	Ok(v6::ToOutboundActorStart {
		namespace_id: x.namespace_id,
		pool_name: x.pool_name,
		checkpoint: convert_actor_checkpoint_v7_to_v6(x.checkpoint)?,
		actor_config: convert_actor_config_v7_to_v6(x.actor_config)?,
	})
}

pub fn convert_to_outbound_v7_to_v6(x: v7::ToOutbound) -> Result<v6::ToOutbound> {
	Ok(match x {
		v7::ToOutbound::ToOutboundActorStart(v) => {
			v6::ToOutbound::ToOutboundActorStart(convert_to_outbound_actor_start_v7_to_v6(v)?)
		}
	})
}
