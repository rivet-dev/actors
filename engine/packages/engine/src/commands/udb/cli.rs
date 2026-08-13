use std::{
	result::Result::{Err, Ok},
	time::Duration,
};

use anyhow::Context;
use clap::{Parser, ValueEnum};
use futures_util::{StreamExt, TryStreamExt};
use rivet_pools::UdbPool;
use rivet_term::console::style;
use universaldb::prelude::*;

use super::key_parser;
use crate::util::{
	format::indent_string,
	udb::{ListStyle, SimpleTuple, SimpleTupleValue},
};

// TODO: Tab completion
#[derive(Parser)]
#[command(name = "")]
pub enum SubCommand {
	/// Change current key.
	#[command(name = "cd")]
	ChangeKey {
		/// Key path to change to. Supports relative key paths.
		key: String,
	},

	/// Get value at current key.
	#[command(name = "get")]
	Get {
		/// Key path to get. Supports relative key paths.
		key: Option<String>,

		/// Optional type hint for value parsing.
		#[arg(short = 't', long = "type")]
		type_hint: Option<String>,
	},

	/// List all keys under the current key.
	#[command(name = "ls")]
	List {
		/// Key path to list. Supports relative key paths.
		key: Option<String>,

		/// Max depth of keys shown.
		#[arg(short = 'd', long, default_value_t = 1)]
		max_depth: usize,
		/// Whether or not to hide subspace keys which are past the max depth.
		#[arg(short = 'u', long)]
		hide_subspaces: bool,
		/// Print style
		#[arg(short = 's', long, default_value = "tree")]
		style: ListStyle,

		/// Max entries to return.
		#[arg(short = 'l', long)]
		limit: Option<usize>,

		/// Lists all entries after the current key, not just under it.
		#[arg(short = 'o', long)]
		open: bool,

		/// Print string as bytes instead.
		#[arg(long = "prefer-bytes")]
		prefer_bytes: bool,
	},

	/// Calculates or estimates the size of all data under the current key.
	#[command(name = "size")]
	Size {
		/// Key path to list. Supports relative key paths.
		key: Option<String>,
		/// Whether or not to scan the entire subspace and calculate the size manually.
		#[arg(short = 's', long)]
		scan: bool,
	},

	/// Move single key or entire subspace from A to B.
	#[command(name = "move")]
	Move {
		/// Old key path. Supports relative key paths.
		old_key: String,

		/// New key path. Supports relative key paths.
		new_key: String,

		/// Move all keys under the given key instead of just the given key.
		#[arg(short = 'r', long)]
		recursive: bool,
	},

	/// Set value at current path.
	#[command(name = "set")]
	Set {
		/// Key path to set. Supports relative key paths. Used as value if value is not set.
		key_or_value: String,

		/// Value to set, with optional type prefix (e.g. "u64:123", overrides type hint).
		value: Option<String>,

		/// Optional type hint for the value.
		#[arg(short = 't', long)]
		type_hint: Option<String>,
	},

	/// Clear value or range at current path.
	#[command(name = "clear")]
	Clear {
		/// Key path to clear. Supports relative key paths.
		key: Option<String>,

		/// Clears the entire subspace range instead of just the key.
		#[arg(short = 'r', long = "range")]
		clear_range: bool,
		/// Disable confirmation prompt.
		#[arg(short = 'y', long)]
		yes: bool,
	},

	#[command(name = "exit")]
	Exit,

	Oneoff {
		#[clap(subcommand)]
		command: OneoffSubCommand,
	},
}

#[derive(Parser)]
pub enum OneoffSubCommand {
	#[command(name = "reapply-lost-serverless")]
	ReapplyLostServerless {
		#[arg(short = 'f')]
		flip: bool,
		#[arg(long)]
		dry_run: bool,
	},
	/// Re-apply epoxy v2 changelog entries that were left mixed with v3 entries due
	/// to a missing version gate. Iterates the per-replica changelog subspace in
	/// chunks, attempts to deserialize each entry as the v2 schema with a
	/// byte-identical re-serialize roundtrip, and applies any entry that succeeds
	/// via the standard catchup path inside the same transaction. Entries that
	/// fail the roundtrip are assumed to be v3 and ignored.
	#[command(name = "repair-epoxy-changelog")]
	RepairEpoxyChangelog {
		/// Replica id whose changelog subspace will be scanned.
		replica_id: u64,
		#[arg(long)]
		dry_run: bool,
	},
}

