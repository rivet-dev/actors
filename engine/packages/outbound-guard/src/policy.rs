use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::{Context, Result};
use ipnet::IpNet;
use url::Url;

/// Why a destination was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockReason {
	#[error("url is not a valid absolute url")]
	InvalidUrl,
	#[error("scheme {scheme:?} is not allowed, expected http or https")]
	UnsupportedScheme { scheme: String },
	#[error("plaintext http is not allowed, use https")]
	InsecureScheme,
	#[error("url must not contain embedded credentials")]
	EmbeddedCredentials,
	#[error("url has no host")]
	MissingHost,
	#[error("address {addr} is not an allowed destination ({class})")]
	BlockedAddress { addr: IpAddr, class: AddressClass },
	#[error("address {addr} is explicitly denied")]
	DeniedAddress { addr: IpAddr },
	#[error("host {host:?} resolved to no allowed addresses")]
	NoAllowedAddresses { host: String },
	#[error("failed to resolve host {host:?}")]
	ResolutionFailed { host: String },
	#[error("exceeded the maximum of {max} redirects")]
	TooManyRedirects { max: usize },
}

/// The reason an address is not globally routable.
///
/// Every class except [`AddressClass::Loopback`] is governed by
/// `outbound.allow_private_networks`. An address that matches no class is globally
/// routable and always allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressClass {
	/// `0.0.0.0` or `::`, which the kernel routes to a local interface.
	Unspecified,
	/// `127.0.0.0/8` or `::1`.
	Loopback,
	/// `169.254.0.0/16` or `fe80::/10`. Covers the cloud metadata endpoint.
	LinkLocal,
	/// `10/8`, `172.16/12`, `192.168/16`.
	Private,
	/// `255.255.255.255`.
	Broadcast,
	/// `224.0.0.0/4` or `ff00::/8`.
	Multicast,
	/// `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, or `2001:db8::/32`.
	Documentation,
	/// `100.64.0.0/10`, the carrier-grade NAT range.
	Shared,
	/// `192.0.0.0/24`, reserved for IETF protocol assignments.
	ProtocolAssignments,
	/// `198.18.0.0/15`, reserved for network benchmarking.
	Benchmarking,
	/// `240.0.0.0/4`.
	Reserved,
	/// `fc00::/7`, the IPv6 equivalent of the private ranges.
	UniqueLocal,
	/// `100::/64`, which is discarded rather than routed.
	Discard,
	/// An IPv6 address carrying an IPv4 destination that is itself globally routable.
	///
	/// These are held to the same rule as the private classes because they are an easy way to
	/// smuggle a destination past a filter that only understands one address family.
	Ipv4Embedded,
}

impl fmt::Display for AddressClass {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let name = match self {
			AddressClass::Unspecified => "unspecified",
			AddressClass::Loopback => "loopback",
			AddressClass::LinkLocal => "link-local",
			AddressClass::Private => "private",
			AddressClass::Broadcast => "broadcast",
			AddressClass::Multicast => "multicast",
			AddressClass::Documentation => "documentation",
			AddressClass::Shared => "shared",
			AddressClass::ProtocolAssignments => "protocol-assignments",
			AddressClass::Benchmarking => "benchmarking",
			AddressClass::Reserved => "reserved",
			AddressClass::UniqueLocal => "unique-local",
			AddressClass::Discard => "discard",
			AddressClass::Ipv4Embedded => "ipv4-embedded",
		};

		f.write_str(name)
	}
}

impl AddressClass {
	/// Classify an address, or `None` if it is globally routable.
	pub fn of(addr: IpAddr) -> Option<AddressClass> {
		match addr {
			IpAddr::V4(v4) => AddressClass::of_ipv4(v4),
			IpAddr::V6(v6) => AddressClass::of_ipv6(v6),
		}
	}

