use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::common::ActorKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRequest {
	pub name: String,
	pub key: ActorKey,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub input: Option<JsonValue>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub region: Option<String>,
	#[serde(rename = "poolName", skip_serializing_if = "Option::is_none")]
	pub pool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetForKeyRequest {
	pub name: String,
	pub key: ActorKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetForIdRequest {
	pub name: String,
	#[serde(rename = "actorId")]
	pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOrCreateRequest {
	pub name: String,
	pub key: ActorKey,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub input: Option<JsonValue>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub region: Option<String>,
	#[serde(rename = "poolName", skip_serializing_if = "Option::is_none")]
	pub pool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActorQuery {
	GetForId {
		#[serde(rename = "getForId")]
		get_for_id: GetForIdRequest,
	},
	GetForKey {
		#[serde(rename = "getForKey")]
		get_for_key: GetForKeyRequest,
	},
	GetOrCreateForKey {
		#[serde(rename = "getOrCreateForKey")]
		get_or_create_for_key: GetOrCreateRequest,
	},
	Create {
		create: CreateRequest,
	},
}
