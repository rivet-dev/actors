use anyhow::Result;
use gas::prelude::*;
use universaldb::prelude::*;
use vbare::OwnedVersionedData;

use crate::types::{DeliveryRecord, WebhookConfig};

fn serialize_config(value: WebhookConfig) -> Result<Vec<u8>> {
	rivet_data::versioned::WebhookConfigData::wrap_latest(value.into())
		.serialize_with_embedded_version(rivet_data::WEBHOOK_CONFIG_VERSION)
}

fn deserialize_config(raw: &[u8]) -> Result<WebhookConfig> {
	Ok(rivet_data::versioned::WebhookConfigData::deserialize_with_embedded_version(raw)?.into())
}

fn serialize_delivery(value: DeliveryRecord) -> Result<Vec<u8>> {
	rivet_data::versioned::WebhookDeliveryData::wrap_latest(value.into())
		.serialize_with_embedded_version(rivet_data::WEBHOOK_DELIVERY_VERSION)
}

fn deserialize_delivery(raw: &[u8]) -> Result<DeliveryRecord> {
	Ok(rivet_data::versioned::WebhookDeliveryData::deserialize_with_embedded_version(raw)?.into())
}

// Durable, replicated copy proposed through epoxy. Slow to write and not meant to be read
// frequently; the local `DataKey` below is what backs listing/reads within a datacenter.
#[derive(Debug)]
pub struct GlobalDataKey {
	pub namespace_id: Id,
	pub name: String,
}

impl GlobalDataKey {
	pub fn new(namespace_id: Id, name: String) -> Self {
		GlobalDataKey { namespace_id, name }
	}
}

impl FormalKey for GlobalDataKey {
	type Value = WebhookConfig;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		deserialize_config(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		serialize_config(value)
	}
}

impl TuplePack for GlobalDataKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (WEBHOOK, CONFIG, GLOBAL, DATA, self.namespace_id, &self.name);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for GlobalDataKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (_, _, _, _, namespace_id, name)) =
			<(usize, usize, usize, usize, Id, String)>::unpack(input, tuple_depth)?;

		let v = GlobalDataKey { namespace_id, name };

		Ok((input, v))
	}
}

// Local-only mirror of `GlobalDataKey`, written directly to this datacenter's UDB after every
// epoxy propose succeeds. Listing reads scan this instead of epoxy.
#[derive(Debug)]
pub struct DataKey {
	pub namespace_id: Id,
	pub name: String,
}

impl DataKey {
	pub fn new(namespace_id: Id, name: String) -> Self {
		DataKey { namespace_id, name }
	}

	pub fn subspace(namespace_id: Id) -> DataSubspaceKey {
		DataSubspaceKey::new(namespace_id)
	}
}

impl FormalKey for DataKey {
	type Value = WebhookConfig;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		deserialize_config(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		serialize_config(value)
	}
}

impl TuplePack for DataKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (WEBHOOK, CONFIG, DATA, self.namespace_id, &self.name);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for DataKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (_, _, _, namespace_id, name)) =
			<(usize, usize, usize, Id, String)>::unpack(input, tuple_depth)?;

		let v = DataKey { namespace_id, name };

		Ok((input, v))
	}
}

// Subspace of all webhook `DataKey`s for a namespace, used to list webhook names.
#[derive(Debug)]
pub struct DataSubspaceKey {
	pub namespace_id: Id,
}

impl DataSubspaceKey {
	pub fn new(namespace_id: Id) -> Self {
		DataSubspaceKey { namespace_id }
	}
}

impl TuplePack for DataSubspaceKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (WEBHOOK, CONFIG, DATA, self.namespace_id);
		t.pack(w, tuple_depth)
	}
}

// Local-only record of a single delivery (one triggered event, identified by delivery id, and
// every attempt made to deliver it), written by the webhook workflow. Not replicated through
// epoxy: unlike config, a delivery only ever matters to the datacenter that ran it, since the
// workflow that owns a delivery lives in exactly one datacenter.
#[derive(Debug)]
pub struct DeliveryKey {
	pub namespace_id: Id,
	pub name: String,
	pub delivery_id: String,
}

impl DeliveryKey {
	pub fn new(namespace_id: Id, name: String, delivery_id: String) -> Self {
		DeliveryKey {
			namespace_id,
			name,
			delivery_id,
		}
	}
}

impl FormalKey for DeliveryKey {
	type Value = DeliveryRecord;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		deserialize_delivery(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		serialize_delivery(value)
	}
}

impl TuplePack for DeliveryKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (
			WEBHOOK,
			DELIVERY,
			DATA,
			self.namespace_id,
			&self.name,
			&self.delivery_id,
		);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for DeliveryKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (_, _, _, namespace_id, name, delivery_id)) =
			<(usize, usize, usize, Id, String, String)>::unpack(input, tuple_depth)?;

		let v = DeliveryKey {
			namespace_id,
			name,
			delivery_id,
		};

		Ok((input, v))
	}
}
