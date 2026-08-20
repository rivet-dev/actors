use std::io::Cursor;

use anyhow::{Context, Result};
use rivetkit_core::{
	ActorConfig, ActorEvent, ActorFactory, CoreRegistry, RequestSaveOpts, SerializeStateReason,
	StateDelta,
};
use serde_json::{Value as JsonValue, json};

use crate::common::ctx::IntegrationCtx;

const ACTOR_NAME: &str = "counter";

#[tokio::test(flavor = "multi_thread")]
async fn counter_actor_handles_actions_through_engine() -> Result<()> {
	let ctx = IntegrationCtx::builder().start().await?;
	ctx.create_default_namespace().await?;
	let mut registry = CoreRegistry::new();
	registry.register(ACTOR_NAME, counter_factory());
	let registry_task = ctx.serve_registry(registry);

	ctx.wait_for_envoy_ready().await?;
	let actor = ctx.create_actor(ACTOR_NAME).await?;

	let first = ctx
		.wait_for_json_action(&actor.actor_id, "increment")
		.await
		.context("first increment")?;
	let second = ctx
		.wait_for_json_action(&actor.actor_id, "increment")
		.await
		.context("second increment")?;
	let current = ctx
		.wait_for_json_action(&actor.actor_id, "get")
		.await
		.context("get count")?;

	assert_eq!(action_output(&first)?, json!(1));
	assert_eq!(action_output(&second)?, json!(2));
	assert_eq!(action_output(&current)?, json!(2));

	registry_task.shutdown().await?;
	ctx.shutdown().await?;

	Ok(())
}

fn counter_factory() -> ActorFactory {
	let config = ActorConfig {
		remote_sqlite: true,
		..ActorConfig::default()
	};
	ActorFactory::new(config, |start| {
		Box::pin(async move {
			let ctx = start.ctx;
			let mut count = read_count(&ctx.state());
			let mut events = start.events;
			while let Some(event) = events.recv().await {
				match event {
					ActorEvent::Action {
						name,
						args: _,
						conn: _,
						reply,
						..
					} => match name.as_str() {
						"increment" => {
							count += 1;
							ctx.request_save(RequestSaveOpts::default());
							reply.send(Ok(encode_json(&json!(count))));
						}
						"get" => {
							reply.send(Ok(encode_json(&json!(count))));
						}
						name => {
							reply.send(Err(anyhow::anyhow!("unknown action `{name}`")));
						}
					},
					ActorEvent::SerializeState { reason, reply } => match reason {
						SerializeStateReason::Save | SerializeStateReason::Inspector => {
							reply.send(Ok(vec![StateDelta::ActorState(encode_json(&json!({
								"count": count,
							})))]));
						}
					},
					ActorEvent::RunGracefulCleanup { reason: _, reply } => {
						reply.send(Ok(()));
					}
					ActorEvent::HttpRequest { request: _, reply } => {
						reply.send(Err(anyhow::anyhow!("http requests are not handled")));
					}
					ActorEvent::QueueSend {
						name: _,
						body: _,
						conn: _,
						request: _,
						wait: _,
						timeout_ms: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("queue sends are not handled")));
					}
					ActorEvent::WebSocketOpen {
						ws: _,
						conn: _,
						request: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("websockets are not handled")));
					}
					ActorEvent::ConnectionPreflight {
						conn: _,
						params: _,
						request: _,
						reply,
					} => {
						reply.send(Ok(()));
					}
					ActorEvent::ConnectionOpen { reply, .. } => {
						reply.send(Ok(()));
					}
					ActorEvent::ConnectionClosed { conn: _ } => {}
					ActorEvent::SubscribeRequest {
						conn: _,
						event_name: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("subscriptions are not handled")));
					}
					ActorEvent::DisconnectConn { conn_id: _, reply } => {
						reply.send(Ok(()));
					}
					ActorEvent::WorkflowHistoryRequested { reply } => {
						reply.send(Ok(None));
					}
					ActorEvent::WorkflowReplayRequested { entry_id: _, reply } => {
						reply.send(Ok(None));
					}
					ActorEvent::RunWake { .. } => {}
				}
			}

			Ok(())
		})
	})
}

fn action_output(body: &str) -> Result<JsonValue> {
	let value: JsonValue = serde_json::from_str(body).context("decode action response")?;
	Ok(value.get("output").cloned().unwrap_or(JsonValue::Null))
}

fn read_count(state: &[u8]) -> i64 {
	if state.is_empty() {
		return 0;
	}

	let value: JsonValue = ciborium::from_reader(Cursor::new(state)).unwrap_or(JsonValue::Null);
	value
		.get("count")
		.and_then(JsonValue::as_i64)
		.unwrap_or_default()
}

fn encode_json(value: &JsonValue) -> Vec<u8> {
	let mut out = Vec::new();
	ciborium::into_writer(value, &mut out).expect("encode cbor json");
	out
}
