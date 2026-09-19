use super::*;

#[test]
fn extracts_namespace_and_token_from_url_auth() {
	let parsed = extract_endpoint_auth("https://my-ns:sk_secret@api.rivet.dev".to_owned()).unwrap();
	assert_eq!(parsed.endpoint, "https://api.rivet.dev/");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token.as_deref(), Some("sk_secret"));
}

#[test]
fn decodes_percent_encoded_credentials() {
	let parsed =
		extract_endpoint_auth("https://cloud-ns:sk_cloud%2Dtoken@api.rivet.dev".to_owned())
			.unwrap();
	assert_eq!(parsed.endpoint, "https://api.rivet.dev/");
	assert_eq!(parsed.namespace.as_deref(), Some("cloud-ns"));
	assert_eq!(parsed.token.as_deref(), Some("sk_cloud-token"));
}

#[test]
fn supports_namespace_without_token() {
	let parsed = extract_endpoint_auth("https://my-ns@api.rivet.dev".to_owned()).unwrap();
	assert_eq!(parsed.endpoint, "https://api.rivet.dev/");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token, None);
}

#[test]
fn preserves_path_when_stripping_auth() {
	let parsed = extract_endpoint_auth("https://my-ns:tok@api.rivet.dev/base".to_owned()).unwrap();
	assert_eq!(parsed.endpoint, "https://api.rivet.dev/base");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token.as_deref(), Some("tok"));
}

#[test]
fn normalizes_endpoint_without_auth() {
	let parsed = extract_endpoint_auth("http://127.0.0.1:6420".to_owned()).unwrap();
	assert_eq!(parsed.endpoint, "http://127.0.0.1:6420/");
	assert_eq!(parsed.namespace, None);
	assert_eq!(parsed.token, None);
}

// A missing scheme makes the namespace parse as one, producing an opaque URL
// with no auth. It must pass through instead of failing on credential
// stripping, matching the silent WHATWG setter no-op in TypeScript.
#[test]
fn passes_through_opaque_scheme_url() {
	let parsed = extract_endpoint_auth("my-ns:tok@api.rivet.dev".to_owned()).unwrap();
	assert_eq!(parsed.endpoint, "my-ns:tok@api.rivet.dev");
	assert_eq!(parsed.namespace, None);
	assert_eq!(parsed.token, None);
}

#[test]
fn rejects_token_without_namespace() {
	let error = extract_endpoint_auth("https://:sk_secret@api.rivet.dev".to_owned()).unwrap_err();
	assert!(error.to_string().contains("token without a namespace"));
}

#[test]
fn rejects_non_url_endpoint() {
	let error = extract_endpoint_auth("not a url".to_owned()).unwrap_err();
	assert!(error.to_string().contains("invalid URL"));
}

#[test]
fn rejects_query_string() {
	let error =
		extract_endpoint_auth("https://my-ns:tok@api.rivet.dev?foo=bar".to_owned()).unwrap_err();
	assert!(error.to_string().contains("query string"));
}

#[test]
fn rejects_fragment() {
	let error =
		extract_endpoint_auth("https://my-ns:tok@api.rivet.dev#section".to_owned()).unwrap_err();
	assert!(error.to_string().contains("fragment"));
}
