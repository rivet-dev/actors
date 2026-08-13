use std::{fmt::Display, marker::PhantomData, sync::Arc, time::Instant};

use anyhow::Result;
use futures_util::StreamExt;
use rivet_util::Id;
use serde::Serialize;
use tracing::Instrument;

use crate::{
	builder::{BuilderError, WorkflowRepr},
	ctx::WorkflowCtx,
	db::BumpSubSubject,
	error::{WorkflowError, WorkflowResult},
	history::cursor::HistoryResult,
	metrics,
	workflow::{Workflow, WorkflowInput},
};

pub struct SubWorkflowBuilder<'a, T, I: WorkflowInput> {
	ctx: &'a mut WorkflowCtx,
	version: usize,

	repr: T,
	tags: serde_json::Map<String, serde_json::Value>,
	unique: bool,
	error: Option<BuilderError>,
	_marker: PhantomData<I>,
}

impl<'a, T, I> SubWorkflowBuilder<'a, T, I>
where
	T: WorkflowRepr<I>,
	I: WorkflowInput,
	<I as WorkflowInput>::Workflow: Workflow<Input = I>,
{
	pub(crate) fn new(ctx: &'a mut WorkflowCtx, version: usize, repr: T) -> Self {
		SubWorkflowBuilder {
			ctx,
			version,

			repr,
			tags: serde_json::Map::new(),
			unique: false,
			error: None,
			_marker: PhantomData,
		}
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

	/// Does not dispatch a workflow if one already exists with the given name and tags. Has no effect if no
	/// tags are provided (will always spawn a new workflow).
	pub fn unique(mut self) -> Self {
		if self.error.is_some() {
			return self;
		}

		self.unique = true;

		self
	}

	#[tracing::instrument(skip_all)]
	pub async fn dispatch(self) -> Result<Id> {
		self.ctx.check_stop()?;

		if let Some(err) = self.error {
			return Err(err.into());
		}

		let tags = if self.tags.is_empty() {
			None
		} else {
			Some(serde_json::Value::Object(self.tags))
		};

		// Error for version mismatch. This is done in the builder instead of in `VersionedWorkflowCtx` to
		// defer the error.
		self.ctx.compare_version("sub workflow", self.version)?;

		Self::dispatch_workflow_inner(
			self.ctx,
			self.version,
			self.repr.as_input()?,
			tags,
			self.unique,
		)
		.in_current_span()
		.await
		.map_err(Into::into)
	}

	// This doesn't have a self parameter because self.tags was already moved (see above)
	#[tracing::instrument(skip_all, fields(sub_workflow_name=I::Workflow::NAME, sub_workflow_id, unique))]
	async fn dispatch_workflow_inner(
		ctx: &mut WorkflowCtx,
		version: usize,
		input: &I,
		tags: Option<serde_json::Value>,
		unique: bool,
	) -> WorkflowResult<Id>
	where
		I: WorkflowInput,
		<I as WorkflowInput>::Workflow: Workflow<Input = I>,
	{
		let history_res = ctx
			.cursor()
			.compare_sub_workflow(version, I::Workflow::NAME)?;
		let location = ctx.cursor().current_location_for(&history_res);

		// Sub workflow dispatched before
		let id = if let HistoryResult::Event(sub_workflow) = history_res {
			tracing::debug!("replaying workflow dispatch");

			sub_workflow.sub_workflow_id
		}
		// Dispatch new workflow
		else {
			let sub_workflow_name = I::Workflow::NAME;
			let sub_workflow_id = Id::new_v1(ctx.config().dc_label());
			let start_instant = Instant::now();

			if unique {
				tracing::debug!(?tags, ?input, "dispatching unique sub workflow");
			} else {
				tracing::debug!(?tags, ?input, "dispatching sub workflow");
			}

			// Serialize input
			let input_val = rivet_util::serde::json_to_raw_value!(input)
				.map_err(WorkflowError::SerializeWorkflowOutput)?;

			let actual_sub_workflow_id = ctx
				.db()
				.dispatch_sub_workflow(
					ctx.ray_id(),
					ctx.workflow_id(),
					&location,
					version,
					sub_workflow_id,
					sub_workflow_name,
					tags.as_ref(),
					&input_val,
					ctx.loop_location(),
					unique,
				)
				.await?;

			if unique {
				if sub_workflow_id == actual_sub_workflow_id {
					tracing::debug!(?tags, "dispatched unique sub workflow");
				} else {
					tracing::debug!(?tags, "unique sub workflow already exists");
				}
			}

			if sub_workflow_id == actual_sub_workflow_id {
				let dt = start_instant.elapsed().as_secs_f64();
				metrics::WORKFLOW_DISPATCH_DURATION
					.with_label_values(&[ctx.name(), sub_workflow_name])
					.observe(dt);
				metrics::WORKFLOW_DISPATCHED
					.with_label_values(&[ctx.name(), sub_workflow_name])
					.inc();
			}

			actual_sub_workflow_id
		};

		tracing::Span::current().record("sub_workflow_id", id.to_string());

		// Move to next event
		ctx.cursor_mut().update(&location);

		Ok(id)
	}

	#[tracing::instrument(name="sub_workflow", skip_all, fields(sub_workflow_name=I::Workflow::NAME))]
	pub async fn output(self) -> Result<<<I as WorkflowInput>::Workflow as Workflow>::Output> {
		self.ctx.check_stop()?;

		if let Some(err) = self.error {
			return Err(err.into());
		}

		if !self.tags.is_empty() {
			return Err(
				BuilderError::TagsOnSubWorkflowOutputNotSupported(I::Workflow::NAME).into(),
			);
		}

		if let Ok(workflow_id) = self.repr.as_workflow_id() {
			return self.wait_for_workflow(workflow_id).await;
		}

		let input = self.repr.as_input()?;

		tracing::debug!("running sub workflow");

		// Err for version mismatch
		self.ctx.compare_version("sub workflow", self.version)?;

		let input_val = rivet_util::serde::json_to_raw_value!(&input)
			.map_err(WorkflowError::SerializeWorkflowInput)?;
		let mut branch = self
			.ctx
			.custom_branch(Arc::from(input_val), self.version)
			.await?;

		// Run workflow
		let output = <<I as WorkflowInput>::Workflow as Workflow>::run(&mut branch, &input).await?;

		// Validate no leftover events
		branch.cursor().check_clear()?;

		// Move to next event
		self.ctx.cursor_mut().update(branch.cursor().root());

		Ok(output)
	}

	/// Wait for another workflow's response. If no response was found after polling the database, this
	/// workflow will go to sleep until the sub workflow completes.
	#[tracing::instrument(skip_all, fields(sub_workflow_name=I::Workflow::NAME))]
	async fn wait_for_workflow(
		&self,
		sub_workflow_id: Id,
	) -> Result<<<I as WorkflowInput>::Workflow as Workflow>::Output> {
		self.ctx.check_stop()?;

		tracing::debug!("waiting for sub workflow");

		let mut bump_sub = self
			.ctx
			.db()
			.bump_sub(BumpSubSubject::WorkflowComplete {
				workflow_id: sub_workflow_id,
			})
			.await?;
		let mut retries = self.ctx.db().max_sub_workflow_poll_retries();
		let mut interval = tokio::time::interval(self.ctx.db().sub_workflow_poll_interval());
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

		// Skip first tick, we wait after the db call instead of before
		interval.tick().await;

		loop {
			// Check if workflow completed
			let workflow = self
				.ctx
				.db()
				.get_sub_workflow(self.ctx.workflow_id(), &self.ctx.name(), sub_workflow_id)
				.await?
				.ok_or(WorkflowError::WorkflowNotFound)?;

			if let Some(output) = workflow.parse_output::<<I as WorkflowInput>::Workflow>()? {
				return Ok(output);
			} else {
				if retries == 0 {
					return Err(WorkflowError::SubWorkflowIncomplete(sub_workflow_id).into());
				}
				retries -= 1;
			}

			// Poll and wait for a wake at the same time
			tokio::select! {
				_ = bump_sub.next() => {},
				_ = interval.tick() => {},
			}
		}
	}

	// TODO: Currently not supported in workflows because it is not idempotent. Requires a history step
	// #[tracing::instrument(skip_all, fields(sub_workflow_name=I::Workflow::NAME))]
	// pub async fn get(self) -> Result<Option<WorkflowData>> {
	// 	let db = self.db.clone();
	// 	let workflow_id = self.repr.as_workflow_id()?;

	// 	db.get_workflow(workflow_id).await
	// }
}
