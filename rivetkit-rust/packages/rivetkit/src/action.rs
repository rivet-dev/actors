use std::future::Future;
use std::io::Cursor;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use ciborium::Value;
use serde::de::{self, DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};

use crate::{actor::Actor, context::Ctx};

pub const TUPLE_ARITY_MAX: usize = 128;
pub(crate) type BoxActionFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;

pub trait Action: serde::Serialize + DeserializeOwned + Send + Sync + 'static {
	type Output: serde::Serialize + DeserializeOwned + Send + 'static;

	const NAME: &'static str;
}

pub fn encode_positional<T: Serialize>(value: &T) -> Result<Vec<u8>> {
	encode_varargs(value, "action args")
}

pub(crate) fn encode_varargs<T: Serialize>(value: &T, label: &str) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded)
		.with_context(|| format!("encode {label} as cbor"))?;
	let value: Value = ciborium::from_reader(Cursor::new(&encoded))
		.with_context(|| format!("decode {label} into cbor value"))?;
	let value = positional_value(value);
	encode_value(&value, label)
}

pub fn decode_positional<T: DeserializeOwned>(args: &[u8]) -> Result<T> {
	let value = if args.is_empty() {
		Value::Array(Vec::new())
	} else {
		ciborium::from_reader(Cursor::new(args)).context("decode action args from cbor")?
	};
	let value = match value {
		Value::Null => Value::Array(Vec::new()),
		value => value,
	};

	match decode_value::<T>(&value) {
		Ok(value) => Ok(value),
		Err(first_error) => match &value {
			Value::Array(values) if values.is_empty() => decode_value(&Value::Null)
				.or_else(|_| Err(first_error).context("decode positional action args as unit")),
			Value::Array(values) if values.len() == 1 => decode_value(&values[0])
				.or_else(|_| Err(first_error).context("decode positional action args as newtype")),
			_ => Err(first_error).context("decode positional action args"),
		},
	}
}

fn positional_value(value: Value) -> Value {
	match value {
		Value::Map(_) => Value::Array(vec![value]),
		Value::Array(values) => Value::Array(values),
		Value::Null => Value::Array(Vec::new()),
		value => Value::Array(vec![value]),
	}
}

fn encode_value(value: &Value, label: &str) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded)
		.with_context(|| format!("encode positional {label} as cbor"))?;
	Ok(encoded)
}

fn decode_value<T: DeserializeOwned>(value: &Value) -> Result<T> {
	crate::event::deserialize_cbor_value(value.clone())
		.map_err(|error| anyhow::anyhow!(error.to_string()))
		.context("decode positional action args from cbor")
}

pub trait Handles<A: Action>: Actor + Sized {
	type Future: Future<Output = Result<A::Output>> + Send + 'static;

	fn handle(self: Arc<Self>, ctx: Ctx<Self>, action: A) -> Self::Future;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionEntry<A: Actor> {
	pub name: &'static str,
	_p: PhantomData<fn() -> A>,
}

impl<A: Actor> ActionEntry<A> {
	pub const fn new(name: &'static str) -> Self {
		Self {
			name,
			_p: PhantomData,
		}
	}
}

pub trait ActionSet<A: Actor>: Send + Sync + 'static {
	fn entries() -> Vec<ActionEntry<A>>;
	fn dispatch(actor: Arc<A>, ctx: Ctx<A>, name: &str, args: &[u8]) -> Option<BoxActionFuture>;
}

impl<A: Actor> ActionSet<A> for () {
	fn entries() -> Vec<ActionEntry<A>> {
		Vec::new()
	}

	fn dispatch(
		_actor: Arc<A>,
		_ctx: Ctx<A>,
		_name: &str,
		_args: &[u8],
	) -> Option<BoxActionFuture> {
		None
	}
}

macro_rules! impl_action_set {
	($($action:ident),+) => {
		impl<Act, $($action),+> ActionSet<Act> for ($($action,)+)
		where
			Act: Actor + $(Handles<$action> +)+,
			$($action: Action,)+
		{
			fn entries() -> Vec<ActionEntry<Act>> {
				vec![$(ActionEntry::new(<$action as Action>::NAME)),+]
			}

			fn dispatch(
				actor: Arc<Act>,
				ctx: Ctx<Act>,
				name: &str,
				args: &[u8],
			) -> Option<BoxActionFuture> {
				$(
					if name == <$action as Action>::NAME {
						let args = args.to_vec();
						return Some(Box::pin(async move {
							let action = decode_positional::<$action>(&args).with_context(|| {
								format!("decode action '{}' args", <$action as Action>::NAME)
							})?;
							let output = <Act as Handles<$action>>::handle(actor, ctx, action).await?;
							encode_cbor(&output, "action response")
						}));
					}
				)+
				None
			}
		}
	};
}

