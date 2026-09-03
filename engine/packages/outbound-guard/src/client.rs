use std::net::SocketAddr;
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::policy::{BlockReason, Policy};

/// A DNS resolver that drops every address the policy disallows.
///
/// Placing the check here rather than before the request closes the DNS rebinding window: reqwest
/// connects to exactly the addresses this returns, and it runs for every redirect hop as well as
/// the initial request.
#[derive(Debug)]
pub struct GuardedResolver {
	policy: Arc<Policy>,
}

impl GuardedResolver {
	pub fn new(policy: Arc<Policy>) -> Self {
		GuardedResolver { policy }
	}
}

impl Resolve for GuardedResolver {
	fn resolve(&self, name: Name) -> Resolving {
		let policy = self.policy.clone();
		let host = name.as_str().to_string();

		Box::pin(async move {
			// The port is discarded by the connector, which substitutes the real one.
			let resolved = tokio::net::lookup_host((host.as_str(), 0))
				.await
				.map_err(|err| {
					tracing::debug!(%host, ?err, "failed to resolve outbound host");
					Box::new(BlockReason::ResolutionFailed { host: host.clone() })
						as Box<dyn std::error::Error + Send + Sync>
				})?
				.map(|addr| addr.ip())
				.collect::<Vec<_>>();

			let addrs = policy
				.filter_addrs(&host, resolved)?
				.into_iter()
				.map(|ip| SocketAddr::new(ip, 0))
				.collect::<Vec<_>>();

			Ok(Box::new(addrs.into_iter()) as Addrs)
		})
	}
}

/// Build the redirect policy a guarded client must be configured with.
///
/// Every hop is re-checked, so a destination cannot bounce the engine somewhere it was not
/// allowed to reach directly.
pub fn redirect_policy(policy: Arc<Policy>) -> reqwest::redirect::Policy {
	let max_redirects = policy.max_redirects();

	reqwest::redirect::Policy::custom(move |attempt| {
		if attempt.previous().len() >= max_redirects {
			return attempt.error(BlockReason::TooManyRedirects { max: max_redirects });
		}

		match policy.check_url(attempt.url()) {
			Ok(()) => attempt.follow(),
			Err(reason) => {
				tracing::debug!(url = %attempt.url(), %reason, "blocked outbound redirect");
				attempt.error(reason)
			}
		}
	})
}

/// Recover the [`BlockReason`] that caused a request to fail, if the policy is what stopped it.
///
/// The reason is buried in the source chain of a `reqwest::Error`, so callers that want to tell
/// "the destination is not allowed" apart from "the destination is down" have to walk it.
pub fn block_reason(err: &anyhow::Error) -> Option<BlockReason> {
	err.chain()
		.find_map(|err| err.downcast_ref::<BlockReason>())
		.cloned()
}
