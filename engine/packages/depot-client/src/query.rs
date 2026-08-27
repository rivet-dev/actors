use std::error::Error;
use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::c_char;
use std::ptr;

use anyhow::{Result, anyhow};
pub use depot_client_types::{BindParam, ColumnValue, ExecResult, ExecuteResult, QueryResult};
use libsqlite3_sys::{
	SQLITE_BLOB, SQLITE_DONE, SQLITE_FLOAT, SQLITE_INTEGER, SQLITE_NULL, SQLITE_OK, SQLITE_ROW,
	SQLITE_TEXT, SQLITE_TRANSIENT, sqlite3, sqlite3_bind_blob, sqlite3_bind_double,
	sqlite3_bind_int64, sqlite3_bind_null, sqlite3_bind_text, sqlite3_changes, sqlite3_column_blob,
	sqlite3_column_bytes, sqlite3_column_count, sqlite3_column_double, sqlite3_column_int64,
	sqlite3_column_name, sqlite3_column_text, sqlite3_column_type, sqlite3_errmsg,
	sqlite3_extended_errcode, sqlite3_finalize, sqlite3_last_insert_rowid, sqlite3_prepare_v2,
	sqlite3_step,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteQueryProfile {
	pub bind_count: u64,
	pub bind_logical_bytes: u64,
	pub result_rows: u64,
	pub result_columns: u64,
	pub result_logical_bytes: u64,
}

#[derive(Debug)]
pub struct SqliteStatementError {
	pub code: i32,
	pub statement_index: u32,
	pub message: String,
}

impl fmt::Display for SqliteStatementError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

impl Error for SqliteStatementError {}

pub fn execute_statement(
	db: *mut sqlite3,
	sql: &str,
	params: Option<&[BindParam]>,
) -> Result<ExecResult> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut stmt = ptr::null_mut();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(sqlite_error(db, 0, "failed to prepare sqlite statement"));
	}
	if stmt.is_null() {
		return Ok(ExecResult { changes: 0 });
	}

	let result = (|| {
		if let Some(params) = params {
			bind_params(db, stmt, params)?;
		}

		loop {
			let step_rc = unsafe { sqlite3_step(stmt) };
			if step_rc == SQLITE_DONE {
				break;
			}
			if step_rc != SQLITE_ROW {
				return Err(sqlite_error(db, 0, "failed to execute sqlite statement"));
			}
		}

		Ok(ExecResult {
			changes: unsafe { sqlite3_changes(db) as i64 },
		})
	})();

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

pub fn query_statement(
	db: *mut sqlite3,
	sql: &str,
	params: Option<&[BindParam]>,
) -> Result<QueryResult> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut stmt = ptr::null_mut();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(sqlite_error(db, 0, "failed to prepare sqlite query"));
	}
	if stmt.is_null() {
		return Ok(QueryResult {
			columns: Vec::new(),
			rows: Vec::new(),
		});
	}

	let result = (|| {
		if let Some(params) = params {
			bind_params(db, stmt, params)?;
		}

		let columns = collect_columns(stmt);
		let mut rows = Vec::new();

		loop {
			let step_rc = unsafe { sqlite3_step(stmt) };
			if step_rc == SQLITE_DONE {
				break;
			}
			if step_rc != SQLITE_ROW {
				return Err(sqlite_error(db, 0, "failed to step sqlite query"));
			}

			let mut row = Vec::with_capacity(columns.len());
			for index in 0..columns.len() {
				row.push(column_value(stmt, index as i32));
			}
			rows.push(row);
		}

		Ok(QueryResult { columns, rows })
	})();

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

