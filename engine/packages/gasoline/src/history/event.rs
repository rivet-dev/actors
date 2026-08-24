use std::ops::Deref;

use rivet_util::Id;
use serde::de::DeserializeOwned;
use strum::FromRepr;

use super::location::Coordinate;
use crate::error::{WorkflowError, WorkflowResult};

/// An event that happened in the workflow run.
///
/// This is used to replay events.
#[derive(Debug)]
pub struct Event {
	/// Position within the root location.
	pub(crate) coordinate: Coordinate,
	pub(crate) version: usize,
	pub(crate) data: EventData,
}

impl Event {
	pub fn coordinate(&self) -> &Coordinate {
		&self.coordinate
	}
}

impl std::fmt::Display for Event {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{} v{} @ {}", self.data, self.version, self.coordinate)
	}
}

impl Deref for Event {
	type Target = EventData;

	fn deref(&self) -> &Self::Target {
		&self.data
	}
}

#[derive(Debug)]
pub enum EventData {
	Activity(ActivityEvent),
	SignalSend(SignalSendEvent),
	MessageSend(MessageSendEvent),
	SubWorkflow(SubWorkflowEvent),
	Loop(LoopEvent),
	Sleep(SleepEvent),
	Removed(RemovedEvent),
	VersionCheck(VersionCheckEvent),
	Branch,
	Signals(SignalsEvent),
}

impl std::fmt::Display for EventData {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self {
			EventData::Activity(activity) => write!(f, "activity {:?}", activity.name),
			EventData::SignalSend(signal_send) => write!(f, "signal send {:?}", signal_send.name),
			EventData::MessageSend(message_send) => {
				write!(f, "message send {:?}", message_send.name)
			}
			EventData::SubWorkflow(sub_workflow) => {
				write!(f, "sub workflow {:?}", sub_workflow.name)
			}
			EventData::Loop(_) => write!(f, "loop"),
			EventData::Sleep(_) => write!(f, "sleep"),
			EventData::Removed(removed) => {
				if let Some(name) = &removed.name {
					write!(f, "removed {} {name:?}", removed.event_type)
				} else {
					write!(f, "removed {}", removed.event_type)
				}
			}
			EventData::VersionCheck(_) => write!(f, "version check"),
			EventData::Branch => write!(f, "branch"),
			EventData::Signals(signals) => {
				let mut unique_names = signals.names.clone();
				unique_names.sort();
				unique_names.dedup();

				write!(f, "signals {:?}", unique_names.join(", "))
			}
		}
	}
}

#[derive(Hash, Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum EventType {
	Activity = 0,
	/// Deprecated.
	Signal = 1,
	SignalSend = 2,
	MessageSend = 3,
	SubWorkflow = 4,
	Loop = 5,
	Sleep = 6,
	Branch = 7,
	Removed = 8,
	VersionCheck = 9,
	Signals = 10,
}

impl std::fmt::Display for EventType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			EventType::Activity => write!(f, "activity"),
			EventType::Signal => write!(f, "signal"),
			EventType::SignalSend => write!(f, "signal send"),
			EventType::MessageSend => write!(f, "message send"),
			EventType::SubWorkflow => write!(f, "sub workflow"),
			EventType::Loop => write!(f, "loop"),
			EventType::Sleep => write!(f, "sleep"),
			EventType::Removed => write!(f, "removed event"),
			EventType::VersionCheck => write!(f, "version check"),
			EventType::Branch => write!(f, "branch"),
			EventType::Signals => write!(f, "signals"),
		}
	}
}

#[derive(Debug)]
pub struct ActivityEvent {
	pub name: String,
	pub create_ts: i64,

	/// If the activity succeeds, this will be some.
	pub(crate) output: Option<Box<serde_json::value::RawValue>>,
	pub error_count: usize,
}

impl ActivityEvent {
	pub fn parse_output<O: DeserializeOwned>(&self) -> WorkflowResult<Option<O>> {
		self.output
			.as_ref()
			.map(|x| rivet_util::serde::json_from_str!(x.get()))
			.transpose()
			.map_err(WorkflowError::DeserializeActivityOutput)
	}
}

#[derive(Debug)]
pub struct SignalSendEvent {
	pub signal_id: Id,
	pub name: String,
}

#[derive(Debug)]
pub struct MessageSendEvent {
	pub name: String,
}

#[derive(Debug)]
pub struct SubWorkflowEvent {
	pub sub_workflow_id: Id,
	pub name: String,
}

#[derive(Debug)]
pub struct LoopEvent {
	pub(crate) state: Box<serde_json::value::RawValue>,
	/// If the loop completes, this will be some.
	pub(crate) output: Option<Box<serde_json::value::RawValue>>,
	pub iteration: usize,
}

impl LoopEvent {
	pub fn parse_state<S: DeserializeOwned>(&self) -> WorkflowResult<S> {
		rivet_util::serde::json_from_str!(self.state.get())
			.map_err(WorkflowError::DeserializeLoopState)
	}

	pub fn parse_output<O: DeserializeOwned>(&self) -> WorkflowResult<Option<O>> {
		self.output
			.as_ref()
			.map(|x| rivet_util::serde::json_from_str!(x.get()))
			.transpose()
			.map_err(WorkflowError::DeserializeLoopOutput)
	}
}

#[derive(Debug)]
pub struct SleepEvent {
	pub deadline_ts: i64,
	pub state: SleepState,
}

#[derive(Debug, Clone, Hash, Copy, PartialEq, Eq, FromRepr)]
pub enum SleepState {
	Normal = 0,
	Uninterrupted = 1,
	Interrupted = 2,
}

impl std::fmt::Display for SleepState {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SleepState::Normal => write!(f, "normal"),
			SleepState::Uninterrupted => write!(f, "uninterrupted"),
			SleepState::Interrupted => write!(f, "interrupted"),
		}
	}
}

#[derive(Debug)]
pub struct RemovedEvent {
	pub event_type: EventType,
	pub name: Option<String>,
}

#[derive(Debug)]
pub struct VersionCheckEvent {
	pub inner_version: usize,
}

#[derive(Debug)]
pub struct SignalsEvent {
	pub names: Vec<String>,
	pub bodies: Vec<Box<serde_json::value::RawValue>>,
}
