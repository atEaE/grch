pub mod dropbox;
pub mod local;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Result, bail};

use crate::library;

/// Opaque version of the remote manifest, used for optimistic locking on push.
pub type Rev = String;

/// Returned by `put_manifest` when the remote manifest changed since `expect` was read.
#[derive(Debug)]
pub struct ManifestConflict;

impl fmt::Display for ManifestConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the remote manifest changed since it was fetched")
    }
}

impl std::error::Error for ManifestConflict {}

/// One stored object as the backend sees it, for `remote gc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub name: String,
    pub size: u64,
    /// When the backend last wrote the object (server time for Dropbox).
    pub modified: SystemTime,
}

/// Storage backend. Objects are named by the caller (sha256 hex of the archive); the
/// manifest is a single blob with a revision so concurrent pushes can be detected.
pub trait Remote {
    /// Human-readable description for `remote info` (account, folder, quota).
    fn describe(&self) -> Result<String>;

    /// The encrypted manifest and its revision, or `None` when nothing has been pushed yet.
    fn get_manifest(&self) -> Result<Option<(Vec<u8>, Rev)>>;

    /// Replace the manifest. `expect` is the revision the caller read; `None` means
    /// "there must be no manifest yet". A mismatch fails with `ManifestConflict`.
    fn put_manifest(&self, bytes: &[u8], expect: Option<&Rev>) -> Result<Rev>;

    fn upload(&self, object: &str, file: &Path) -> Result<()>;

    fn download(&self, object: &str, dest: &Path) -> Result<()>;

    /// Deleting an object that does not exist is not an error.
    fn delete(&self, object: &str) -> Result<()>;

    /// Every object on the remote, whether or not the manifest references it.
    fn list_objects(&self) -> Result<Vec<ObjectInfo>>;
}

pub fn open(config: &library::Remote) -> Result<Box<dyn Remote>> {
    match config.backend.as_str() {
        "local" => {
            let Some(path) = &config.path else {
                bail!("remote.path is required for the local backend");
            };
            Ok(Box::new(local::LocalDir::new(PathBuf::from(path))))
        }
        "dropbox" => Ok(Box::new(dropbox::open(config)?)),
        other => bail!("unknown remote backend: {other:?}"),
    }
}
