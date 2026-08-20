use std::path::PathBuf;

/// Collect only files. Anything other than files is automatically skipped.
pub fn collect_files(input: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in input {
        let path_str = path.to_string_lossy();
        match glob::glob(&path_str) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if entry.is_file() {
                        files.push(entry);
                    }
                }
            }
            Err(e) => {
                eprintln!("invalid pattern {}: {}", path_str, e);
            }
        }
    }
    files
}