pub fn execute_single_statement(
	db: *mut sqlite3,
	sql: &str,
	params: Option<&[BindParam]>,
) -> Result<ExecuteResult> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut stmt = ptr::null_mut();
	let mut tail = ptr::null();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, &mut tail) };
	if rc != SQLITE_OK {
		return Err(sqlite_error(
			db,
			0,
			"failed to prepare sqlite execute statement",
		));
	}
	if has_non_whitespace_tail(tail) {
		if !stmt.is_null() {
			unsafe {
				sqlite3_finalize(stmt);
			}
		}
		return Err(SqliteStatementError {
			code: -1,
			statement_index: 0,
			message: "sqlite execute only supports a single statement".to_owned(),
		}
		.into());
	}
	if stmt.is_null() {
		return Ok(ExecuteResult {
			columns: Vec::new(),
			rows: Vec::new(),
			changes: 0,
			last_insert_row_id: None,
		});
	}

	let result = (|| {
		if let Some(params) = params {
			bind_params(db, stmt, params)?;
		}

		let columns = collect_columns(stmt);
		let mut rows = Vec::new();
		loop {
			let step_rc = unsafe { sqlite3_step(stmt) };
			if step_rc == SQLITE_DONE {
				break;
			}
			if step_rc != SQLITE_ROW {
				return Err(sqlite_error(
					db,
					0,
					"failed to step sqlite execute statement",
				));
			}

			let mut row = Vec::with_capacity(columns.len());
			for index in 0..columns.len() {
				row.push(column_value(stmt, index as i32));
			}
			rows.push(row);
		}

		let changes = unsafe { sqlite3_changes(db) as i64 };
		Ok(ExecuteResult {
			columns,
			rows,
			changes,
			last_insert_row_id: (changes > 0).then(|| unsafe { sqlite3_last_insert_rowid(db) }),
		})
	})();

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

pub fn execute_single_statement_profiled(
	db: *mut sqlite3,
	sql: &str,
	params: Option<&[BindParam]>,
) -> Result<(ExecuteResult, SqliteQueryProfile)> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut stmt = ptr::null_mut();
	let mut tail = ptr::null();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, &mut tail) };
	if rc != SQLITE_OK {
		return Err(sqlite_error(
			db,
			0,
			"failed to prepare sqlite execute statement",
		));
	}
	if has_non_whitespace_tail(tail) {
		if !stmt.is_null() {
			unsafe {
				sqlite3_finalize(stmt);
			}
		}
		return Err(SqliteStatementError {
			code: -1,
			statement_index: 0,
			message: "sqlite execute only supports a single statement".to_owned(),
		}
		.into());
	}
	if stmt.is_null() {
		return Ok((
			ExecuteResult {
				columns: Vec::new(),
				rows: Vec::new(),
				changes: 0,
				last_insert_row_id: None,
			},
			SqliteQueryProfile::default(),
		));
	}

	let result = (|| {
		let mut profile = SqliteQueryProfile::default();
		if let Some(params) = params {
			bind_params_profiled(db, stmt, params, &mut profile)?;
		}

		let columns = collect_columns(stmt);
		profile.result_columns = columns.len() as u64;
		let mut rows = Vec::new();
		loop {
			let step_rc = unsafe { sqlite3_step(stmt) };
			if step_rc == SQLITE_DONE {
				break;
			}
			if step_rc != SQLITE_ROW {
				return Err(sqlite_error(
					db,
					0,
					"failed to step sqlite execute statement",
				));
			}

			let mut row = Vec::with_capacity(columns.len());
			for index in 0..columns.len() {
				row.push(column_value_profiled(stmt, index as i32, &mut profile));
			}
			rows.push(row);
			profile.result_rows = profile.result_rows.saturating_add(1);
		}

		let changes = unsafe { sqlite3_changes(db) as i64 };
		Ok((
			ExecuteResult {
				columns,
				rows,
				changes,
				last_insert_row_id: (changes > 0).then(|| unsafe { sqlite3_last_insert_rowid(db) }),
			},
			profile,
		))
	})();

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

