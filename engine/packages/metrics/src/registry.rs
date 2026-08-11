use prometheus::*;

lazy_static::lazy_static! {
	pub static ref REGISTRY: Registry = Registry::new_custom(
		Some("rivet".to_string()),
		Some(labels! { })).unwrap();
}
