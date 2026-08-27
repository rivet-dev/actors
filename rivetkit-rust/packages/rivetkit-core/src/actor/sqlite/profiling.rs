use std::sync::Arc;

use depot_client::vfs::SqliteOperationProfile;
use sha2::{Digest, Sha256};

use crate::SqliteProfilingConfig;

const FINGERPRINT_FORMAT_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatementClass {
	Select,
	Insert,
	Update,
	Delete,
	Ddl,
	Pragma,
	Other,
	Control,
}

impl StatementClass {
	fn label(self) -> &'static str {
		match self {
			Self::Select => "select",
			Self::Insert => "insert",
			Self::Update => "update",
			Self::Delete => "delete",
			Self::Ddl => "ddl",
			Self::Pragma => "pragma",
			Self::Other | Self::Control => "other",
		}
	}
}

#[derive(Clone, Debug)]
pub(super) struct StatementFingerprint {
	pub(super) display: String,
	pub(super) hash: String,
	pub(super) normalized_sql: String,
}

#[derive(Debug)]
pub(super) struct SqliteProfilingState {
	pub(super) config: SqliteProfilingConfig,
	cache: scc::HashMap<[u8; 32], Arc<StatementFingerprint>>,
	cataloged: scc::HashSet<String>,
}

impl Default for SqliteProfilingState {
	fn default() -> Self {
		Self::new(SqliteProfilingConfig::default())
	}
}

impl SqliteProfilingState {
	pub(super) fn new(config: SqliteProfilingConfig) -> Self {
		Self {
			config,
			cache: scc::HashMap::new(),
			cataloged: scc::HashSet::new(),
		}
	}

	pub(super) fn statement_fingerprint(&self, sql: &str) -> Option<Arc<StatementFingerprint>> {
		if !self.config.enabled {
			return None;
		}
		if sql.len() > self.config.max_sql_bytes_to_normalize {
			return Some(Arc::new(StatementFingerprint {
				display: "other".to_owned(),
				hash: "other".to_owned(),
				normalized_sql: format!("<sql-too-long:{}-bytes>", sql.len()),
			}));
		}

		let raw_digest: [u8; 32] = Sha256::digest(sql.as_bytes()).into();
		if let Some(cached) = self
			.cache
			.read_sync(&raw_digest, |_, value| Arc::clone(value))
		{
			return Some(cached);
		}

		let (class, normalized_sql) = normalize_sql(sql);
		if class == StatementClass::Control || normalized_sql.is_empty() {
			return None;
		}
		let hash = fingerprint_hash(b"rivetkit-sqlite-statement", normalized_sql.as_bytes());
		let fingerprint = Arc::new(StatementFingerprint {
			display: format!("{}-{hash}", class.label()),
			hash,
			normalized_sql: truncate_utf8(normalized_sql, self.config.max_catalog_sql_shape_bytes),
		});
		if self.cache.len() < self.config.fingerprint_computation_cache_entries {
			let _ = self.cache.insert_sync(raw_digest, Arc::clone(&fingerprint));
		}
		Some(fingerprint)
	}

	pub(super) fn mark_cataloged(&self, fingerprint: &str) -> bool {
		self.cataloged.insert_sync(fingerprint.to_owned()).is_ok()
	}
}

#[derive(Clone, Debug)]
pub(super) struct StatementObservation {
	pub(super) fingerprint: Arc<StatementFingerprint>,
	pub(super) total_ns: u64,
	pub(super) transaction_wait_ns: u64,
	pub(super) profile: SqliteOperationProfile,
}

#[derive(Debug)]
pub(super) struct TransactionProfile {
	pub(super) started_at: crate::time::Instant,
	pub(super) transaction_wait_ns: u64,
	pub(super) fingerprint: Option<String>,
	pub(super) name: Option<String>,
	shape: Sha256,
	pub(super) statement_fingerprint_hashes:
		[Option<[u8; 16]>; depot_client::vfs::MAX_PROFILED_TRANSACTION_STATEMENTS],
	pub(super) omitted_statement_fingerprints: u64,
	statement_fingerprint_limit: usize,
	pub(super) statement_count: u64,
	pub(super) worker_wait_ns: u64,
	pub(super) storage_ns: u64,
	pub(super) local_work_ns: u64,
	pub(super) get_pages_round_trips: u64,
	pub(super) commit_round_trips: u64,
	pub(super) dirty_pages: u64,
	pub(super) dirty_bytes: u64,
	pub(super) commit_ns: u64,
}

