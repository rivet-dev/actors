pub use rivet_util_serde::*;

/// Wraps `serde_json::to_vec` with observability.
#[macro_export]
macro_rules! json_to_vec {
	($value:expr) => {{
		let __res = $crate::observe!(serde_json::to_vec($value));
		if let std::result::Result::Ok(__res) = &__res {
			$crate::metrics::SERIALIZE_SIZE
				.with_label_values(&["json", $crate::location!().to_string().as_str()])
				.observe(__res.len() as f64);
		}
		__res
	}};
}
pub use json_to_vec;

/// Wraps `serde_json::to_string` with observability.
#[macro_export]
macro_rules! json_to_string {
	($value:expr) => {{
		let __res = $crate::observe!(serde_json::to_string($value));
		if let std::result::Result::Ok(__res) = &__res {
			$crate::metrics::SERIALIZE_SIZE
				.with_label_values(&["json", $crate::location!().to_string().as_str()])
				.observe(__res.len() as f64);
		}
		__res
	}};
}
pub use json_to_string;

/// Wraps `serde_json::to_value` with observability.
#[macro_export]
macro_rules! json_to_value {
	($value:expr) => {{ $crate::observe!(serde_json::to_value($value)) }};
}
pub use json_to_value;

/// Wraps `serde_json::value::to_raw_value` with observability.
#[macro_export]
macro_rules! json_to_raw_value {
	($value:expr) => {{
		let __res = $crate::observe!(serde_json::value::to_raw_value($value));
		if let std::result::Result::Ok(__res) = &__res {
			$crate::metrics::SERIALIZE_SIZE
				.with_label_values(&["json", $crate::location!().to_string().as_str()])
				.observe(__res.get().len() as f64);
		}
		__res
	}};
}
pub use json_to_raw_value;

/// Wraps `serde_json::to_vec` with observability.
#[macro_export]
macro_rules! json_from_str {
	($value:expr) => {{
		let __bind = $value;
		$crate::metrics::DESERIALIZE_SIZE
			.with_label_values(&["json", $crate::location!().to_string().as_str()])
			.observe(__bind.len() as f64);
		$crate::observe!(serde_json::from_str(__bind))
	}};
}
pub use json_from_str;

/// Wraps `serde_json::to_vec` with observability.
#[macro_export]
macro_rules! json_from_slice {
	($value:expr) => {{
		let __bind = $value;
		$crate::metrics::DESERIALIZE_SIZE
			.with_label_values(&["json", $crate::location!().to_string().as_str()])
			.observe(__bind.len() as f64);
		$crate::observe!(serde_json::from_slice(__bind))
	}};
}
pub use json_from_slice;

/// Wraps `rivet_util::serde::bare_to_vec!` with observability.
#[macro_export]
macro_rules! bare_to_vec {
	($value:expr) => {{
		let __res = $crate::observe!(serde_bare::to_vec($value));
		if let std::result::Result::Ok(__res) = &__res {
			$crate::metrics::SERIALIZE_SIZE
				.with_label_values(&["bare", $crate::location!().to_string().as_str()])
				.observe(__res.len() as f64);
		}
		__res
	}};
}
pub use bare_to_vec;

/// Wraps `rivet_util::serde::bare_to_vec!` with observability.
#[macro_export]
macro_rules! bare_from_slice {
	($value:expr) => {{
		let __bind = $value;
		$crate::metrics::DESERIALIZE_SIZE
			.with_label_values(&["bare", $crate::location!().to_string().as_str()])
			.observe(__bind.len() as f64);
		$crate::observe!(serde_bare::from_slice(__bind))
	}};
}
pub use bare_from_slice;