	fn of_ipv4(addr: Ipv4Addr) -> Option<AddressClass> {
		let [a, b, c, _] = addr.octets();

		if addr.is_unspecified() {
			return Some(AddressClass::Unspecified);
		}
		if addr.is_loopback() {
			return Some(AddressClass::Loopback);
		}
		if addr.is_link_local() {
			return Some(AddressClass::LinkLocal);
		}
		if addr.is_private() {
			return Some(AddressClass::Private);
		}
		if addr.is_broadcast() {
			return Some(AddressClass::Broadcast);
		}
		if addr.is_multicast() {
			return Some(AddressClass::Multicast);
		}
		if addr.is_documentation() {
			return Some(AddressClass::Documentation);
		}
		if a == 100 && (64..128).contains(&b) {
			return Some(AddressClass::Shared);
		}
		if a == 192 && b == 0 && c == 0 {
			return Some(AddressClass::ProtocolAssignments);
		}
		if a == 198 && (b == 18 || b == 19) {
			return Some(AddressClass::Benchmarking);
		}
		if a >= 240 {
			return Some(AddressClass::Reserved);
		}

		None
	}

	fn of_ipv6(addr: Ipv6Addr) -> Option<AddressClass> {
		// An IPv4 address wearing an IPv6 costume routes to the embedded IPv4 destination, so
		// classify it as that address rather than trusting the outer form.
		if let Some(v4) = unwrap_embedded_ipv4(addr) {
			return AddressClass::of_ipv4(v4).or(Some(AddressClass::Ipv4Embedded));
		}

		let segments = addr.segments();

		if addr.is_unspecified() {
			return Some(AddressClass::Unspecified);
		}
		if addr.is_loopback() {
			return Some(AddressClass::Loopback);
		}
		if addr.is_multicast() {
			return Some(AddressClass::Multicast);
		}
		if segments[0] & 0xfe00 == 0xfc00 {
			return Some(AddressClass::UniqueLocal);
		}
		if segments[0] & 0xffc0 == 0xfe80 {
			return Some(AddressClass::LinkLocal);
		}
		if segments[0] == 0x2001 && segments[1] == 0x0db8 {
			return Some(AddressClass::Documentation);
		}
		if segments[0] == 0x0100 && segments[1..4] == [0, 0, 0] {
			return Some(AddressClass::Discard);
		}

		None
	}
}

/// Extract the IPv4 destination an IPv6 address actually routes to, if any.
///
/// Covers IPv4-mapped (`::ffff:0:0/96`), IPv4-compatible (`::/96`), and the well-known NAT64
/// prefix (`64:ff9b::/96`).
fn unwrap_embedded_ipv4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
	if let Some(v4) = addr.to_ipv4_mapped() {
		return Some(v4);
	}

	let segments = addr.segments();
	let tail = Ipv4Addr::new(
		(segments[6] >> 8) as u8,
		(segments[6] & 0xff) as u8,
		(segments[7] >> 8) as u8,
		(segments[7] & 0xff) as u8,
	);

	if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6] == [0, 0, 0, 0] {
		return Some(tail);
	}

	// IPv4-compatible addresses are deprecated but still routed. `::` and `::1` have their own
	// classes, so skip anything in the lowest /104.
	if segments[0..6] == [0, 0, 0, 0, 0, 0] && segments[6] != 0 {
		return Some(tail);
	}

	None
}

/// Which destinations the engine may reach on behalf of a user-supplied URL.
#[derive(Debug, Clone)]
pub struct Policy {
	allow_loopback: bool,
	allow_private_networks: bool,
	allow_insecure_scheme: bool,
	allow_cidrs: Vec<IpNet>,
	deny_cidrs: Vec<IpNet>,
	max_redirects: usize,
}

impl Policy {
	pub fn from_config(config: &rivet_config::Config) -> Result<Self> {
		let outbound = config.outbound();

		Ok(Policy {
			allow_loopback: outbound.allow_loopback(),
			allow_private_networks: outbound.allow_private_networks(),
			allow_insecure_scheme: outbound.allow_insecure_scheme(),
			allow_cidrs: parse_cidrs(outbound.allow_cidrs(), "outbound.allow_cidrs")?,
			deny_cidrs: parse_cidrs(outbound.deny_cidrs(), "outbound.deny_cidrs")?,
			max_redirects: outbound.max_redirects(),
		})
	}

	pub fn max_redirects(&self) -> usize {
		self.max_redirects
	}

