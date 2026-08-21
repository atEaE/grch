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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn collect_files_expands_glob() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("a.gba"), b"").unwrap();
        fs::write(temp_dir.path().join("b.gba"), b"").unwrap();
        fs::write(temp_dir.path().join("c.gb"), b"").unwrap();
        fs::write(temp_dir.path().join("d.gbc"), b"").unwrap();
        fs::write(temp_dir.path().join("e.sfc"), b"").unwrap();

        // act
        let pattern = temp_dir.path().join("*.gba");
        let files = collect_files(&[pattern]);

        // assert
        assert_eq!(files.len(), 2);
        let mut names = Vec::new();
        for file in files {
            names.push(file.file_name().unwrap().to_string_lossy().into_owned());
        }
        assert!(names.contains(&"a.gba".to_string()));
        assert!(names.contains(&"b.gba".to_string()));
    }

    #[test]
    fn collect_files_glob_no_matched() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("a.gba"), b"").unwrap();
        fs::write(temp_dir.path().join("b.gba"), b"").unwrap();
        fs::write(temp_dir.path().join("c.gb"), b"").unwrap();

        // act
        let pattern = temp_dir.path().join("*.sfc");
        let files = collect_files(&[pattern]);

        // assert
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn collect_files_direct_match() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("c.gb"), b"").unwrap();
        fs::write(temp_dir.path().join("d.gbc"), b"").unwrap();
        fs::write(temp_dir.path().join("e.sfc"), b"").unwrap();

        // act
        let pattern = temp_dir.path().join("d.gbc");
        let files = collect_files(&[pattern]);

        // assert
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "d.gbc")
    }

    #[test]
    fn collect_files_is_file_only() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("a.gba"), b"").unwrap();
        fs::create_dir(temp_dir.path().join("dir.gba")).unwrap();

        // act
        let pattern = temp_dir.path().join("*.gba");
        let files = collect_files(&[pattern]);

        // assert
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "a.gba")
    }
}
