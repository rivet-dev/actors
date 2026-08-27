use std::net::IpAddr;

use rivet_config::config::{Outbound, Root};
use rivet_outbound_guard::{AddressClass, BlockReason, Policy};
use url::Url;

fn policy(outbound: Outbound) -> Policy {
	let config = rivet_config::Config::from_root(Root {
		outbound: Some(outbound),
		..Default::default()
	});

	Policy::from_config(&config).expect("policy should build")
}

fn default_policy() -> Policy {
	policy(Outbound::default())
}

fn check(policy: &Policy, url: &str) -> Result<(), BlockReason> {
	policy.check_url(&Url::parse(url).expect("test url should parse"))
}

fn addr(raw: &str) -> IpAddr {
	raw.parse().expect("test address should parse")
}

#[test]
fn allows_public_destinations() {
	let policy = default_policy();

	check(&policy, "https://runner.example.com/start").expect("public host should be allowed");
	check(&policy, "https://8.8.8.8/start").expect("public literal should be allowed");
	policy
		.check_addr(addr("2606:4700::1"))
		.expect("public v6 should be allowed");
}

#[test]
fn allows_loopback_by_default() {
	let policy = default_policy();

	check(&policy, "http://127.0.0.1:6420/start").expect("loopback v4 should be allowed");
	check(&policy, "http://[::1]:6420/start").expect("loopback v6 should be allowed");
}

#[test]
fn denies_loopback_when_disabled() {
	let policy = policy(Outbound {
		allow_loopback: Some(false),
		..Default::default()
	});

	assert_eq!(
		check(&policy, "http://127.0.0.1:6420/start"),
		Err(BlockReason::BlockedAddress {
			addr: addr("127.0.0.1"),
			class: AddressClass::Loopback,
		}),
	);
}

#[test]
fn denies_private_ranges_by_default() {
	let policy = default_policy();

	for (raw, class) in [
		("10.0.0.5", AddressClass::Private),
		("172.16.4.1", AddressClass::Private),
		("192.168.1.1", AddressClass::Private),
		("169.254.169.254", AddressClass::LinkLocal),
		("100.64.0.1", AddressClass::Shared),
		("198.18.0.1", AddressClass::Benchmarking),
		("0.0.0.0", AddressClass::Unspecified),
		("240.0.0.1", AddressClass::Reserved),
		("fd00::1", AddressClass::UniqueLocal),
		("fe80::1", AddressClass::LinkLocal),
	] {
		assert_eq!(
			policy.check_addr(addr(raw)),
			Err(BlockReason::BlockedAddress {
				addr: addr(raw),
				class,
			}),
			"{raw} should be blocked",
		);
	}
}

#[test]
fn denies_ipv6_wrapped_ipv4_metadata_endpoint() {
	let policy = default_policy();

	// The same destination reached through three different IPv6 encodings.
	for raw in ["::ffff:169.254.169.254", "64:ff9b::169.254.169.254"] {
		assert_eq!(
			policy.check_addr(addr(raw)),
			Err(BlockReason::BlockedAddress {
				addr: addr(raw),
				class: AddressClass::LinkLocal,
			}),
			"{raw} should be blocked",
		);
	}
}

#[test]
fn allows_private_ranges_when_enabled() {
	let policy = policy(Outbound {
		allow_private_networks: Some(true),
		..Default::default()
	});

	policy
		.check_addr(addr("10.0.0.5"))
		.expect("private should be allowed when enabled");
	policy
		.check_addr(addr("169.254.169.254"))
		.expect("link-local should be allowed when enabled");
}

#[test]
fn allow_cidrs_open_a_single_destination() {
	let policy = policy(Outbound {
		allow_cidrs: Some(vec!["10.1.2.0/24".to_string(), "192.168.5.9".to_string()]),
		..Default::default()
	});

	policy
		.check_addr(addr("10.1.2.7"))
		.expect("allow-listed cidr should be allowed");
	policy
		.check_addr(addr("192.168.5.9"))
		.expect("bare address should be read as a single-host cidr");
	assert!(
		policy.check_addr(addr("10.1.3.7")).is_err(),
		"address outside the allow-listed cidr should stay blocked",
	);
}

#[test]
fn deny_cidrs_win_over_allow_rules() {
	let policy = policy(Outbound {
		allow_private_networks: Some(true),
		allow_cidrs: Some(vec!["169.254.0.0/16".to_string()]),
		deny_cidrs: Some(vec!["169.254.169.254".to_string()]),
		..Default::default()
	});

	policy
		.check_addr(addr("169.254.1.1"))
		.expect("rest of the range should still be reachable");
	assert_eq!(
		policy.check_addr(addr("169.254.169.254")),
		Err(BlockReason::DeniedAddress {
			addr: addr("169.254.169.254"),
		}),
	);
}

#[test]
fn deny_cidrs_catch_the_ipv6_wrapped_form() {
	let policy = policy(Outbound {
		allow_private_networks: Some(true),
		deny_cidrs: Some(vec!["169.254.169.254".to_string()]),
		..Default::default()
	});

	assert_eq!(
		policy.check_addr(addr("::ffff:169.254.169.254")),
		Err(BlockReason::DeniedAddress {
			addr: addr("::ffff:169.254.169.254"),
		}),
	);
}

#[test]
fn rejects_non_http_schemes() {
	let policy = default_policy();

	assert_eq!(
		check(&policy, "file:///etc/passwd"),
		Err(BlockReason::UnsupportedScheme {
			scheme: "file".to_string(),
		}),
	);
	assert_eq!(
		check(&policy, "gopher://example.com/"),
		Err(BlockReason::UnsupportedScheme {
			scheme: "gopher".to_string(),
		}),
	);
}

#[test]
fn rejects_plaintext_http_when_disabled() {
	let policy = policy(Outbound {
		allow_insecure_scheme: Some(false),
		..Default::default()
	});

	assert_eq!(
		check(&policy, "http://runner.example.com/"),
		Err(BlockReason::InsecureScheme),
	);
	check(&policy, "https://runner.example.com/").expect("https should still be allowed");
}

#[test]
fn rejects_embedded_credentials() {
	let policy = default_policy();

	assert_eq!(
		check(&policy, "https://user:pass@runner.example.com/"),
		Err(BlockReason::EmbeddedCredentials),
	);
}

#[test]
fn filter_addrs_keeps_only_allowed_answers() {
	let policy = default_policy();

	let allowed = policy
		.filter_addrs(
			"rebind.example.com",
			[addr("10.0.0.1"), addr("93.184.216.34")],
		)
		.expect("a dual answer with one public address should still connect");
	assert_eq!(allowed, vec![addr("93.184.216.34")]);

	assert_eq!(
		policy.filter_addrs(
			"rebind.example.com",
			[addr("10.0.0.1"), addr("192.168.0.1")]
		),
		Err(BlockReason::NoAllowedAddresses {
			host: "rebind.example.com".to_string(),
		}),
	);
}
