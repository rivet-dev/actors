use std::time::Instant;

use anyhow::Result;

use crate::{
	builder::BuilderError, ctx::WorkflowCtx, error::WorkflowError, history::cursor::HistoryResult,
	message::Message, metrics, utils::topic::AsTopic,
};

pub struct MessageBuilder<'a, M: Message> {
	ctx: &'a mut WorkflowCtx,
	version: usize,

	body: M,
	topic: Option<String>,
	wait: bool,
	error: Option<BuilderError>,
}

impl<'a, M: Message> MessageBuilder<'a, M> {
	pub(crate) fn new(ctx: &'a mut WorkflowCtx, version: usize, body: M) -> Self {
		MessageBuilder {
			ctx,
			version,

			body,
			topic: None,
			wait: false,
			error: None,
		}
	}

	pub fn topic(mut self, topic: impl AsTopic) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.topic = Some(topic.as_topic());

		self
	}

	pub fn wait(mut self) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.wait = true;

		self
	}

	#[tracing::instrument(skip_all, fields(message_name=M::NAME))]
	pub async fn send(self) -> Result<()> {
		self.ctx.check_stop()?;

		if let Some(err) = self.error {
			return Err(err.into());
		}

		// Error for version mismatch. This is done in the builder instead of in `VersionedWorkflowCtx` to
		// defer the error.
		self.ctx.compare_version("message", self.version)?;

		let history_res = self.ctx.cursor().compare_msg(self.version, M::NAME)?;
		let location = self.ctx.cursor().current_location_for(&history_res);

		// Message sent before
		if let HistoryResult::Event(_) = history_res {
			tracing::debug!("replaying message dispatch");
		}
		// Send message
		else {
			tracing::debug!(topic=?self.topic, "dispatching message");

			let start_instant = Instant::now();

			// Serialize body
			let body_val = rivet_util::serde::json_to_raw_value!(&self.body)
				.map_err(WorkflowError::SerializeMessageBody)?;
			let topic = self.topic.unwrap_or_else(|| "*".to_string());
			let tags = serde_json::Value::Object(
				[(
					"topic".to_string(),
					serde_json::Value::String(topic.clone()),
				)]
				.into_iter()
				.collect(),
			);

			self.ctx
				.db()
				.commit_workflow_message_send_event(
					self.ctx.workflow_id(),
					&location,
					self.version,
					&tags,
					M::NAME,
					&body_val,
					self.ctx.loop_location(),
				)
				.await?;

			if self.wait {
				self.ctx.msg_ctx().message_wait(&topic, self.body).await?;
			} else {
				self.ctx.msg_ctx().message(&topic, self.body).await?;
			}

			let dt = start_instant.elapsed().as_secs_f64();
			metrics::MESSAGE_SEND_DURATION
				.with_label_values(&[self.ctx.name(), M::NAME])
				.observe(dt);
			metrics::MESSAGE_PUBLISHED
				.with_label_values(&[self.ctx.name(), M::NAME])
				.inc();
		}

		// Move to next event
		self.ctx.cursor_mut().update(&location);

		Ok(())
	}
}