pub fn exec_statements(db: *mut sqlite3, sql: &str) -> Result<QueryResult> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut remaining = c_sql.as_ptr();
	let mut statement_index = 0_u32;
	let mut final_result = QueryResult {
		columns: Vec::new(),
		rows: Vec::new(),
	};

	while unsafe { *remaining } != 0 {
		let mut stmt = ptr::null_mut();
		let mut tail = ptr::null();
		let rc = unsafe { sqlite3_prepare_v2(db, remaining, -1, &mut stmt, &mut tail) };
		if rc != SQLITE_OK {
			return Err(sqlite_error(
				db,
				statement_index,
				"failed to prepare sqlite exec statement",
			));
		}

		if stmt.is_null() {
			if tail == remaining {
				break;
			}
			remaining = tail;
			continue;
		}

		let result = (|| {
			let columns = collect_columns(stmt);
			let mut rows = Vec::new();
			loop {
				let step_rc = unsafe { sqlite3_step(stmt) };
				if step_rc == SQLITE_DONE {
					break;
				}
				if step_rc != SQLITE_ROW {
					return Err(sqlite_error(
						db,
						statement_index,
						"failed to step sqlite exec statement",
					));
				}

				let mut row = Vec::with_capacity(columns.len());
				for index in 0..columns.len() {
					row.push(column_value(stmt, index as i32));
				}
				rows.push(row);
			}

			Ok((columns, rows))
		})();

		unsafe {
			sqlite3_finalize(stmt);
		}

		let (columns, rows) = result?;
		if !columns.is_empty() || !rows.is_empty() {
			final_result = QueryResult { columns, rows };
		}

		if tail == remaining {
			break;
		}
		remaining = tail;
		statement_index = statement_index.saturating_add(1);
	}

	Ok(final_result)
}

pub fn exec_statements_profiled(
	db: *mut sqlite3,
	sql: &str,
) -> Result<(QueryResult, SqliteQueryProfile)> {
	let c_sql = CString::new(sql).map_err(|err| anyhow!(err.to_string()))?;
	let mut remaining = c_sql.as_ptr();
	let mut statement_index = 0_u32;
	let mut final_result = QueryResult {
		columns: Vec::new(),
		rows: Vec::new(),
	};
	let mut final_profile = SqliteQueryProfile::default();

	while unsafe { *remaining } != 0 {
		let mut stmt = ptr::null_mut();
		let mut tail = ptr::null();
		let rc = unsafe { sqlite3_prepare_v2(db, remaining, -1, &mut stmt, &mut tail) };
		if rc != SQLITE_OK {
			return Err(sqlite_error(
				db,
				statement_index,
				"failed to prepare sqlite exec statement",
			));
		}

		if stmt.is_null() {
			if tail == remaining {
				break;
			}
			remaining = tail;
			continue;
		}

		let result = (|| {
			let columns = collect_columns(stmt);
			let mut profile = SqliteQueryProfile {
				result_columns: columns.len() as u64,
				..Default::default()
			};
			let mut rows = Vec::new();
			loop {
				let step_rc = unsafe { sqlite3_step(stmt) };
				if step_rc == SQLITE_DONE {
					break;
				}
				if step_rc != SQLITE_ROW {
					return Err(sqlite_error(
						db,
						statement_index,
						"failed to step sqlite exec statement",
					));
				}

				let mut row = Vec::with_capacity(columns.len());
				for index in 0..columns.len() {
					row.push(column_value_profiled(stmt, index as i32, &mut profile));
				}
				rows.push(row);
				profile.result_rows = profile.result_rows.saturating_add(1);
			}

			Ok((columns, rows, profile))
		})();

		unsafe {
			sqlite3_finalize(stmt);
		}

		let (columns, rows, profile) = result?;
		if !columns.is_empty() || !rows.is_empty() {
			final_result = QueryResult { columns, rows };
			final_profile = profile;
		}

		if tail == remaining {
			break;
		}
		remaining = tail;
		statement_index = statement_index.saturating_add(1);
	}

	Ok((final_result, final_profile))
}