impl SubCommand {
	pub async fn execute(
		self,
		pool: &UdbPool,
		previous_tuple: &mut SimpleTuple,
		current_tuple: &mut SimpleTuple,
	) -> CommandResult {
		match self {
			SubCommand::ChangeKey { key } => {
				if key.trim() == "-" {
					let other = current_tuple.clone();
					*current_tuple = previous_tuple.clone();
					*previous_tuple = other;
				} else {
					*previous_tuple = current_tuple.clone();
					update_current_tuple(current_tuple, Some(key));
				}
			}
			SubCommand::Get { key, type_hint } => {
				let mut current_tuple = current_tuple.clone();
				if update_current_tuple(&mut current_tuple, key) {
					return CommandResult::Error;
				}

				let fut = pool.txn("udb_cli_get", |tx| {
					let current_tuple = current_tuple.clone();
					async move {
						let key = universaldb::tuple::pack(&current_tuple);
						let entry = tx.get(&key, Snapshot).await?;
						Ok(entry)
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok(entry)) => {
						if let Some(entry) = entry {
							if type_hint.as_deref() == Some("raw") {
								println!("{}", SimpleTupleValue::Unknown(entry.to_vec()));
							} else {
								match SimpleTupleValue::deserialize(type_hint.as_deref(), &entry) {
									Ok(parsed) => {
										let mut s = String::new();
										parsed.write(&mut s, false).unwrap();
										println!("{s}");
									}
									Err(err) => println!("error: {err:#}"),
								}
							}
						} else {
							println!("key does not exist");
						};
					}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::List {
				key,
				max_depth,
				hide_subspaces,
				style: list_style,
				limit,
				open,
				prefer_bytes,
			} => {
				let mut current_tuple = current_tuple.clone();
				if update_current_tuple(&mut current_tuple, key) {
					return CommandResult::Error;
				}

				let subspace = universaldb::tuple::Subspace::all().subspace(&current_tuple);
				let range = subspace.range();
				let start = range.0;

				let (subspace, end) = if open {
					let mut parent_tuple = current_tuple.clone();
					parent_tuple.segments.pop();
					let subspace = universaldb::tuple::Subspace::all().subspace(&parent_tuple);

					let end = subspace.range().1;

					(subspace, end)
				} else {
					(subspace, range.1)
				};

				let fut = pool.txn("udb_cli_list", |tx| {
					let subspace = &subspace;
					let start = start.as_slice();
					let end = end.as_slice();
					async move {
						let mut ctx = ListRenderContext {
							entry_count: 0,
							subspace_count: 0,
							current_hidden_subspace: None,
							hidden_count: 0,
							last_key: SimpleTuple::new(),
						};

						let mut stream = tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								limit,
								..(start, end).into()
							},
							Snapshot,
						);
						let signal = tokio::signal::ctrl_c();
						tokio::pin!(signal);

						loop {
							tokio::select! {
								res = stream.try_next() => {
									let Some(entry) = res? else {
										break;
									};

									render_list_entry(
										max_depth,
										hide_subspaces,
										list_style,
										prefer_bytes,
										subspace,
										&mut ctx,
										entry,
									);
								}
								_ = &mut signal => {
									break;
								}
							}
						}

						if !hide_subspaces {
							if let Some(curr) = ctx.current_hidden_subspace {
								curr.print(&list_style, &ctx.last_key);
								println!(
									"/ {}",
									style(format!(
										"{} {}",
										ctx.hidden_count,
										if ctx.hidden_count == 1 {
											"entry"
										} else {
											"entries"
										}
									))
									.dim()
								);
							}
						}

						if ctx.entry_count != 0 {
							println!();
						}

						print!(
							"{} {}",
							ctx.entry_count,
							if ctx.entry_count == 1 {
								"entry"
							} else {
								"entries"
							}
						);

						if ctx.subspace_count != 0 {
							print!(
								", {} {} ({} total entries)",
								ctx.subspace_count,
								if ctx.subspace_count == 1 {
									"subspace"
								} else {
									"subspaces"
								},
								ctx.entry_count
							);
						}

						println!();

						Ok(())
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok(())) => {}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::Size { key, scan } => {
				let mut current_tuple = current_tuple.clone();
				if update_current_tuple(&mut current_tuple, key) {
					return CommandResult::Error;
				}

				let subspace = universaldb::tuple::Subspace::all().subspace(&current_tuple);

				let fut = pool.txn("udb_cli_size", |tx| {
					let subspace = subspace.clone();
					async move {
						let (start, end) = subspace.range();

						let estimate_bytes =
							tx.get_estimated_range_size_bytes(&start, &end).await?;

						let exact_bytes = if scan {
							let exact_bytes = tx
								.get_ranges_keyvalues(
									universaldb::RangeOption {
										mode: StreamingMode::WantAll,
										..(&subspace).into()
									},
									Snapshot,
								)
								.try_fold(0, |s, entry| async move {
									Ok(s + entry.key().len() + entry.value().len())
								})
								.await?;

							Some(exact_bytes as i64)
						} else {
							None
						};

						Ok((estimate_bytes, exact_bytes))
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok((estimate_bytes, exact_bytes))) => {
						println!("estimated size: {estimate_bytes}b");
						if let Some(exact_bytes) = exact_bytes {
							println!("exact size: {exact_bytes}b");
							println!(
								"difference: {}b ({:.1}%)",
								exact_bytes - estimate_bytes,
								(exact_bytes as f64 - estimate_bytes as f64)
									/ estimate_bytes as f64 * 100.0,
							);
						}
					}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::Move {
				old_key,
				new_key,
				recursive,
			} => {
				let mut old_tuple = current_tuple.clone();
				if update_current_tuple(&mut old_tuple, Some(old_key)) {
					return CommandResult::Error;
				}

				let mut new_tuple = current_tuple.clone();
				if update_current_tuple(&mut new_tuple, Some(new_key)) {
					return CommandResult::Error;
				}

				let fut = pool.txn("udb_cli_move", |tx| {
					let old_tuple = old_tuple.clone();
					let new_tuple = new_tuple.clone();
					async move {
						if recursive {
							let old_subspace =
								universaldb::tuple::Subspace::all().subspace(&old_tuple);
							let new_subspace =
								universaldb::tuple::Subspace::all().subspace(&new_tuple);

							// Get all key-value pairs from the old subspace
							let mut stream = tx.get_ranges_keyvalues(
								universaldb::RangeOption {
									mode: StreamingMode::WantAll,
									..(&old_subspace).into()
								},
								Snapshot,
							);

							let mut keys_moved = 0;
							while let Some(entry) = stream.try_next().await? {
								// Unpack key from old subspace
								if let Ok(relative_tuple) =
									old_subspace.unpack::<SimpleTuple>(entry.key())
								{
									// Create new key in the new subspace
									let new_key = new_subspace.pack(&relative_tuple);

									// Set value at new key and clear old key
									tx.set(&new_key, entry.value());
									tx.clear(entry.key());

									keys_moved += 1;
								} else {
									eprintln!("failed unpacking key");
								}
							}

							Ok(keys_moved)
						} else {
							let old_key = universaldb::tuple::pack(&old_tuple);
							let new_key = universaldb::tuple::pack(&new_tuple);

							let Some(value) = tx.get(&old_key, Snapshot).await? else {
								return Ok(0);
							};

							tx.set(&new_key, &value);
							tx.clear(&old_key);

							Ok(1)
						}
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok(keys_moved)) => {
						if keys_moved == 0 && !recursive {
							println!("key does not exist");
						} else {
							println!(
								"{} key{} moved",
								keys_moved,
								if keys_moved == 1 { "" } else { "s" }
							);
						}
					}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::Set {
				key_or_value,
				value,
				type_hint,
			} => {
				let (key, value) = if let Some(value) = value {
					(Some(key_or_value), value)
				} else {
					(None, key_or_value)
				};

				let mut current_tuple = current_tuple.clone();
				if update_current_tuple(&mut current_tuple, key) {
					return CommandResult::Error;
				}

				let parsed_value = match SimpleTupleValue::parse_str_with_type_hint(
					type_hint.as_deref(),
					&value,
				) {
					Ok(parsed) => parsed,
					Err(err) => {
						println!("error: {err:#}");
						return CommandResult::Error;
					}
				};

				let fut = pool.txn("udb_cli_set", |tx| {
					let current_tuple = current_tuple.clone();
					let value = parsed_value.clone();
					async move {
						let key = universaldb::tuple::pack(&current_tuple);
						let value = value.serialize()?;

						tx.set(&key, &value);
						Ok(())
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok(_)) => {}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::Clear {
				key,
				clear_range,
				yes,
			} => {
				let mut current_tuple = current_tuple.clone();
				if update_current_tuple(&mut current_tuple, key) {
					return CommandResult::Error;
				}

				if !yes {
					let term = rivet_term::terminal();
					let response = rivet_term::prompt::PromptBuilder::default()
						.message("Are you sure?")
						.build()
						.expect("failed to build prompt")
						.bool(&term)
						.await
						.expect("failed to show prompt");
					if !response {
						return CommandResult::Error;
					}
				}

				let fut = pool.txn("udb_cli_clear", |tx| {
					let current_tuple = current_tuple.clone();
					async move {
						if clear_range {
							let subspace =
								universaldb::utils::Subspace::all().subspace(&current_tuple);
							tx.clear_subspace_range(&subspace);
						} else {
							let key = universaldb::tuple::pack(&current_tuple);
							tx.clear(&key);
						}

						Ok(())
					}
				});

				match tokio::time::timeout(Duration::from_secs(5), fut).await {
					Ok(Ok(_)) => {}
					Ok(Err(err)) => println!("txn error: {err:#}"),
					Err(_) => println!("txn timed out"),
				}
			}
			SubCommand::Exit => return CommandResult::Exit,
			SubCommand::Oneoff { command } => {
				match command {
					OneoffSubCommand::ReapplyLostServerless { flip, dry_run } => {
						let fut = pool.txn("udb_cli_reapply_lost_serverless", |tx| async move {
							// NOTE: The lost data has no subspace
							let lost_serverless_desired_slots_subspace = universaldb::Subspace::all().subspace(
								&rivet_types::keys::pegboard::ns::ServerlessDesiredSlotsKey::entire_subspace(),
							);

							let mut stream = tx.get_ranges_keyvalues(
								universaldb::RangeOption {
									mode: StreamingMode::WantAll,
									..(&lost_serverless_desired_slots_subspace).into()
								},
								Serializable,
							);

							loop {
								let Some(entry) = stream.try_next().await? else {
									break;
								};

								let (lost_serverless_desired_slots_key, slots) =
									tx.read_entry::<rivet_types::keys::pegboard::ns::ServerlessDesiredSlotsKey>(&entry)?;

								if dry_run {
									tracing::info!(
										namespace_id=?lost_serverless_desired_slots_key.namespace_id,
										runner_name=?lost_serverless_desired_slots_key.runner_name,
										slots,
										"found lost slots",
									);
								} else {
									tracing::info!(
										namespace_id=?lost_serverless_desired_slots_key.namespace_id,
										runner_name=?lost_serverless_desired_slots_key.runner_name,
										slots,
										"applying lost slots",
									);

									let slots = if flip { slots * -1 } else { slots };

									// NOTE: The subspace has changed
									tx.with_subspace(pegboard::keys::subspace()).atomic_op(
										&lost_serverless_desired_slots_key,
										&slots.to_le_bytes(),
										MutationType::Add,
									);
								}
							}

							Ok(())
						});

						match tokio::time::timeout(Duration::from_secs(5), fut).await {
							Ok(Ok(_)) => {}
							Ok(Err(err)) => println!("txn error: {err:#}"),
							Err(_) => println!("txn timed out"),
						}
					}
					OneoffSubCommand::RepairEpoxyChangelog {
						replica_id,
						dry_run,
					} => {
						use std::sync::Arc;
						use std::time::Instant;

						use epoxy_protocol::{generated::v2 as proto_v2, protocol};
						use futures_util::stream;
						use tokio::sync::{Mutex, mpsc};

						const EARLY_TXN_TIMEOUT: Duration = Duration::from_millis(2500);
						// FDB hard limit on txn size is 10 MiB. Cap our writer below that
						// with headroom for per-key/value framing the estimator can't see.
						const TXN_SIZE_CAP: usize = 1 * 1024 * 1024;

						// Conservative per-entry size estimate. apply_entry writes value_key
						// once and clears up to four sibling keys, so charge the entry key
						// length five times plus the value payload and a flat overhead for
						// subspace prefixes / BARE framing.
						fn entry_txn_bytes(entry: &protocol::ChangelogEntry) -> usize {
							entry
								.key
								.len()
								.saturating_mul(5)
								.saturating_add(entry.value.as_ref().map_or(0, |v| v.len()))
								.saturating_add(500)
						}

						let (tx_entries, rx_entries) =
							mpsc::unbounded_channel::<protocol::ChangelogEntry>();

						// Writer task: re-applies v2 entries in txn-bounded batches.
						// recv_many is called inside each txn iteration when the local buffer
						// runs dry; entries not applied before EARLY_TXN_TIMEOUT carry
						// forward into the next txn iteration via `pending`.
						let pool_writer = pool.clone();
						let rx_entries = Arc::new(Mutex::new(rx_entries));
						let writer_task = tokio::spawn(async move {
							let mut pending = Vec::<protocol::ChangelogEntry>::new();
							let mut total_applied = 0;

							loop {
								let to_process = std::mem::take(&mut pending);

								let (remaining, applied, closed) = pool_writer
									.txn("udb_cli_apply_changelog_batch", |tx| {
										let rx = rx_entries.clone();
										let mut buf = to_process.clone();
										async move {
											let start = Instant::now();
											let mut applied = 0;
											let mut closed = false;
											let mut txn_bytes = 0usize;

											loop {
												if start.elapsed() > EARLY_TXN_TIMEOUT {
													break;
												}
												if txn_bytes >= TXN_SIZE_CAP {
													break;
												}

												if buf.is_empty() {
													let n = rx
														.lock()
														.await
														.recv_many(&mut buf, 64)
														.await;
													if n == 0 {
														closed = true;
														break;
													}
												}

												// Take only as many entries as fit under the cap.
												// Always take at least one so a single oversized
												// entry can't deadlock the loop.
												let remaining_cap =
													TXN_SIZE_CAP.saturating_sub(txn_bytes);
												let mut take_count = 0usize;
												let mut take_bytes = 0usize;
												for entry in buf.iter() {
													let eb = entry_txn_bytes(entry);
													if take_count > 0
														&& take_bytes + eb > remaining_cap
													{
														break;
													}
													take_bytes += eb;
													take_count += 1;
												}
												let batch: Vec<_> =
													buf.drain(..take_count).collect();
												let n = batch.len();
												tracing::info!(
													n,
													take_bytes,
													txn_bytes_before = txn_bytes,
													remaining_cap,
													"writer batch drained",
												);
												stream::iter(batch)
													.map(|entry| {
														let tx = tx.clone();
														async move {
															epoxy::replica::changelog::apply_entry(
																&*tx, replica_id, entry, false,
																true, true,
															)
															.await
														}
													})
													.buffer_unordered(64)
													.try_collect::<Vec<_>>()
													.await?;
												applied += n;
												txn_bytes += take_bytes;
											}

											Ok((buf, applied, closed))
										}
									})
									.await?;

								pending = remaining;
								total_applied += applied;
								if applied > 0 {
									tracing::info!(
										applied,
										total_applied,
										"applied changelog batch",
									);
								} else {
									tracing::warn!(
										remaining_in_buf = pending.len(),
										closed,
										"writer txn produced zero applied entries",
									);
								}

								if closed && pending.is_empty() {
									break;
								}
							}

							anyhow::Ok(total_applied)
						});

						// One-shot read of first/last changelog versionstamps for progress.
						// FDB versionstamp = 12 bytes: [commit_version_be: u64][batch: u16][user: u16].
						// commit_version maps to wall-clock via the FDB time keeper
						// (\xff\x02/timeKeeper/map/<commit_version_be>).
						fn commit_version(vs: &[u8]) -> Option<u64> {
							vs.get(0..8)
								.and_then(|b| <[u8; 8]>::try_from(b).ok())
								.map(u64::from_be_bytes)
						}

						let range_lookup = pool
							.txn("udb_cli_changelog_range_lookup", |tx| async move {
								let replica_subspace = epoxy::keys::subspace(replica_id);
								let changelog_subspace = replica_subspace.subspace(&(CHANGELOG,));
								let (range_start, range_end) = changelog_subspace.range();

								let read_one =
									async |reverse: bool| -> anyhow::Result<Option<Vec<u8>>> {
										let mut s = tx.get_ranges_keyvalues(
											RangeOption {
												mode: StreamingMode::Iterator,
												limit: Some(1),
												reverse,
												..(
													KeySelector::first_greater_or_equal(
														range_start.clone(),
													),
													KeySelector::first_greater_or_equal(
														range_end.clone(),
													),
												)
													.into()
											},
											Snapshot,
										);
										Ok(s.try_next()
											.await?
											.and_then(|e| {
												changelog_subspace
													.unpack::<epoxy::keys::ChangelogKey>(e.key())
													.ok()
											})
											.map(|k| k.versionstamp().as_bytes().to_vec()))
									};

								let first = read_one(false).await?;
								let last = read_one(true).await?;
								anyhow::Ok((first, last))
							})
							.await;

						let (first_versionstamp, last_versionstamp) = match range_lookup {
							Ok(pair) => pair,
							Err(err) => {
								println!("range lookup failed: {err:#}");
								(None, None)
							}
						};

						let first_cv = first_versionstamp.as_deref().and_then(commit_version);
						let last_cv = last_versionstamp.as_deref().and_then(commit_version);

						tracing::info!(
							first = first_versionstamp
								.as_deref()
								.map(hex::encode)
								.as_deref()
								.unwrap_or("none"),
							last = last_versionstamp
								.as_deref()
								.map(hex::encode)
								.as_deref()
								.unwrap_or("none"),
							first_commit_version = first_cv,
							last_commit_version = last_cv,
							"changelog range",
						);

						// Reader loop: scans the changelog in chunks and sends detected v2
						// entries to the writer task.
						let mut last_key: Option<Vec<u8>> = None;
						let mut total_scanned = 0usize;
						let mut total_v2 = 0usize;
						let mut total_v3 = 0usize;

						'reader: loop {
							let last_key_for_chunk = last_key.clone();
							let chunk_fut = pool.txn("udb_cli_scan_changelog_chunk", |tx| {
								let last_key = last_key_for_chunk.clone();
								let tx_entries = tx_entries.clone();
								async move {
									let start = Instant::now();
									let replica_subspace = epoxy::keys::subspace(replica_id);
									let changelog_subspace =
										replica_subspace.subspace(&(CHANGELOG,));
									let (range_start, range_end) = changelog_subspace.range();

									let begin = if let Some(lk) = last_key {
										KeySelector::first_greater_than(lk)
									} else {
										KeySelector::first_greater_or_equal(range_start)
									};
									let end = KeySelector::first_greater_or_equal(range_end);

									let mut stream = tx.get_ranges_keyvalues(
										RangeOption {
											mode: StreamingMode::WantAll,
											..(begin, end).into()
										},
										Snapshot,
									);

									let mut new_last_key: Option<Vec<u8>> = None;
									let mut current_versionstamp: Option<Vec<u8>> = None;
									let mut scanned = 0usize;
									let mut v2_count = 0usize;
									let mut v3_count = 0usize;
									let mut timed_out = false;
									let mut exhausted = false;

									loop {
										if start.elapsed() > EARLY_TXN_TIMEOUT {
											timed_out = true;
											break;
										}

										let Some(entry) = stream.try_next().await? else {
											exhausted = true;
											break;
										};

										new_last_key = Some(entry.key().to_vec());
										if let Ok(ck) = changelog_subspace
											.unpack::<epoxy::keys::ChangelogKey>(entry.key())
										{
											current_versionstamp =
												Some(ck.versionstamp().as_bytes().to_vec());
										}
										scanned += 1;

										// A v2 entry roundtrips byte-identically through the v2
										// schema. v3 entries either fail to deserialize as v2 or
										// re-serialize to different bytes, so they are ignored.
										let v2_entry: proto_v2::ChangelogEntry = match rivet_util::serde::bare_from_slice!(
											entry.value()
										) {
											Ok(v) => v,
											Err(_) => {
												v3_count += 1;
												continue;
											}
										};
										let reserialized =
											match rivet_util::serde::bare_to_vec!(&v2_entry) {
												Ok(b) => b,
												Err(_) => {
													v3_count += 1;
													continue;
												}
											};
										if reserialized != entry.value() {
											v3_count += 1;
											continue;
										}
										// A v3 `None` (deletion) encodes as [0x00], which the
										// v2 deserializer reads as empty `data` and roundtrips
										// identically. Reject empty-value entries: we cannot
										// distinguish a real v2 empty-value write from a v3
										// deletion, and applying either as `Some([])` would be
										// wrong for the deletion case.
										if v2_entry.value.is_empty() {
											v3_count += 1;
											continue;
										}

										v2_count += 1;
										tracing::debug!(
											replica_id,
											version = v2_entry.version,
											mutable = v2_entry.mutable,
											"found v2 changelog entry",
										);

										if !dry_run {
											let v3_entry = protocol::ChangelogEntry {
												key: v2_entry.key,
												value: Some(v2_entry.value),
												version: v2_entry.version,
												mutable: v2_entry.mutable,
											};

											tx_entries
												.send(v3_entry)
												.context("writer task closed")?;
										}
									}

									Ok((
										new_last_key,
										current_versionstamp,
										scanned,
										v2_count,
										v3_count,
										timed_out,
										exhausted,
									))
								}
							});

							let (
								new_last_key,
								current_versionstamp,
								scanned,
								v2_count,
								v3_count,
								timed_out,
								exhausted,
							) =
								match tokio::time::timeout(Duration::from_secs(5), chunk_fut).await
								{
									Ok(Ok(res)) => res,
									Ok(Err(err)) => {
										println!("chunk txn error: {err:#}");
										break 'reader;
									}
									Err(_) => {
										println!("chunk txn timed out");
										break 'reader;
									}
								};

							total_scanned += scanned;
							total_v2 += v2_count;
							total_v3 += v3_count;

							let cur_cv = current_versionstamp.as_deref().and_then(commit_version);
							let progress_pct = match (first_cv, last_cv, cur_cv) {
								(Some(f), Some(l), Some(c)) if l > f => {
									Some((c.saturating_sub(f) as f64 / (l - f) as f64) * 100.0)
								}
								_ => None,
							};

							tracing::info!(
								scanned,
								v2_count,
								v3_count,
								timed_out,
								exhausted,
								versionstamp = current_versionstamp
									.as_deref()
									.map(hex::encode)
									.as_deref()
									.unwrap_or("none"),
								commit_version = cur_cv,
								progress_pct = progress_pct.map(|p| format!("{:.3}", p)),
								"processed changelog chunk",
							);

							if exhausted {
								break;
							}
							if new_last_key.is_none() {
								// Nothing scanned this chunk (early timeout before first
								// entry); retry the same cursor.
								continue;
							}
							last_key = new_last_key;
						}

						// Signal the writer that the reader is done and wait for it to flush.
						drop(tx_entries);
						let total_applied = match writer_task.await {
							Ok(Ok(n)) => n,
							Ok(Err(err)) => {
								println!("writer task error: {err:#}");
								0
							}
							Err(err) => {
								println!("writer task panicked: {err}");
								0
							}
						};

						println!("scanned: {total_scanned}");
						println!("v2 entries: {total_v2}");
						println!("v3 entries (skipped): {total_v3}");
						println!("applied: {total_applied}");
					}
				}
			}
		}

		CommandResult::Ok
	}
}

#[derive(Debug, ValueEnum, Clone, Copy, PartialEq)]
pub enum ClearType {
	Range,
}

pub enum CommandResult {
	Ok,
	Error,
	Exit,
}

fn update_current_tuple(current_tuple: &mut SimpleTuple, key: Option<String>) -> bool {
	let Some(key) = key.as_deref() else {
		return false;
	};

	match key_parser::parse_path(key) {
		Ok(parsed) => {
			if parsed.relative {
				for _ in 0..parsed.back_count {
					current_tuple.segments.pop();
				}
				current_tuple.segments.extend(parsed.tuple.segments);
			} else {
				*current_tuple = parsed.tuple;
			}

			false
		}
		Err(err) => {
			println!("error: {err:#}");

			true
		}
	}
}

struct ListRenderContext {
	entry_count: usize,
	subspace_count: usize,
	current_hidden_subspace: Option<SimpleTuple>,
	hidden_count: usize,
	last_key: SimpleTuple,
}

fn render_list_entry(
	max_depth: usize,
	hide_subspaces: bool,
	list_style: ListStyle,
	prefer_bytes: bool,
	subspace: &universaldb::tuple::Subspace,
	ctx: &mut ListRenderContext,
	entry: universaldb::value::Value,
) {
	match subspace.unpack::<SimpleTuple>(entry.key()) {
		Ok(key) => {
			if key.segments.len() <= max_depth {
				if entry.value().is_empty() {
					key.print(&list_style, &ctx.last_key);
					println!();
				} else {
					match SimpleTupleValue::deserialize(None, entry.value()) {
						Ok(value) => {
							// Convert string into bytes
							let value =
								if prefer_bytes && let SimpleTupleValue::String(value) = value {
									SimpleTupleValue::Bytes(value.into_bytes())
								} else {
									value
								};

							let mut s = String::new();
							value.write(&mut s, false).unwrap();

							key.print(&list_style, &ctx.last_key);

							let indent = match list_style {
								ListStyle::List => "  ".to_string(),
								ListStyle::Tree => {
									format!("  {}", "| ".repeat(key.segments.len()))
								}
							};
							println!(
								" = {}",
								indent_string(&s, style(indent).dim().to_string(), true)
							);
						}
						Err(err) => {
							key.print(&list_style, &ctx.last_key);
							println!(" error: {err:#}");
						}
					}
				}

				ctx.last_key = key;

				if let Some(curr) = &ctx.current_hidden_subspace {
					curr.print(&list_style, &ctx.last_key);
					println!(
						"/ {}",
						style(format!(
							"{} {}",
							ctx.hidden_count,
							if ctx.hidden_count == 1 {
								"entry"
							} else {
								"entries"
							}
						))
						.dim()
					);

					ctx.last_key = curr.clone();
					ctx.current_hidden_subspace = None;
					ctx.hidden_count = 0;
				}

				ctx.entry_count += 1;
			} else if !hide_subspaces {
				let sliced = key.slice(max_depth);

				if let Some(curr) = &ctx.current_hidden_subspace {
					if &sliced == curr {
						ctx.hidden_count += 1;
					} else {
						curr.print(&list_style, &ctx.last_key);
						println!(
							"/ {}",
							style(format!(
								"{} {}",
								ctx.hidden_count,
								if ctx.hidden_count == 1 {
									"entry"
								} else {
									"entries"
								}
							))
							.dim()
						);

						ctx.last_key = curr.clone();
						ctx.current_hidden_subspace = Some(sliced);
						ctx.hidden_count = 1;
						ctx.subspace_count += 1;
					}
				} else {
					ctx.current_hidden_subspace = Some(sliced);
					ctx.hidden_count = 1;
					ctx.subspace_count += 1;
				}
			}
		}
		Err(err) => println!("error parsing key: {err:#}"),
	}
}
