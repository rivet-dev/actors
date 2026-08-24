use std::fmt::Display;

pub use id::Id;
pub use rivet_util_id as id;

pub mod async_counter;
pub mod billing;
pub mod build_meta;
pub mod check;
pub mod duration;
pub mod format;
pub mod future;
pub mod geo;
pub mod math;
pub mod metric;
pub mod metrics;
pub mod req;
pub mod serde;
pub mod size;
pub mod sort;
pub mod throttle;
pub mod timestamp;
pub mod url;

/// Slices a string without panicking on char boundaries. Defaults to the left side of the char if a slice
/// is invalid.
pub fn safe_slice(s: &str, start: usize, end: usize) -> &str {
	if s.is_empty() || end <= start {
		return "";
	}

	let mut new_start = 0;
	let mut new_end = s.len().saturating_sub(1);

	for (i, _) in s.char_indices() {
		if i <= start {
			new_start = i;
		}

		if i >= end {
			break;
		}

		new_end = i;
	}

	&s[new_start..=new_end]
}

/// Records the duration of the code inside the macro.
///
/// ```rust
/// observe!(task());
/// // or
/// observe!(long, long_task());
/// ```
///
/// Supports async work.
///	Use `observe_with!` for callback.
/// ```
#[macro_export]
macro_rules! observe {
	(long, $($tt:tt)*) => {{
		let __start = std::time::Instant::now();

		let __res = $($tt)*;
		let __dt = __start.elapsed().as_secs_f64();

		let __location = format!("{}:{}:{}", file!(), line!(), column!());
		$crate::metrics::LONG_OBSERVATION_DURATION.with_label_values(&[&__location])
			.observe(__dt);

		__res
	}};
	($($tt:tt)*) => {{
		let __start = std::time::Instant::now();

		let __res = $($tt)*;
		let __dt = __start.elapsed().as_secs_f64();

		let __location = format!("{}:{}:{}", file!(), line!(), column!());
		$crate::metrics::OBSERVATION_DURATION.with_label_values(&[&__location])
			.observe(__dt);

		__res
	}};
}

/// Records the duration of the code inside the macro and a callback macro.
///
/// ```rust
/// observe_with!(task(), |dt, location| {
///     if dt > Duration::from_secs(10) {
///         tracing::warn!("long work at {location}");
///     }
/// });
/// // or
/// observe_with!(long, task(), |dt, location| {
///     if dt > Duration::from_secs(10) {
///         tracing::warn!("long work at {location}");
///     }
/// });
/// ```
///
/// Supports async work.
#[macro_export]
macro_rules! observe_with {
	(long, $cb:expr, $($tt:tt)*) => {{
		let __start = std::time::Instant::now();

		let __res = $($tt)*;
		let __dt = __start.elapsed().as_secs_f64();

		let __location = $crate::location!().to_string();

		($cb)(__dt, __location.as_str());

		$crate::metrics::LONG_OBSERVATION_DURATION.with_label_values(&[__location.as_str()])
			.observe(__dt);

		__res
	}};
	($cb:expr, $($tt:tt)*) => {{
		let __start = std::time::Instant::now();

		let __res = $($tt)*;
		let __dt = __start.elapsed().as_secs_f64();

		let __location = $crate::location!().to_string();

		($cb)(__dt, __location.as_str());

		$crate::metrics::OBSERVATION_DURATION.with_label_values(&[__location.as_str()])
			.observe(__dt);

		__res
	}};
}

#[derive(Debug)]
pub struct Location {
	file: &'static str,
	line: u32,
	column: u32,
}

impl Location {
	pub fn new(file: &'static str, line: u32, column: u32) -> Self {
		Location { file, line, column }
	}
}

impl Display for Location {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}:{}:{}", self.file, self.line, self.column)
	}
}

/// Constructs a `Location` object with the current file name, line number, and
/// column number.
///
/// # Examples
///
/// ```
/// let loc = location!();
/// println!("This code is at: {:?}", loc);
/// ```
#[macro_export]
macro_rules! location {
	() => {
		$crate::Location::new(file!(), line!(), column!())
	};
}