fn bind_params(
	db: *mut sqlite3,
	stmt: *mut libsqlite3_sys::sqlite3_stmt,
	params: &[BindParam],
) -> Result<()> {
	for (index, param) in params.iter().enumerate() {
		bind_param(db, stmt, (index + 1) as i32, param)?;
	}

	Ok(())
}

fn bind_params_profiled(
	db: *mut sqlite3,
	stmt: *mut libsqlite3_sys::sqlite3_stmt,
	params: &[BindParam],
	profile: &mut SqliteQueryProfile,
) -> Result<()> {
	for (index, param) in params.iter().enumerate() {
		profile.bind_count = profile.bind_count.saturating_add(1);
		profile.bind_logical_bytes = profile
			.bind_logical_bytes
			.saturating_add(logical_bind_bytes(param));
		bind_param(db, stmt, (index + 1) as i32, param)?;
	}

	Ok(())
}

fn bind_param(
	db: *mut sqlite3,
	stmt: *mut libsqlite3_sys::sqlite3_stmt,
	bind_index: i32,
	param: &BindParam,
) -> Result<()> {
	let rc = match param {
		BindParam::Null => unsafe { sqlite3_bind_null(stmt, bind_index) },
		BindParam::Integer(value) => unsafe { sqlite3_bind_int64(stmt, bind_index, *value) },
		BindParam::Float(value) => unsafe { sqlite3_bind_double(stmt, bind_index, *value) },
		BindParam::Text(value) => unsafe {
			sqlite3_bind_text(
				stmt,
				bind_index,
				value.as_ptr() as *const c_char,
				value.len() as i32,
				SQLITE_TRANSIENT(),
			)
		},
		BindParam::Blob(value) => unsafe {
			sqlite3_bind_blob(
				stmt,
				bind_index,
				value.as_ptr() as *const _,
				value.len() as i32,
				SQLITE_TRANSIENT(),
			)
		},
	};

	if rc != SQLITE_OK {
		return Err(sqlite_error(db, 0, "failed to bind sqlite parameter"));
	}

	Ok(())
}

fn collect_columns(stmt: *mut libsqlite3_sys::sqlite3_stmt) -> Vec<String> {
	let column_count = unsafe { sqlite3_column_count(stmt) };
	(0..column_count)
		.map(|index| unsafe {
			let name_ptr = sqlite3_column_name(stmt, index);
			if name_ptr.is_null() {
				String::new()
			} else {
				CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
			}
		})
		.collect()
}

fn column_value(stmt: *mut libsqlite3_sys::sqlite3_stmt, index: i32) -> ColumnValue {
	match unsafe { sqlite3_column_type(stmt, index) } {
		SQLITE_NULL => ColumnValue::Null,
		SQLITE_INTEGER => ColumnValue::Integer(unsafe { sqlite3_column_int64(stmt, index) }),
		SQLITE_FLOAT => ColumnValue::Float(unsafe { sqlite3_column_double(stmt, index) }),
		SQLITE_TEXT => {
			let text_ptr = unsafe { sqlite3_column_text(stmt, index) };
			if text_ptr.is_null() {
				ColumnValue::Null
			} else {
				let text_len = unsafe { sqlite3_column_bytes(stmt, index) } as usize;
				let text = String::from_utf8_lossy(unsafe {
					std::slice::from_raw_parts(text_ptr as *const u8, text_len)
				})
				.into_owned();
				ColumnValue::Text(text)
			}
		}
		SQLITE_BLOB => {
			let blob_ptr = unsafe { sqlite3_column_blob(stmt, index) };
			if blob_ptr.is_null() {
				ColumnValue::Null
			} else {
				let blob_len = unsafe { sqlite3_column_bytes(stmt, index) } as usize;
				let blob = unsafe { std::slice::from_raw_parts(blob_ptr as *const u8, blob_len) };
				ColumnValue::Blob(blob.to_vec())
			}
		}
		_ => ColumnValue::Null,
	}
}