	/// Check everything about a destination that can be known without resolving DNS.
	///
	/// This is the gate that runs when a runner config is written, so a bad URL is rejected before
	/// it is ever stored. It is also re-run on every redirect hop.
	pub fn check_url(&self, url: &Url) -> Result<(), BlockReason> {
		match url.scheme() {
			"https" => {}
			"http" => {
				if !self.allow_insecure_scheme {
					return Err(BlockReason::InsecureScheme);
				}
			}
			scheme => {
				return Err(BlockReason::UnsupportedScheme {
					scheme: scheme.to_string(),
				});
			}
		}

		// Credentials in the URL would be replayed to whatever the destination redirects to.
		if !url.username().is_empty() || url.password().is_some() {
			return Err(BlockReason::EmbeddedCredentials);
		}

		let Some(host) = url.host() else {
			return Err(BlockReason::MissingHost);
		};

		// A hostname is checked once it resolves, at connect time. An address literal skips the
		// resolver entirely, so it has to be checked here.
		match host {
			url::Host::Ipv4(addr) => self.check_addr(IpAddr::V4(addr)),
			url::Host::Ipv6(addr) => self.check_addr(IpAddr::V6(addr)),
			url::Host::Domain(_) => Ok(()),
		}
	}

	/// Check a single resolved address.
	pub fn check_addr(&self, addr: IpAddr) -> Result<(), BlockReason> {
		// An IPv6 address that carries an IPv4 destination has to match CIDR rules written for
		// either form, so both are tested.
		let mut forms = vec![addr];
		if let IpAddr::V6(v6) = addr {
			if let Some(v4) = unwrap_embedded_ipv4(v6) {
				forms.push(IpAddr::V4(v4));
			}
		}

		// An explicit deny always wins, including over the allow list.
		if forms
			.iter()
			.any(|form| self.deny_cidrs.iter().any(|net| net.contains(form)))
		{
			return Err(BlockReason::DeniedAddress { addr });
		}

		if forms
			.iter()
			.any(|form| self.allow_cidrs.iter().any(|net| net.contains(form)))
		{
			return Ok(());
		}

		let Some(class) = AddressClass::of(addr) else {
			return Ok(());
		};

		let allowed = match class {
			AddressClass::Loopback => self.allow_loopback,
			AddressClass::Unspecified
			| AddressClass::LinkLocal
			| AddressClass::Private
			| AddressClass::Broadcast
			| AddressClass::Multicast
			| AddressClass::Documentation
			| AddressClass::Shared
			| AddressClass::ProtocolAssignments
			| AddressClass::Benchmarking
			| AddressClass::Reserved
			| AddressClass::UniqueLocal
			| AddressClass::Discard
			| AddressClass::Ipv4Embedded => self.allow_private_networks,
		};

		if allowed {
			Ok(())
		} else {
			Err(BlockReason::BlockedAddress { addr, class })
		}
	}

	/// Filter a resolver's answer down to the addresses this policy permits.
	///
	/// Dropping individual addresses rather than rejecting the whole answer keeps a dual-stack
	/// host reachable when only one of its families is allowed, and still guarantees the
	/// connection can only land on an address that passed.
	pub fn filter_addrs(
		&self,
		host: &str,
		addrs: impl IntoIterator<Item = IpAddr>,
	) -> Result<Vec<IpAddr>, BlockReason> {
		let mut allowed = Vec::new();

		for addr in addrs {
			match self.check_addr(addr) {
				Ok(()) => allowed.push(addr),
				Err(reason) => {
					tracing::debug!(%host, %addr, %reason, "dropped disallowed resolved address");
				}
			}
		}

		if allowed.is_empty() {
			Err(BlockReason::NoAllowedAddresses {
				host: host.to_string(),
			})
		} else {
			Ok(allowed)
		}
	}
}

fn parse_cidrs(raw: &[String], label: &str) -> Result<Vec<IpNet>> {
	raw.iter()
		.map(|entry| {
			let entry = entry.trim();
			// Accept a bare address as a single-host CIDR so operators do not have to write /32.
			if let Ok(addr) = entry.parse::<IpAddr>() {
				return Ok(IpNet::from(addr));
			}

			entry
				.parse::<IpNet>()
				.with_context(|| format!("invalid cidr in {label}: {entry:?}"))
		})
		.collect()
}