impl TransactionProfile {
	pub(super) fn new(
		name: Option<String>,
		started_at: crate::time::Instant,
		statement_fingerprint_limit: usize,
	) -> Self {
		let fingerprint = name.as_deref().map(|name| {
			format!(
				"txn-{}",
				fingerprint_hash(b"rivetkit-sqlite-transaction", name.as_bytes())
			)
		});
		let mut shape = Sha256::new();
		shape.update(b"rivetkit-sqlite-transaction-shape");
		shape.update([FINGERPRINT_FORMAT_VERSION]);
		Self {
			started_at,
			transaction_wait_ns: 0,
			fingerprint,
			name,
			shape,
			statement_fingerprint_hashes: [None;
				depot_client::vfs::MAX_PROFILED_TRANSACTION_STATEMENTS],
			omitted_statement_fingerprints: 0,
			statement_fingerprint_limit: statement_fingerprint_limit
				.min(depot_client::vfs::MAX_PROFILED_TRANSACTION_STATEMENTS),
			statement_count: 0,
			worker_wait_ns: 0,
			storage_ns: 0,
			local_work_ns: 0,
			get_pages_round_trips: 0,
			commit_round_trips: 0,
			dirty_pages: 0,
			dirty_bytes: 0,
			commit_ns: 0,
		}
	}

	pub(super) fn record_control(
		&mut self,
		profile: &SqliteOperationProfile,
		duration_ns: u64,
		is_commit: bool,
	) {
		self.worker_wait_ns = self.worker_wait_ns.saturating_add(profile.worker_wait_ns);
		self.storage_ns = self.storage_ns.saturating_add(profile.storage_ns);
		self.local_work_ns = self.local_work_ns.saturating_add(
			duration_ns
				.saturating_sub(profile.worker_wait_ns)
				.saturating_sub(profile.storage_ns),
		);
		self.get_pages_round_trips = self
			.get_pages_round_trips
			.saturating_add(profile.get_pages_round_trips);
		self.commit_round_trips = self
			.commit_round_trips
			.saturating_add(profile.commit_round_trips);
		self.dirty_pages = self.dirty_pages.saturating_add(profile.dirty_pages);
		self.dirty_bytes = self.dirty_bytes.saturating_add(profile.dirty_bytes);
		if is_commit {
			self.commit_ns = duration_ns;
		}
	}

	pub(super) fn record_statement(&mut self, observation: &StatementObservation) {
		self.shape.update(observation.fingerprint.hash.as_bytes());
		self.shape.update([0]);
		let statement_index = self.statement_count as usize;
		if statement_index < self.statement_fingerprint_limit {
			let mut hash = [0; 16];
			let source = observation.fingerprint.hash.as_bytes();
			let copy_len = source.len().min(hash.len());
			hash[..copy_len].copy_from_slice(&source[..copy_len]);
			self.statement_fingerprint_hashes[statement_index] = Some(hash);
		} else {
			self.omitted_statement_fingerprints =
				self.omitted_statement_fingerprints.saturating_add(1);
		}
		self.statement_count = self.statement_count.saturating_add(1);
		self.transaction_wait_ns = self
			.transaction_wait_ns
			.saturating_add(observation.transaction_wait_ns);
		self.worker_wait_ns = self
			.worker_wait_ns
			.saturating_add(observation.profile.worker_wait_ns);
		self.storage_ns = self
			.storage_ns
			.saturating_add(observation.profile.storage_ns);
		self.local_work_ns = self.local_work_ns.saturating_add(
			observation
				.total_ns
				.saturating_sub(observation.transaction_wait_ns)
				.saturating_sub(observation.profile.worker_wait_ns)
				.saturating_sub(observation.profile.storage_ns),
		);
		self.get_pages_round_trips = self
			.get_pages_round_trips
			.saturating_add(observation.profile.get_pages_round_trips);
		self.commit_round_trips = self
			.commit_round_trips
			.saturating_add(observation.profile.commit_round_trips);
		self.dirty_pages = self
			.dirty_pages
			.saturating_add(observation.profile.dirty_pages);
		self.dirty_bytes = self
			.dirty_bytes
			.saturating_add(observation.profile.dirty_bytes);
	}

	pub(super) fn fingerprint(&self) -> (String, &'static str) {
		if let Some(fingerprint) = &self.fingerprint {
			return (fingerprint.clone(), "name");
		}
		let digest = self.shape.clone().finalize();
		(format!("shape-{}", hex_prefix(&digest)), "shape")
	}

	pub(super) fn shape_fingerprint(&self) -> String {
		format!("shape-{}", hex_prefix(&self.shape.clone().finalize()))
	}
}

fn fingerprint_hash(domain: &[u8], value: &[u8]) -> String {
	let mut hasher = Sha256::new();
	hasher.update(domain);
	hasher.update([0, FINGERPRINT_FORMAT_VERSION, 0]);
	hasher.update(value);
	hex_prefix(&hasher.finalize())
}

