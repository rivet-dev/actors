use std::time::{Duration, Instant};

use prometheus::HistogramVec;
use prometheus::core::Collector;

#[doc(hidden)]
#[must_use = "PerfMeasure must be ended with perf_finish! or perf_abandon!"]
pub struct PerfMeasure {
	pub name: &'static str,
	pub histogram: &'static HistogramVec,
	pub slow_threshold: Duration,
	pub start: Instant,
	pub label_names: Vec<&'static str>,
	pub label_values: Vec<String>,
	pub span: tracing::Span,
	pub finished: bool,
}

impl PerfMeasure {
	#[doc(hidden)]
	pub fn __new(
		name: &'static str,
		histogram: &'static HistogramVec,
		slow_threshold: Duration,
		label_names: Vec<&'static str>,
		label_values: Vec<String>,
		span: tracing::Span,
	) -> Self {
		Self {
			name,
			histogram,
			slow_threshold,
			start: Instant::now(),
			label_names,
			label_values,
			span,
			finished: false,
		}
	}

	#[doc(hidden)]
	pub fn __finish(&mut self) -> Duration {
		self.finished = true;

		let elapsed = self.start.elapsed();
		let metric_label_names = self
			.histogram
			.desc()
			.first()
			.map(|desc| {
				desc.variable_labels
					.iter()
					.map(String::as_str)
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		assert_eq!(
			self.label_names, metric_label_names,
			"PerfMeasure label order must match HistogramVec registration",
		);

		let label_values = self
			.label_values
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();
		self.histogram
			.with_label_values(&label_values)
			.observe(elapsed.as_secs_f64());

		elapsed
	}

	#[doc(hidden)]
	pub fn __abandon(&mut self) {
		self.finished = true;
	}

	#[doc(hidden)]
	pub fn __elapsed_ms(elapsed: Duration) -> u64 {
		elapsed.as_millis().try_into().unwrap_or(u64::MAX)
	}

	#[doc(hidden)]
	pub fn __threshold_ms(&self) -> u64 {
		Self::__elapsed_ms(self.slow_threshold)
	}
}

impl Drop for PerfMeasure {
	fn drop(&mut self) {
		if self.finished {
			return;
		}

		let elapsed = self.start.elapsed();
		let _guard = self.span.enter();
		tracing::debug!(
			name = self.name,
			elapsed_ms = PerfMeasure::__elapsed_ms(elapsed),
			"PerfMeasure dropped without finish() - measurement discarded",
		);
	}
}

#[macro_export]
macro_rules! perf_start {
	(
		$histogram:expr,
		slow_ms = $slow_ms:expr,
		$name:literal,
		labels: { $($labels:tt)* },
		fields: { $($fields:tt)* } $(,)?
	) => {{
		let __label_values = $crate::__perf_label_values!($($labels)*);
		let __label_names = $crate::__perf_label_names!($($labels)*);
		let __span = $crate::__perf_span!($name, labels: { $($labels)* }, fields: { $($fields)* });
		$crate::PerfMeasure::__new(
			$name,
			$histogram,
			::std::time::Duration::from_millis($slow_ms),
			__label_names,
			__label_values,
			__span,
		)
	}};
	(
		$histogram:expr,
		slow_ms = $slow_ms:expr,
		$name:literal,
		labels: { $($labels:tt)* } $(,)?
	) => {
		$crate::perf_start!(
			$histogram,
			slow_ms = $slow_ms,
			$name,
			labels: { $($labels)* },
			fields: {},
		)
	};
}

#[macro_export]
macro_rules! perf_finish {
	($measure:expr, fields: { $($fields:tt)* } $(,)?) => {{
		let mut __measure = $measure;
		let __elapsed = __measure.__finish();
		if __elapsed > __measure.slow_threshold {
			let _guard = __measure.span.enter();
			$crate::__perf_warn!(
				elapsed_ms = $crate::PerfMeasure::__elapsed_ms(__elapsed),
				threshold_ms = __measure.__threshold_ms(),
				fields: { $($fields)* },
			);
		}
		__elapsed
	}};
	($measure:expr $(,)?) => {
		$crate::perf_finish!($measure, fields: {});
	};
}

#[macro_export]
macro_rules! perf_abandon {
	($measure:expr $(,)?) => {{
		let mut __measure = $measure;
		__measure.__abandon();
	}};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_label_values {
	() => {
		::std::vec::Vec::<String>::new()
	};
	($($name:ident = $marker:tt $value:expr),+ $(,)?) => {
		::std::vec![$($crate::__perf_format_label!($marker $value)),+]
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_format_label {
	(% $value:expr) => {
		::std::format!("{}", $value)
	};
	(? $value:expr) => {
		::std::format!("{:?}", $value)
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_label_names {
	() => {
		::std::vec::Vec::<&'static str>::new()
	};
	($($name:ident = $marker:tt $value:expr),+ $(,)?) => {
		::std::vec![$(::std::stringify!($name)),+]
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_span {
	(
		$name:literal,
		labels: { $($label_name:ident = $label_marker:tt $label_value:expr),* $(,)? },
		fields: { $($field_name:ident = $field_marker:tt $field_value:expr),* $(,)? }
	) => {
		::tracing::info_span!(
			$name,
			$($label_name = $label_marker $label_value,)*
			$($field_name = $field_marker $field_value,)*
		)
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_span_items {
	($name:literal, [$($items:tt)*], ; ) => {
		::tracing::info_span!($name, $($items)*)
	};
	($name:literal, [$($items:tt)*], $field:ident = %$value:expr, $($rest:tt)* ; $($fields:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = %$value,], $($rest)* ; $($fields)*)
	};
	($name:literal, [$($items:tt)*], $field:ident = ?$value:expr, $($rest:tt)* ; $($fields:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = ?$value,], $($rest)* ; $($fields)*)
	};
	($name:literal, [$($items:tt)*], $field:ident = %$value:expr ; $($fields:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = %$value,], ; $($fields)*)
	};
	($name:literal, [$($items:tt)*], $field:ident = ?$value:expr ; $($fields:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = ?$value,], ; $($fields)*)
	};
	($name:literal, [$($items:tt)*], ; $field:ident = %$value:expr, $($rest:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = %$value,], ; $($rest)*)
	};
	($name:literal, [$($items:tt)*], ; $field:ident = ?$value:expr, $($rest:tt)*) => {
		$crate::__perf_span_items!($name, [$($items)* $field = ?$value,], ; $($rest)*)
	};
	($name:literal, [$($items:tt)*], ; $field:ident = %$value:expr) => {
		$crate::__perf_span_items!($name, [$($items)* $field = %$value,], ; )
	};
	($name:literal, [$($items:tt)*], ; $field:ident = ?$value:expr) => {
		$crate::__perf_span_items!($name, [$($items)* $field = ?$value,], ; )
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_warn {
	(elapsed_ms = $elapsed_ms:expr, threshold_ms = $threshold_ms:expr, fields: { $($fields:tt)* } $(,)?) => {
		$crate::__perf_warn_items!([$elapsed_ms, $threshold_ms], []; $($fields)*,)
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perf_warn_items {
	([$elapsed_ms:expr, $threshold_ms:expr], [$($items:tt)*];) => {
		::tracing::warn!(
			elapsed_ms = $elapsed_ms,
			threshold_ms = $threshold_ms,
			$($items)*
			"PerfMeasure exceeded slow threshold",
		)
	};
	([$elapsed_ms:expr, $threshold_ms:expr], [$($items:tt)*];,) => {
		$crate::__perf_warn_items!([$elapsed_ms, $threshold_ms], [$($items)*];)
	};
	([$elapsed_ms:expr, $threshold_ms:expr], [$($items:tt)*]; $field:ident = %$value:expr, $($rest:tt)*) => {
		$crate::__perf_warn_items!([$elapsed_ms, $threshold_ms], [$($items)* $field = %$value,]; $($rest)*)
	};
	([$elapsed_ms:expr, $threshold_ms:expr], [$($items:tt)*]; $field:ident = ?$value:expr, $($rest:tt)*) => {
		$crate::__perf_warn_items!([$elapsed_ms, $threshold_ms], [$($items)* $field = ?$value,]; $($rest)*)
	};
	([$elapsed_ms:expr, $threshold_ms:expr], [$($items:tt)*]; $field:ident = $value:expr, $($rest:tt)*) => {
		$crate::__perf_warn_items!([$elapsed_ms, $threshold_ms], [$($items)* $field = $value,]; $($rest)*)
	};
}