macro_rules! impl_action_sets {
	(@accum [$($action:ident),*]) => {};
	(@accum [$($action:ident),*] $next:ident $(, $rest:ident)*) => {
		impl_action_set!($($action,)* $next);
		impl_action_sets!(@accum [$($action,)* $next] $($rest),*);
	};
	($first:ident $(, $rest:ident)*) => {
		impl_action_sets!(@accum [] $first $(, $rest)*);
	};
}

macro_rules! with_action_type_params {
	($callback:ident) => {
		$callback!(
			A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18,
			A19, A20, A21, A22, A23, A24, A25, A26, A27, A28, A29, A30, A31, A32, A33, A34, A35,
			A36, A37, A38, A39, A40, A41, A42, A43, A44, A45, A46, A47, A48, A49, A50, A51, A52,
			A53, A54, A55, A56, A57, A58, A59, A60, A61, A62, A63, A64, A65, A66, A67, A68, A69,
			A70, A71, A72, A73, A74, A75, A76, A77, A78, A79, A80, A81, A82, A83, A84, A85, A86,
			A87, A88, A89, A90, A91, A92, A93, A94, A95, A96, A97, A98, A99, A100, A101, A102,
			A103, A104, A105, A106, A107, A108, A109, A110, A111, A112, A113, A114, A115, A116,
			A117, A118, A119, A120, A121, A122, A123, A124, A125, A126, A127
		);
	};
}

with_action_type_params!(impl_action_sets);

fn encode_cbor<T: Serialize>(value: &T, label: &str) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded)
		.with_context(|| format!("encode {label} as cbor"))?;
	Ok(encoded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raw;

impl<'de> Deserialize<'de> for Raw {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let _ = de::IgnoredAny::deserialize(deserializer)?;
		Err(de::Error::custom(
			"rivetkit::action::Raw cannot be deserialized; use Action::raw_args() or Action::decode_as(...) instead",
		))
	}
}

#[cfg(test)]
mod tests {
	use std::future::{Ready, ready};
	use std::sync::Arc;

	use anyhow::Result;
	use serde::Deserialize;
	use serde::Serialize;
	use serde::de::value::{Error as ValueError, UnitDeserializer};

	use super::{Action, ActionSet, Handles, Raw, decode_positional, encode_positional};
	use crate::{actor::Actor, context::Ctx};

	#[test]
	fn raw_deserialize_fails_with_guidance() {
		let err = Raw::deserialize(UnitDeserializer::<ValueError>::new())
			.expect_err("Raw should refuse serde decoding");

		let message = err.to_string();
		assert!(message.contains("Action::raw_args()"));
		assert!(message.contains("Action::decode_as"));
	}

	struct TestActor;