fn hex_prefix(bytes: &[u8]) -> String {
	let mut output = String::with_capacity(16);
	for byte in bytes.iter().take(8) {
		use std::fmt::Write;
		let _ = write!(output, "{byte:02x}");
	}
	output
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
	if value.len() <= max_bytes {
		return value;
	}
	let mut end = max_bytes;
	while !value.is_char_boundary(end) {
		end -= 1;
	}
	value.truncate(end);
	value
}

fn normalize_sql(sql: &str) -> (StatementClass, String) {
	let tokens = tokenize(sql);
	let class = classify_tokens(&tokens);
	(class, tokens.join(" "))
}

fn classify_tokens(tokens: &[String]) -> StatementClass {
	let Some(mut index) = tokens.iter().position(|token| token != ";") else {
		return StatementClass::Other;
	};
	if tokens[index] == "explain" {
		index += 1;
		if tokens.get(index).is_some_and(|token| token == "query") {
			index = index.saturating_add(2);
		}
	}
	if tokens.get(index).is_some_and(|token| token == "with") {
		let mut depth = 0_i32;
		for (offset, token) in tokens[index + 1..].iter().enumerate() {
			match token.as_str() {
				"(" => depth += 1,
				")" => depth = depth.saturating_sub(1),
				"select" | "insert" | "update" | "delete" if depth == 0 => {
					index += offset + 1;
					break;
				}
				_ => {}
			}
		}
	}

	match tokens.get(index).map(String::as_str) {
		Some("select" | "values") => StatementClass::Select,
		Some("insert" | "replace") => StatementClass::Insert,
		Some("update") => StatementClass::Update,
		Some("delete") => StatementClass::Delete,
		Some(
			"create" | "alter" | "drop" | "vacuum" | "reindex" | "analyze" | "attach" | "detach",
		) => StatementClass::Ddl,
		Some("pragma") => StatementClass::Pragma,
		Some("begin" | "commit" | "end" | "rollback" | "savepoint" | "release") => {
			StatementClass::Control
		}
		_ => StatementClass::Other,
	}
}

/// SQLite-oriented lexical normalization. Literal and bind tokens become `?`,
/// comments disappear, unquoted keywords/identifiers are case-folded, and
/// token spacing is canonical. This is deliberately a tokenizer rather than a
/// regex so quoted text and comments cannot leak into the catalog.
fn tokenize(sql: &str) -> Vec<String> {
	let bytes = sql.as_bytes();
	let mut tokens = Vec::new();
	let mut index = 0;
	while index < bytes.len() {
		let byte = bytes[index];
		if byte.is_ascii_whitespace() {
			index += 1;
			continue;
		}
		if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
			index += 2;
			while index < bytes.len() && bytes[index] != b'\n' {
				index += 1;
			}
			continue;
		}
		if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
			index += 2;
			while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
				index += 1;
			}
			index = (index + 2).min(bytes.len());
			continue;
		}
		if byte == b'\'' {
			index = skip_quoted(bytes, index, b'\'', b'\'');
			if tokens.last().is_some_and(|token| token == "x") {
				tokens.pop();
			}
			tokens.push("?".to_owned());
			continue;
		}
		if matches!(byte, b'"' | b'`' | b'[') {
			let end = if byte == b'[' { b']' } else { byte };
			let start = index;
			index = skip_quoted(bytes, index, end, byte);
			tokens.push(String::from_utf8_lossy(&bytes[start..index]).into_owned());
			continue;
		}
		if byte.is_ascii_digit()
			|| (byte == b'.' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
		{
			index = skip_numeric_literal(bytes, index);
			tokens.push("?".to_owned());
			continue;
		}
		if matches!(byte, b'?' | b':' | b'@' | b'$') {
			index += 1;
			while index < bytes.len()
				&& (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
			{
				index += 1;
			}
			tokens.push("?".to_owned());
			continue;
		}
		if byte.is_ascii_alphabetic() || byte == b'_' {
			let start = index;
			index += 1;
			while index < bytes.len()
				&& (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
			{
				index += 1;
			}
			tokens.push(String::from_utf8_lossy(&bytes[start..index]).to_ascii_lowercase());
			continue;
		}

		let start = index;
		index += 1;
		if index < bytes.len()
			&& matches!(
				(byte, bytes[index]),
				(b'<', b'=')
					| (b'>', b'=') | (b'!', b'=')
					| (b'<', b'>') | (b'=', b'=')
					| (b'|', b'|') | (b'-', b'>')
			) {
			index += 1;
		}
		tokens.push(String::from_utf8_lossy(&bytes[start..index]).into_owned());
	}
	tokens
}

fn skip_numeric_literal(bytes: &[u8], mut index: usize) -> usize {
	if bytes.get(index) == Some(&b'0') && matches!(bytes.get(index + 1), Some(b'x' | b'X')) {
		index += 2;
		while index < bytes.len() && (bytes[index].is_ascii_hexdigit() || bytes[index] == b'_') {
			index += 1;
		}
		return index;
	}

	while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
		index += 1;
	}
	if bytes.get(index) == Some(&b'.') {
		index += 1;
		while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
			index += 1;
		}
	}
	if matches!(bytes.get(index), Some(b'e' | b'E')) {
		let exponent = index;
		index += 1;
		if matches!(bytes.get(index), Some(b'+' | b'-')) {
			index += 1;
		}
		let digits = index;
		while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
			index += 1;
		}
		if index == digits {
			index = exponent;
		}
	}
	index
}