fn column_value_profiled(
	stmt: *mut libsqlite3_sys::sqlite3_stmt,
	index: i32,
	profile: &mut SqliteQueryProfile,
) -> ColumnValue {
	let mut logical_bytes = 0_u64;
	let value = match unsafe { sqlite3_column_type(stmt, index) } {
		SQLITE_NULL => ColumnValue::Null,
		SQLITE_INTEGER => {
			logical_bytes = 8;
			ColumnValue::Integer(unsafe { sqlite3_column_int64(stmt, index) })
		}
		SQLITE_FLOAT => {
			logical_bytes = 8;
			ColumnValue::Float(unsafe { sqlite3_column_double(stmt, index) })
		}
		SQLITE_TEXT => {
			let text_ptr = unsafe { sqlite3_column_text(stmt, index) };
			if text_ptr.is_null() {
				ColumnValue::Null
			} else {
				let text_len = unsafe { sqlite3_column_bytes(stmt, index) } as usize;
				logical_bytes = text_len as u64;
				let text = String::from_utf8_lossy(unsafe {
					std::slice::from_raw_parts(text_ptr as *const u8, text_len)
				})
				.into_owned();
				ColumnValue::Text(text)
			}
		}
		SQLITE_BLOB => {
			let blob_ptr = unsafe { sqlite3_column_blob(stmt, index) };
			if blob_ptr.is_null() {
				ColumnValue::Null
			} else {
				let blob_len = unsafe { sqlite3_column_bytes(stmt, index) } as usize;
				logical_bytes = blob_len as u64;
				let blob = unsafe { std::slice::from_raw_parts(blob_ptr as *const u8, blob_len) };
				ColumnValue::Blob(blob.to_vec())
			}
		}
		_ => ColumnValue::Null,
	};
	profile.result_logical_bytes = profile.result_logical_bytes.saturating_add(logical_bytes);
	value
}

fn logical_bind_bytes(param: &BindParam) -> u64 {
	match param {
		BindParam::Null => 0,
		BindParam::Integer(_) | BindParam::Float(_) => 8,
		BindParam::Text(value) => value.len() as u64,
		BindParam::Blob(value) => value.len() as u64,
	}
}

fn has_non_whitespace_tail(tail: *const c_char) -> bool {
	if tail.is_null() {
		return false;
	}

	let bytes = unsafe { CStr::from_ptr(tail).to_bytes() };
	bytes.iter().any(|byte| !byte.is_ascii_whitespace())
}

fn sqlite_error(db: *mut sqlite3, statement_index: u32, context: &str) -> anyhow::Error {
	let (code, detail) = unsafe {
		if db.is_null() {
			(-1, "unknown sqlite error".to_string())
		} else {
			(
				sqlite3_extended_errcode(db),
				CStr::from_ptr(sqlite3_errmsg(db))
					.to_string_lossy()
					.into_owned(),
			)
		}
	};
	SqliteStatementError {
		code,
		statement_index,
		message: format!("{context}: {detail}"),
	}
	.into()
}

#[cfg(test)]
mod tests {
	use super::*;
	use libsqlite3_sys::{sqlite3_close, sqlite3_open};

	struct MemoryDb(*mut sqlite3);

	impl MemoryDb {
		fn open() -> Self {
			let name = CString::new(":memory:").unwrap();
			let mut db = ptr::null_mut();
			let rc = unsafe { sqlite3_open(name.as_ptr(), &mut db) };
			assert_eq!(rc, SQLITE_OK);
			Self(db)
		}

		fn as_ptr(&self) -> *mut sqlite3 {
			self.0
		}
	}

	impl Drop for MemoryDb {
		fn drop(&mut self) {
			unsafe {
				sqlite3_close(self.0);
			}
		}
	}