	impl Actor for TestActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = Raw;
	}

	#[derive(Debug, Serialize, Deserialize)]
	struct First;

	impl Action for First {
		type Output = ();

		const NAME: &'static str = "first";
	}

	#[derive(Debug, Serialize, Deserialize)]
	struct Second;

	impl Action for Second {
		type Output = ();

		const NAME: &'static str = "second";
	}

	impl Handles<First> for TestActor {
		type Future = Ready<Result<()>>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: First) -> Self::Future {
			ready(Ok(()))
		}
	}

	impl Handles<Second> for TestActor {
		type Future = Ready<Result<()>>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: Second) -> Self::Future {
			ready(Ok(()))
		}
	}

	#[test]
	fn action_set_unit_registers_nothing() {
		assert!(<() as ActionSet<TestActor>>::entries().is_empty());
	}

	#[test]
	fn action_set_tuple_registers_names_in_order() {
		let entries = <(First, Second) as ActionSet<TestActor>>::entries();

		assert_eq!(
			entries.iter().map(|entry| entry.name).collect::<Vec<_>>(),
			["first", "second",]
		);
	}

	#[test]
	fn action_set_tuple_supports_one_and_max_arity() {
		assert_eq!(
			<(First,) as ActionSet<TestActor>>::entries()
				.iter()
				.map(|entry| entry.name)
				.collect::<Vec<_>>(),
			["first"]
		);

		macro_rules! replace_with_first {
			($_action:ident) => {
				First
			};
		}
		macro_rules! max_actions_type {
			($($action:ident),+) => {
				type MaxActions = ($(replace_with_first!($action),)+);
			};
		}
		with_action_type_params!(max_actions_type);
		let entries = <MaxActions as ActionSet<TestActor>>::entries();

		assert_eq!(entries.len(), super::TUPLE_ARITY_MAX);
		assert!(entries.iter().all(|entry| entry.name == "first"));
	}

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct NamedArgs {
		first: String,
		second: String,
	}

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct TupleArgs(String, String);

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct NewtypeArg(u32);

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct UnitArg;

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct Nested {
		value: u32,
		label: String,
	}

	#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
	struct WithNested {
		nested: Nested,
		flag: bool,
	}

	#[test]
	fn positional_encode_matches_ts_action_args() {
		assert_eq!(
			encode_positional(&NamedArgs {
				first: "a".into(),
				second: "b".into(),
			})
			.expect("encode named args"),
			vec![
				0x81, 0xa2, 0x65, b'f', b'i', b'r', b's', b't', 0x61, b'a', 0x66, b's', b'e', b'c',
				b'o', b'n', b'd', 0x61, b'b',
			]
		);
		assert_eq!(
			encode_positional(&NewtypeArg(5)).expect("encode newtype arg"),
			vec![0x81, 0x05]
		);
		assert_eq!(
			encode_positional(&UnitArg).expect("encode unit arg"),
			vec![0x80]
		);
	}

	#[test]
	fn positional_round_trips_arg_shapes() {
		let named = NamedArgs {
			first: "a".into(),
			second: "b".into(),
		};
		assert_eq!(
			decode_positional::<NamedArgs>(&encode_positional(&named).expect("encode named args"))
				.expect("decode named args"),
			named
		);

		let tuple = TupleArgs("a".into(), "b".into());
		assert_eq!(
			decode_positional::<TupleArgs>(&encode_positional(&tuple).expect("encode tuple args"))
				.expect("decode tuple args"),
			tuple
		);

		assert_eq!(
			decode_positional::<NewtypeArg>(
				&encode_positional(&NewtypeArg(5)).expect("encode newtype arg")
			)
			.expect("decode newtype arg"),
			NewtypeArg(5)
		);

		assert_eq!(
			decode_positional::<UnitArg>(&encode_positional(&UnitArg).expect("encode unit arg"))
				.expect("decode unit arg"),
			UnitArg
		);
	}

	#[test]
	fn positional_decode_accepts_named_struct_seq_and_map() {
		let from_seq = decode_positional::<NamedArgs>(&cbor(&vec!["a", "b"]))
			.expect("decode named args from positional seq");
		assert_eq!(
			from_seq,
			NamedArgs {
				first: "a".into(),
				second: "b".into(),
			}
		);

		let from_map = decode_positional::<NamedArgs>(&cbor(&NamedArgs {
			first: "a".into(),
			second: "b".into(),
		}))
		.expect("decode named args from map");
		assert_eq!(from_map, from_seq);

		let from_single_map_arg = decode_positional::<NamedArgs>(&cbor(&vec![NamedArgs {
			first: "a".into(),
			second: "b".into(),
		}]))
		.expect("decode named args from single object arg");
		assert_eq!(from_single_map_arg, from_seq);
	}

	#[test]
	fn positional_decode_uses_field_order() {
		let decoded = decode_positional::<NamedArgs>(&cbor(&vec!["first", "second"]))
			.expect("decode ordered fields");
		assert_eq!(decoded.first, "first");
		assert_eq!(decoded.second, "second");

		let err = decode_positional::<NamedArgs>(&cbor(&vec![7, 8]))
			.expect_err("wrong positional field types should fail");
		assert!(err.to_string().contains("decode positional action args"));
	}

	#[test]
	fn positional_encode_wraps_named_struct_as_single_arg() {
		let bytes = encode_positional(&WithNested {
			nested: Nested {
				value: 7,
				label: "inside".into(),
			},
			flag: true,
		})
		.expect("encode nested args");
		let value: ciborium::Value =
			ciborium::from_reader(std::io::Cursor::new(bytes)).expect("decode cbor value");

		let ciborium::Value::Array(values) = value else {
			panic!("top-level args should be an array");
		};
		assert_eq!(values.len(), 1);
		let ciborium::Value::Map(fields) = &values[0] else {
			panic!("named struct arg should remain a map");
		};
		assert_eq!(fields.len(), 2);
	}

	fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
		let mut encoded = Vec::new();
		ciborium::into_writer(value, &mut encoded).expect("encode test value as cbor");
		encoded
	}
}
