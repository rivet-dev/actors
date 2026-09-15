use super::*;

#[test]
fn extracts_namespace_and_token_from_url_auth() {
	let parsed =
		extract_endpoint_auth("https://my-ns:sk_secret@api.rivet.dev".to_owned());
	assert_eq!(parsed.endpoint, "https://api.rivet.dev");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token.as_deref(), Some("sk_secret"));
}

#[test]
fn decodes_percent_encoded_credentials() {
	let parsed =
		extract_endpoint_auth("https://cloud-ns:sk_cloud%2Dtoken@api.rivet.dev".to_owned());
	assert_eq!(parsed.endpoint, "https://api.rivet.dev");
	assert_eq!(parsed.namespace.as_deref(), Some("cloud-ns"));
	assert_eq!(parsed.token.as_deref(), Some("sk_cloud-token"));
}

#[test]
fn supports_namespace_without_token() {
	let parsed = extract_endpoint_auth("https://my-ns@api.rivet.dev".to_owned());
	assert_eq!(parsed.endpoint, "https://api.rivet.dev");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token, None);
}

#[test]
fn preserves_path_when_stripping_auth() {
	let parsed = extract_endpoint_auth("https://my-ns:tok@api.rivet.dev/base".to_owned());
	assert_eq!(parsed.endpoint, "https://api.rivet.dev/base");
	assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
	assert_eq!(parsed.token.as_deref(), Some("tok"));
}

#[test]
fn passes_through_endpoint_without_auth() {
	let parsed = extract_endpoint_auth("http://127.0.0.1:6420".to_owned());
	assert_eq!(parsed.endpoint, "http://127.0.0.1:6420");
	assert_eq!(parsed.namespace, None);
	assert_eq!(parsed.token, None);
}

#[test]
fn ignores_token_without_namespace() {
	let parsed = extract_endpoint_auth("https://:sk_secret@api.rivet.dev".to_owned());
	assert_eq!(parsed.endpoint, "https://:sk_secret@api.rivet.dev");
	assert_eq!(parsed.namespace, None);
	assert_eq!(parsed.token, None);
}

#[test]
fn passes_through_non_url_endpoint() {
	let parsed = extract_endpoint_auth("not a url".to_owned());
	assert_eq!(parsed.endpoint, "not a url");
	assert_eq!(parsed.namespace, None);
	assert_eq!(parsed.token, None);
}
