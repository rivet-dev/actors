use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Policy for outbound HTTP requests to user-configured destinations, such as serverless runner
/// URLs.
///
/// These requests originate from inside the trusted engine network, so without restrictions a
/// caller who can configure a runner can reach internal-only services. The defaults deny every
/// non-globally-routable destination except loopback.
#[derive(Debug, Serialize, Deserialize, Clone, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outbound {
	/// Allow destinations that resolve to loopback addresses (127.0.0.0/8, ::1).
	///
	/// Enabled by default so local development against `http://localhost:...` works without
	/// configuration.
	pub allow_loopback: Option<bool>,
	/// Allow destinations that resolve to private, link-local, shared (CGNAT), or otherwise
	/// non-globally-routable addresses.
	///
	/// Self-hosted deployments that point runners at addresses on their own network, such as a
	/// Docker Compose service name, need this enabled.
	pub allow_private_networks: Option<bool>,
	/// Allow plaintext `http://` destinations. When disabled only `https://` is permitted.
	pub allow_insecure_scheme: Option<bool>,
	/// Additional CIDRs that are always permitted, evaluated after `deny_cidrs`.
	///
	/// Use this to reach a specific internal service without opening up the whole private range.
	pub allow_cidrs: Option<Vec<String>>,
	/// Additional CIDRs that are always denied. Takes precedence over every allow rule.
	pub deny_cidrs: Option<Vec<String>>,
	/// Maximum number of redirects to follow. Every hop is re-checked against this policy.
	pub max_redirects: Option<usize>,
}

impl Outbound {
	pub fn allow_loopback(&self) -> bool {
		self.allow_loopback.unwrap_or(true)
	}

	pub fn allow_private_networks(&self) -> bool {
		self.allow_private_networks.unwrap_or(false)
	}

	pub fn allow_insecure_scheme(&self) -> bool {
		self.allow_insecure_scheme.unwrap_or(true)
	}

	pub fn allow_cidrs(&self) -> &[String] {
		self.allow_cidrs.as_deref().unwrap_or(&[])
	}

	pub fn deny_cidrs(&self) -> &[String] {
		self.deny_cidrs.as_deref().unwrap_or(&[])
	}

	pub fn max_redirects(&self) -> usize {
		self.max_redirects.unwrap_or(4)
	}
}