	#[test]
	fn run_and_query_bind_typed_params() {
		let db = MemoryDb::open();
		exec_statements(
			db.as_ptr(),
			"CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT, score REAL, payload BLOB);",
		)
		.unwrap();

		let result = execute_statement(
			db.as_ptr(),
			"INSERT INTO items(label, score, payload) VALUES (?, ?, ?);",
			Some(&[
				BindParam::Text("alpha".to_owned()),
				BindParam::Float(3.5),
				BindParam::Blob(vec![1, 2, 3]),
			]),
		)
		.unwrap();
		assert_eq!(result.changes, 1);

		let rows = query_statement(
			db.as_ptr(),
			"SELECT id, label, score, payload FROM items WHERE label = ?;",
			Some(&[BindParam::Text("alpha".to_owned())]),
		)
		.unwrap();
		assert_eq!(rows.columns, vec!["id", "label", "score", "payload"]);
		assert_eq!(
			rows.rows,
			vec![vec![
				ColumnValue::Integer(1),
				ColumnValue::Text("alpha".to_owned()),
				ColumnValue::Float(3.5),
				ColumnValue::Blob(vec![1, 2, 3]),
			]]
		);
	}

	#[test]
	fn exec_returns_last_statement_rows() {
		let db = MemoryDb::open();
		let result = exec_statements(
			db.as_ptr(),
			"CREATE TABLE items(id INTEGER); INSERT INTO items VALUES (1), (2); SELECT COUNT(*) AS count FROM items;",
		)
		.unwrap();

		assert_eq!(result.columns, vec!["count"]);
		assert_eq!(result.rows, vec![vec![ColumnValue::Integer(2)]]);
	}

	#[test]
	fn exec_reports_the_actual_failing_statement_index() {
		let db = MemoryDb::open();
		exec_statements(
			db.as_ptr(),
			"CREATE TABLE unique_items(value INTEGER UNIQUE);",
		)
		.unwrap();

		let error = exec_statements(
			db.as_ptr(),
			"INSERT INTO unique_items VALUES (1); INSERT INTO unique_items VALUES (1);",
		)
		.expect_err("the second statement should violate the unique constraint");
		let typed = error.downcast_ref::<SqliteStatementError>().unwrap();
		assert_eq!(typed.code, libsqlite3_sys::SQLITE_CONSTRAINT_UNIQUE);
		assert_eq!(typed.statement_index, 1);
	}

	#[test]
	fn execute_single_statement_returns_rows_and_metadata() {
		let db = MemoryDb::open();
		let result = execute_single_statement(db.as_ptr(), "SELECT 7 AS value;", None).unwrap();

		assert_eq!(result.columns, vec!["value"]);
		assert_eq!(result.rows, vec![vec![ColumnValue::Integer(7)]]);
		assert_eq!(result.changes, 0);
		assert_eq!(result.last_insert_row_id, None);
	}

	#[test]
	fn profiled_execute_counts_bind_and_result_logical_bytes_during_conversion() {
		let db = MemoryDb::open();
		let (result, profile) = execute_single_statement_profiled(
			db.as_ptr(),
			"SELECT ?, ?, ?;",
			Some(&[
				BindParam::Integer(7),
				BindParam::Text("hello".to_owned()),
				BindParam::Blob(vec![1, 2, 3]),
			]),
		)
		.unwrap();

		assert_eq!(result.rows.len(), 1);
		assert_eq!(profile.bind_count, 3);
		assert_eq!(profile.bind_logical_bytes, 16);
		assert_eq!(profile.result_rows, 1);
		assert_eq!(profile.result_columns, 3);
		assert_eq!(profile.result_logical_bytes, 16);
	}

