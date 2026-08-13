pub trait Signal {
	const NAME: &'static str;
}

/// Creates an enum that implements `Listen` and selects one of X signals.
///
/// Example:
/// ```rust,ignore
/// #[signal("my_signal")]
/// struct MySignal {
/// 	x: i64,
/// }
///
/// #[signal("my_signal2")]
/// struct MySignal2 {
/// 	y: i64,
/// }
///
/// join_signal!(MyJoinSignal {
/// 	MySignal,
/// 	MySignal2
/// });
///
/// // Automatically becomes:
/// enum MyJoinSignal {
/// 	MySignal(MySignal),
/// 	MySignal2(MySignal2),
/// }
///
/// // Listening:
/// match ctx.listen::<MyJoinSignal>() {
/// 	MySignal(sig) => println!("received MySignal {sig:?}"),
/// 	MySignal2(sig) => println!("received MySignal2 {sig:?}"),
/// }
///
///
/// // Also allows aliases:
/// join_signal!(MyJoinSignal {
/// 	MySignal,
/// 	MySignal2(some_pkg::Signal),
/// });
/// ```
#[macro_export]
macro_rules! join_signal {
	($vis:vis $join:ident { $($tt:tt)* }) => {
		join_signal!(@ $vis $join [] [] $($tt)*);
	};
	(@
		$vis:vis $join:ident
		[$({ $names:tt } { $types:tt })*]
		[$({ $just_types:tt })*]
	) => {
		$vis enum $join {
			$( $names ($types) ),*
		}

		#[async_trait::async_trait]
		impl Listen for $join {
			#[tracing::instrument(skip_all, fields(t=std::any::type_name::<Self>()))]
			async fn listen(ctx: &mut gas::prelude::ListenCtx, limit: usize) -> gas::prelude::WorkflowResult<Vec<Self>> {
				ctx
					.listen_any(&[
						$(<$just_types as gas::signal::Signal>::NAME),*
					], limit)
					.await?
					.into_iter()
					.map(|signal| Self::parse(&signal.signal_name, &signal.body))
					.collect()
			}

			fn parse(name: &str, body: &serde_json::value::RawValue) -> gas::prelude::WorkflowResult<Self> {
				$(
					if name == <$types as gas::signal::Signal>::NAME {
						std::result::Result::Ok(
							Self::$names(
								rivet_util::serde::json_from_str!(body.get())
									.map_err(WorkflowError::DeserializeSignalBody)?
							)
						)
					}
				)else*
				else {
					unreachable!(
						"received signal that wasn't queried for: {}, expected {:?} (body: {:?})",
						name, &[$(<$just_types as gas::signal::Signal>::NAME),*], body,
					);
				}
			}
		}
	};
	(@
		$vis:vis $join:ident
		[$({ $names:tt } { $types:tt })*]
		[$({ $just_types:tt })*]
		$name:ident,
		$($tail:tt)*
	) => {
	   join_signal!(@
	   	$vis $join
	   	[$( { $names } { $types } )* { $name } { $name }]
	   	[$( { $just_types } )* { $name }]
	   	$($tail)*
	   );
	};
	(@
		$vis:vis $join:ident
		[$({ $names:tt } { $types:tt })*]
		[$({ $just_types:tt })*]
		$name:ident($ty:ty),
		$($tail:tt)*
	) => {
	   join_signal!(@
	   	$vis $join
	   	[$( { $names } { $types } )* { $name } { $ty }]
	   	[$( { $just_types } )* { $ty }]
	   	$($tail)*
	   );
	};
}
pub use join_signal;