fn skip_quoted(bytes: &[u8], mut index: usize, end: u8, escape: u8) -> usize {
	index += 1;
	while index < bytes.len() {
		if bytes[index] == end {
			if bytes.get(index + 1) == Some(&escape) {
				index += 2;
				continue;
			}
			return index + 1;
		}
		index += 1;
	}
	index
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn literals_comments_and_formatting_share_a_fingerprint() {
		let state = SqliteProfilingState::default();
		let first = state
			.statement_fingerprint("SELECT * FROM orders WHERE id = 1 AND note = 'secret'")
			.unwrap();
		let second = state
			.statement_fingerprint(
				" -- comment\n select * from orders where id=99 and note='other'",
			)
			.unwrap();
		assert_eq!(first.display, second.display);
		assert!(!first.normalized_sql.contains("secret"));
		assert!(!second.normalized_sql.contains("other"));
	}

	#[test]
	fn cte_uses_the_underlying_statement_class() {
		let state = SqliteProfilingState::default();
		let fingerprint = state
			.statement_fingerprint(
				"WITH ids AS (SELECT id FROM jobs) UPDATE jobs SET done=1 WHERE id IN (SELECT id FROM ids)",
			)
			.unwrap();
		assert!(fingerprint.display.starts_with("update-"));
	}

	#[test]
	fn transaction_control_is_not_profiled() {
		let state = SqliteProfilingState::default();
		assert!(state.statement_fingerprint("BEGIN").is_none());
		assert!(state.statement_fingerprint("ROLLBACK").is_none());
	}

	#[test]
	fn numeric_literals_do_not_consume_arithmetic_operators() {
		let tokens = tokenize("SELECT 1-2, 3+4, 5e-2, 0x10");
		assert_eq!(
			tokens,
			[
				"select", "?", "-", "?", ",", "?", "+", "?", ",", "?", ",", "?"
			]
		);
	}

	#[test]
	fn named_transactions_keep_stable_identity_and_bounded_shape_details() {
		let state = SqliteProfilingState::default();
		let fingerprint = state.statement_fingerprint("SELECT 1").unwrap();
		let observation = StatementObservation {
			fingerprint,
			total_ns: 1,
			transaction_wait_ns: 0,
			profile: Default::default(),
		};
		let mut first = TransactionProfile::new(
			Some("process-order".to_owned()),
			crate::time::Instant::now(),
			1,
		);
		first.record_statement(&observation);
		let mut second = TransactionProfile::new(
			Some("process-order".to_owned()),
			crate::time::Instant::now(),
			1,
		);
		second.record_statement(&observation);
		second.record_statement(&observation);

		assert_eq!(first.fingerprint(), second.fingerprint());
		assert_ne!(first.shape_fingerprint(), second.shape_fingerprint());
		assert_eq!(second.omitted_statement_fingerprints, 1);
		assert!(std::mem::size_of::<TransactionProfile>() < 1024);
	}

	#[test]
	fn oversized_sql_uses_bounded_transaction_shape_details() {
		let state = SqliteProfilingState::new(SqliteProfilingConfig {
			max_sql_bytes_to_normalize: 4,
			..Default::default()
		});
		let fingerprint = state.statement_fingerprint("SELECT 1").unwrap();
		assert_eq!(fingerprint.display, "other");

		let observation = StatementObservation {
			fingerprint,
			total_ns: 1,
			transaction_wait_ns: 0,
			profile: Default::default(),
		};
		let mut transaction = TransactionProfile::new(
			Some("oversized-sql".to_owned()),
			crate::time::Instant::now(),
			1,
		);
		transaction.record_statement(&observation);

		assert_eq!(
			transaction.statement_fingerprint_hashes[0]
				.as_ref()
				.unwrap()
				.get(..5),
			Some(b"other".as_slice())
		);
	}
}
