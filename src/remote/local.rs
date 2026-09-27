use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::{Remote, Rev};

pub const MANIFEST_FILE: &str = "manifest.7z";

/// A directory used as the remote. Meant for tests and for trying the sync flow without
/// an account; a folder watched by a desktop sync client would work the same way.
pub struct LocalDir {
    root: PathBuf,
}

impl LocalDir {
    pub fn new(root: PathBuf) -> Self {
        LocalDir { root }
    }
}

impl Remote for LocalDir {
    fn get_manifest(&self) -> Result<Option<(Vec<u8>, Rev)>> {
        let path = self.root.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let rev = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        Ok(Some((bytes, rev)))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn get_manifest_is_none_until_written() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().to_path_buf());

        // act & assert
        assert!(remote.get_manifest().unwrap().is_none());
    }

    #[test]
    fn get_manifest_returns_bytes_and_content_rev() {
        // arrange
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(MANIFEST_FILE), b"abc").unwrap();
        let remote = LocalDir::new(temp.path().to_path_buf());

        // act
        let (bytes, rev) = remote.get_manifest().unwrap().unwrap();

        // assert
        assert_eq!(bytes, b"abc");
        assert_eq!(
            rev,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
