use std::path::PathBuf;

/// Collect only files. Anything other than files is automatically skipped.
pub fn collect_files(input: &[PathBuf]) -> Vec<PathBuf> {
	let mut files = Vec::new();
	for path in input {
		if path.is_file() {
			files.push(path.clone());
		}
	}
	files
}