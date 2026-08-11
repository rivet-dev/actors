use std::{fmt::Display, time::Instant};

use anyhow::Result;
use rivet_util::Id;
use serde::Serialize;

use crate::{
	builder::BuilderError, db::DatabaseHandle, error::WorkflowError, metrics, signal::Signal,
	workflow::Workflow,
};

pub struct SignalBuilder<T: Signal + Serialize> {
	db: DatabaseHandle,
	config: rivet_config::Config,
	ray_id: Id,
	body: T,
	to_workflow_name: Option<&'static str>,
	to_workflow_id: Option<Id>,
	tags: serde_json::Map<String, serde_json::Value>,
	graceful_not_found: bool,
	error: Option<BuilderError>,
}

impl<T: Signal + Serialize> SignalBuilder<T> {
	pub(crate) fn new(
		db: DatabaseHandle,
		config: rivet_config::Config,
		ray_id: Id,
		body: T,
		from_workflow: bool,
	) -> Self {
		SignalBuilder {
			config,
			db,
			ray_id,
			body,
			to_workflow_name: None,
			to_workflow_id: None,
			tags: serde_json::Map::new(),
			graceful_not_found: false,
			error: from_workflow.then_some(BuilderError::CannotDispatchFromOpInWorkflow),
		}
	}

	// TODO: Get rid of this (RVT-5281)
	// NOTE: This is a bad implementation because it disregards other errors that may have happened earlier
	#[allow(non_snake_case)]
	pub fn bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING(mut self) -> Self {
		if let Some(BuilderError::CannotDispatchFromOpInWorkflow) = &self.error {
			self.error = None;
		}

		self
	}

	pub fn to_workflow_id(mut self, workflow_id: Id) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.to_workflow_id = Some(workflow_id);

		self
	}

	pub fn to_workflow<W: Workflow>(mut self) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.to_workflow_name = Some(W::NAME);

		self
	}

	pub fn tags(mut self, tags: serde_json::Value) -> Self {
		if self.error.is_some() {
			return self;
		}

		match tags {
			serde_json::Value::Object(map) => {
				self.tags.extend(map);
			}
			_ => self.error = Some(BuilderError::TagsNotMap),
		}

		self
	}

	pub fn tag(mut self, k: impl Display, v: impl Serialize) -> Self {
		if self.error.is_some() {
			return self;
		}

		match serde_json::to_value(&v) {
			Ok(v) => {
				self.tags.insert(k.to_string(), v);
			}
			Err(err) => self.error = Some(err.into()),
		}

		self
	}

	/// Does not throw an error when the signal target is not found and instead returns `Ok(None)`.
	pub fn graceful_not_found(mut self) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.graceful_not_found = true;

		self
	}

	/// Returns the signal id that was just sent. Unless `graceful_not_found` is set and the workflow does not
	/// exist, will always return `Some`.
	#[tracing::instrument(skip_all, fields(signal_name=T::NAME, signal_id))]
	pub async fn send(self) -> Result<Option<Id>> {
		if let Some(err) = self.error {
			return Err(err.into());
		}

		let signal_id = Id::new_v1(self.config.dc_label());
		let start_instant = Instant::now();
		let db_write_duration;

		tracing::Span::current().record("signal_id", signal_id.to_string());

		// Serialize input
		let input_val = rivet_util::serde::json_to_raw_value!(&self.body)
			.map_err(WorkflowError::SerializeSignalBody)?;

		match (
			self.to_workflow_name,
			self.to_workflow_id,
			self.tags.is_empty(),
		) {
			(Some(workflow_name), None, _) => {
				tracing::debug!(
					to_workflow_name=%workflow_name,
					tags=?self.tags,
					"dispatching signal via workflow name and tags"
				);

				let workflow_id = self
					.db
					.find_workflow(workflow_name, &serde_json::Value::Object(self.tags))
					.await?;

				let Some(workflow_id) = workflow_id else {
					// Handle signal target not found gracefully
					if self.graceful_not_found {
						tracing::debug!("signal target not found");

						return Ok(None);
					} else {
						return Err(WorkflowError::WorkflowNotFound.into());
					}
				};

				let db_write_started = Instant::now();
				self.db
					.publish_signal(self.ray_id, workflow_id, signal_id, T::NAME, &input_val)
					.await?;
				db_write_duration = db_write_started.elapsed();
			}
			(None, Some(workflow_id), true) => {
				tracing::debug!(to_workflow_id=%workflow_id, "dispatching signal via workflow id");

				let db_write_started = Instant::now();
				self.db
					.publish_signal(self.ray_id, workflow_id, signal_id, T::NAME, &input_val)
					.await?;
				db_write_duration = db_write_started.elapsed();
			}
			(None, None, false) => {
				return Err(BuilderError::InvalidSignalSend(
					"must provide workflow when using tags",
				)
				.into());
			}
			(Some(_), Some(_), _) => {
				return Err(BuilderError::InvalidSignalSend(
					"cannot provide both workflow and workflow id",
				)
				.into());
			}
			(None, Some(_), false) => {
				return Err(BuilderError::InvalidSignalSend(
					"cannot provide tags if providing a workflow id",
				)
				.into());
			}
			(None, None, true) => {
				return Err(BuilderError::InvalidSignalSend(
					"no workflow, workflow id, or tags provided",
				)
				.into());
			}
		}

		let dt = start_instant.elapsed().as_secs_f64();
		metrics::SIGNAL_SEND_DURATION
			.with_label_values(&["", T::NAME])
			.observe(dt);
		metrics::SIGNAL_SEND_DURATION_SECONDS
			.with_label_values(&["", T::NAME])
			.observe(db_write_duration.as_secs_f64());
		metrics::SIGNAL_PUBLISHED
			.with_label_values(&["", T::NAME])
			.inc();

		Ok(Some(signal_id))
	}
}
