use super::*;
use crate::error::ActorLifecycle as ActorLifecycleError;
use crate::time;

pub(super) async fn dispatch_action_through_task(
	dispatch: &mpsc::UnboundedSender<DispatchCommand>,
	conn: ConnHandle,
	name: String,
	args: Vec<u8>,
	incoming: crate::telemetry::IncomingInvocationContext,
) -> std::result::Result<Vec<u8>, ActionDispatchError> {
	let (reply_tx, reply_rx) = oneshot::channel();
	try_send_dispatch_command(
		dispatch,
		DispatchCommand::Action {
			name,
			args,
			incoming,
			conn,
			reply: reply_tx,
		},
	)
	.map_err(ActionDispatchError::from_anyhow)?;
	reply_rx
		.await
		.map_err(|_| ActionDispatchError::from_anyhow(ActorLifecycleError::DroppedReply.build()))?
		.map_err(ActionDispatchError::from_anyhow)
}

pub(super) async fn with_action_dispatch_timeout<T, F>(
	duration: std::time::Duration,
	future: F,
) -> std::result::Result<T, ActionDispatchError>
where
	F: std::future::Future<Output = std::result::Result<T, ActionDispatchError>>,
{
	time::timeout(duration, future)
		.await
		.map_err(|_| ActionDispatchError::from_anyhow(ActionTimedOut.build()))?
}

pub(super) async fn with_framework_action_timeout<T, F>(
	duration: std::time::Duration,
	future: F,
) -> Result<T>
where
	F: std::future::Future<Output = Result<T>>,
{
	time::timeout(duration, future)
		.await
		.map_err(|_| ActionTimedOut.build())?
}

pub(super) async fn dispatch_websocket_open_through_task(
	dispatch: &mpsc::UnboundedSender<DispatchCommand>,
	conn: ConnHandle,
	ws: WebSocket,
	request: Option<Request>,
) -> Result<()> {
	let (reply_tx, reply_rx) = oneshot::channel();
	try_send_dispatch_command(
		dispatch,
		DispatchCommand::OpenWebSocket {
			conn,
			ws,
			request,
			reply: reply_tx,
		},
	)
	.context("actor task stopped before websocket dispatch command could be sent")?;

	reply_rx
		.await
		.context("actor task stopped before websocket dispatch reply was sent")?
}

pub(super) async fn dispatch_workflow_history_through_task(
	dispatch: &mpsc::UnboundedSender<DispatchCommand>,
) -> Result<Option<Vec<u8>>> {
	let (reply_tx, reply_rx) = oneshot::channel();
	try_send_dispatch_command(
		dispatch,
		DispatchCommand::WorkflowHistory { reply: reply_tx },
	)
	.context("actor task stopped before workflow history dispatch command could be sent")?;

	reply_rx
		.await
		.context("actor task stopped before workflow history dispatch reply was sent")?
}

pub(super) async fn dispatch_workflow_replay_request_through_task(
	dispatch: &mpsc::UnboundedSender<DispatchCommand>,
	entry_id: Option<String>,
) -> Result<Option<Vec<u8>>> {
	let (reply_tx, reply_rx) = oneshot::channel();
	try_send_dispatch_command(
		dispatch,
		DispatchCommand::WorkflowReplay {
			entry_id,
			reply: reply_tx,
		},
	)
	.context("actor task stopped before workflow replay dispatch command could be sent")?;

	reply_rx
		.await
		.context("actor task stopped before workflow replay dispatch reply was sent")?
}

pub(super) fn workflow_dispatch_result(
	result: Result<Option<Vec<u8>>>,
) -> Result<(bool, Option<Vec<u8>>)> {
	match result {
		Ok(history) => Ok((true, history)),
		Err(error) if is_dropped_reply_error(&error) => Ok((false, None)),
		Err(error) => Err(error),
	}
}

pub(super) fn is_dropped_reply_error(error: &anyhow::Error) -> bool {
	let error = RivetError::extract(error);
	error.group() == "actor" && error.code() == "dropped_reply"
}

pub(super) async fn dispatch_subscribe_request(
	ctx: &ActorContext,
	conn: ConnHandle,
	event_name: String,
) -> Result<()> {
	let (reply_tx, reply_rx) = oneshot::channel();
	ctx.try_send_actor_event(
		ActorEvent::SubscribeRequest {
			conn,
			event_name,
			reply: Reply::from(reply_tx),
		},
		"subscribe_request",
	)?;
	reply_rx
		.await
		.context("actor task stopped before subscribe dispatch reply was sent")?
}