	#[test]
	fn execute_single_statement_returns_write_metadata() {
		let db = MemoryDb::open();
		exec_statements(
			db.as_ptr(),
			"CREATE TABLE execute_items(id INTEGER PRIMARY KEY, label TEXT);",
		)
		.unwrap();

		let result = execute_single_statement(
			db.as_ptr(),
			"INSERT INTO execute_items(label) VALUES (?);",
			Some(&[BindParam::Text("alpha".to_owned())]),
		)
		.unwrap();

		assert_eq!(result.columns, Vec::<String>::new());
		assert_eq!(result.rows, Vec::<Vec<ColumnValue>>::new());
		assert_eq!(result.changes, 1);
		assert_eq!(result.last_insert_row_id, Some(1));
	}

	#[test]
	fn execute_single_statement_collects_insert_returning_rows() {
		let db = MemoryDb::open();
		exec_statements(
			db.as_ptr(),
			"CREATE TABLE execute_returning(id INTEGER PRIMARY KEY, label TEXT);",
		)
		.unwrap();

		let result = execute_single_statement(
			db.as_ptr(),
			"INSERT INTO execute_returning(label) VALUES ('bravo') RETURNING id, label;",
			None,
		)
		.unwrap();

		assert_eq!(result.columns, vec!["id", "label"]);
		assert_eq!(
			result.rows,
			vec![vec![
				ColumnValue::Integer(1),
				ColumnValue::Text("bravo".to_owned())
			]]
		);
		assert_eq!(result.changes, 1);
		assert_eq!(result.last_insert_row_id, Some(1));
	}

	#[test]
	fn execute_single_statement_collects_readonly_pragma_rows() {
		let db = MemoryDb::open();
		let result = execute_single_statement(db.as_ptr(), "PRAGMA user_version;", None).unwrap();

		assert_eq!(result.columns, vec!["user_version"]);
		assert_eq!(result.rows, vec![vec![ColumnValue::Integer(0)]]);
		assert_eq!(result.changes, 0);
	}

	#[test]
	fn execute_single_statement_runs_mutating_pragma() {
		let db = MemoryDb::open();
		let result =
			execute_single_statement(db.as_ptr(), "PRAGMA user_version = 9;", None).unwrap();

		assert_eq!(result.columns, Vec::<String>::new());
		assert_eq!(result.rows, Vec::<Vec<ColumnValue>>::new());

		let version = execute_single_statement(db.as_ptr(), "PRAGMA user_version;", None).unwrap();
		assert_eq!(version.rows, vec![vec![ColumnValue::Integer(9)]]);
	}

	#[test]
	fn execute_single_statement_rejects_multi_statement_sql() {
		let db = MemoryDb::open();
		let err = execute_single_statement(db.as_ptr(), "SELECT 1; SELECT 2;", None)
			.expect_err("multi statement execute should fail");

		assert!(
			err.to_string().contains("single statement"),
			"unexpected error: {err:#}"
		);
		let typed = err.downcast_ref::<SqliteStatementError>().unwrap();
		assert_eq!(typed.code, -1);
		assert_eq!(typed.statement_index, 0);
	}

	#[test]
	fn execute_single_statement_reports_extended_result_code() {
		let db = MemoryDb::open();
		exec_statements(
			db.as_ptr(),
			"CREATE TABLE unique_items(value TEXT UNIQUE); INSERT INTO unique_items VALUES ('same');",
		)
		.unwrap();
		let err = execute_single_statement(
			db.as_ptr(),
			"INSERT INTO unique_items VALUES ('same')",
			None,
		)
		.expect_err("unique constraint should fail");
		let typed = err.downcast_ref::<SqliteStatementError>().unwrap();
		assert_eq!(typed.code, libsqlite3_sys::SQLITE_CONSTRAINT_UNIQUE);
	}

	#[test]
	fn execute_single_statement_reports_malformed_sql() {
		let db = MemoryDb::open();
		let err = execute_single_statement(db.as_ptr(), "SELECT FROM", None)
			.expect_err("malformed execute should fail");

		assert!(
			err.to_string().contains("failed to prepare"),
			"unexpected error: {err:#}"
		);
	}
}
