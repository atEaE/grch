use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::{ManifestConflict, Remote, Rev};

pub const MANIFEST_FILE: &str = "manifest.7z";
const OBJECTS_DIR: &str = "objects";

/// A directory used as the remote. Meant for tests and for trying the sync flow without
/// an account; a folder watched by a desktop sync client would work the same way.
pub struct LocalDir {
    root: PathBuf,
}

impl LocalDir {
    pub fn new(root: PathBuf) -> Self {
        LocalDir { root }
    }

    fn object_path(&self, object: &str) -> PathBuf {
        self.root.join(OBJECTS_DIR).join(format!("{object}.7z"))
    }

    fn current_rev(&self) -> Result<Option<Rev>> {
        Ok(self.get_manifest()?.map(|(_, rev)| rev))
    }
}

fn rev_of(bytes: &[u8]) -> Rev {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Write via a sibling temp file so a reader never sees a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir)?;
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    fs::write(tmp.path(), bytes)?;
    tmp.persist(path)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

impl Remote for LocalDir {
    fn describe(&self) -> Result<String> {
        Ok(format!("local directory {}", self.root.display()))
    }

    fn get_manifest(&self) -> Result<Option<(Vec<u8>, Rev)>> {
        let path = self.root.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let rev = rev_of(&bytes);
        Ok(Some((bytes, rev)))
    }

    fn put_manifest(&self, bytes: &[u8], expect: Option<&Rev>) -> Result<Rev> {
        if self.current_rev()?.as_ref() != expect {
            return Err(ManifestConflict.into());
        }
        write_atomic(&self.root.join(MANIFEST_FILE), bytes)?;
        Ok(rev_of(bytes))
    }

    fn upload(&self, object: &str, file: &Path) -> Result<()> {
        let dest = self.object_path(object);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(file, &dest).with_context(|| format!("copy to {}", dest.display()))?;
        Ok(())
    }

    fn download(&self, object: &str, dest: &Path) -> Result<()> {
        let src = self.object_path(object);
        fs::copy(&src, dest).with_context(|| format!("copy from {}", src.display()))?;
        Ok(())
    }

    fn delete(&self, object: &str) -> Result<()> {
        match fs::remove_file(self.object_path(object)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("delete object {object}")),
        }
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
    fn put_manifest_requires_matching_rev() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().to_path_buf());

        // act
        let first = remote.put_manifest(b"one", None).unwrap();
        let stale = remote.put_manifest(b"two", None);
        let second = remote.put_manifest(b"two", Some(&first)).unwrap();
        let outdated = remote.put_manifest(b"three", Some(&first));

        // assert
        assert!(
            stale
                .unwrap_err()
                .downcast_ref::<ManifestConflict>()
                .is_some()
        );
        assert!(
            outdated
                .unwrap_err()
                .downcast_ref::<ManifestConflict>()
                .is_some()
        );
        let (bytes, rev) = remote.get_manifest().unwrap().unwrap();
        assert_eq!(bytes, b"two");
        assert_eq!(rev, second);
        assert_ne!(first, second);
    }

    #[test]
    fn objects_roundtrip_and_delete_is_idempotent() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let src = temp.path().join("src.7z");
        let dest = temp.path().join("dest.7z");
        fs::write(&src, b"payload").unwrap();

        // act
        remote.upload("abc", &src).unwrap();
        remote.download("abc", &dest).unwrap();
        remote.delete("abc").unwrap();
        remote.delete("abc").unwrap();

        // assert
        assert_eq!(fs::read(&dest).unwrap(), b"payload");
        assert!(!temp.path().join("remote/objects/abc.7z").exists());
        assert!(remote.download("abc", &dest).is_err());
    }
}
