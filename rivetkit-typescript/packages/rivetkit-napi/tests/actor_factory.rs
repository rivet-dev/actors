use super::*;

mod moved_tests {
	use std::io;
	use std::io::Write;
	use std::sync::Arc;

	use parking_lot::Mutex;
	use rivet_error::{MacroMarker, RivetError, RivetErrorSchema};
	use tracing::Level;
	use tracing_subscriber::fmt::MakeWriter;

	use super::{BRIDGE_RIVET_ERROR_PREFIX, parse_bridge_rivet_error};

	static AUTH_FORBIDDEN_SCHEMA: RivetErrorSchema = RivetErrorSchema {
		group: "auth",
		code: "forbidden",
		default_message: "Forbidden",
		meta_type: None,
		_macro_marker: MacroMarker { _private: () },
	};

	#[derive(Clone, Default)]
	struct LogCapture(Arc<Mutex<Vec<u8>>>);

	struct LogCaptureWriter(Arc<Mutex<Vec<u8>>>);

	impl LogCapture {
		fn output(&self) -> String {
			String::from_utf8(self.0.lock().clone()).expect("log capture should stay utf-8")
		}
	}

	impl<'a> MakeWriter<'a> for LogCapture {
		type Writer = LogCaptureWriter;

		fn make_writer(&'a self) -> Self::Writer {
			LogCaptureWriter(Arc::clone(&self.0))
		}
	}

	impl Write for LogCaptureWriter {
		fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
			self.0.lock().extend_from_slice(buf);
			Ok(buf.len())
		}

		fn flush(&mut self) -> io::Result<()> {
			Ok(())
		}
	}

	fn transport_error(error: &anyhow::Error) -> &RivetError {
		error
			.chain()
			.find_map(|cause| cause.downcast_ref::<RivetError>())
			.expect("expected bridged rivet error")
	}

	#[test]
	fn parse_bridge_rivet_error_uses_dynamic_error_kind() {
		let reason = format!(
			"{BRIDGE_RIVET_ERROR_PREFIX}{}",
			serde_json::json!({
				"group": "actor",
				"code": "same_code",
				"message": "same message",
				"metadata": { "count": 1 },
			})
		);

		let first = parse_bridge_rivet_error(&reason).expect("first parse should succeed");
		let second = parse_bridge_rivet_error(&reason).expect("second parse should succeed");

		assert_eq!(first.to_string(), "same message");
		assert!(transport_error(&first).schema().is_none());
		assert_eq!(transport_error(&second).group(), "actor");
		assert_eq!(transport_error(&second).code(), "same_code");
	}

	#[test]
	fn napi_bridge_payload_promotes_known_core_error_status() {
		let payload = crate::anyhow_to_bridge_rivet_error_payload(anyhow::Error::new(RivetError {
			kind: rivet_error::RivetErrorKind::Static(&AUTH_FORBIDDEN_SCHEMA),
			meta: None,
			message: None,
			actor: None,
		}));

		assert_eq!(
			payload.get("group").and_then(|value| value.as_str()),
			Some("auth")
		);
		assert_eq!(
			payload.get("code").and_then(|value| value.as_str()),
			Some("forbidden")
		);
		assert_eq!(
			payload.get("public").and_then(|value| value.as_bool()),
			Some(true)
		);
		assert_eq!(
			payload.get("statusCode").and_then(|value| value.as_u64()),
			Some(403)
		);
	}

	#[test]
	fn parse_bridge_rivet_error_warns_for_malformed_payload() {
		let capture = LogCapture::default();
		let subscriber = tracing_subscriber::fmt()
			.with_writer(capture.clone())
			.with_max_level(Level::WARN)
			.with_ansi(false)
			.with_target(false)
			.without_time()
			.finish();
		let _guard = tracing::subscriber::set_default(subscriber);

		let malformed = format!("{BRIDGE_RIVET_ERROR_PREFIX}{{not-json");
		assert!(parse_bridge_rivet_error(&malformed).is_none());

		let logs = capture.output();
		assert!(logs.contains("malformed BridgeRivetErrorPayload"));
		assert!(logs.contains("parse_err"));
	}
}
