use std::{
	fs,
	path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
	let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
	let workspace_root = Path::new(&manifest_dir)
		.parent()
		.and_then(|p| p.parent())
		.and_then(|p| p.parent())
		.ok_or("Failed to find workspace root")?;

	let schema_dir = workspace_root
		.join("sdks")
		.join("schemas")
		.join("universaldb-commit");
	println!("cargo:rerun-if-changed={}", schema_dir.display());

	let (highest_version, _) = find_highest_version(&schema_dir);

	let cfg = vbare_compiler::Config::default();
	vbare_compiler::process_schemas_with_config(&schema_dir, &cfg)?;

	// Append protocol version constant to generated file
	let combined_imports_path = out_dir.join("combined_imports.rs");
	let mut combined = fs::read_to_string(&combined_imports_path)?;
	combined.push_str(&format!(
		"\npub const PROTOCOL_VERSION: u16 = {};\n",
		highest_version
	));
	fs::write(combined_imports_path, combined)?;

	Ok(())
}

fn find_highest_version(schema_dir: &Path) -> (u32, PathBuf) {
	let mut highest_version = 0;
	let mut highest_version_path = PathBuf::new();

	for entry in fs::read_dir(schema_dir).unwrap().flatten() {
		if !entry.path().is_dir() {
			let path = entry.path();
			let bare_name = path
				.file_name()
				.unwrap()
				.to_str()
				.unwrap()
				.split_once('.')
				.unwrap()
				.0;

			if let Ok(version) = bare_name[1..].parse::<u32>() {
				if version > highest_version {
					highest_version = version;
					highest_version_path = path;
				}
			}
		}
	}

	(highest_version, highest_version_path)
}
