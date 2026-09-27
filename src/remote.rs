pub mod local;

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::library;

/// Opaque version of the remote manifest, used for optimistic locking on push.
pub type Rev = String;

/// Storage backend. Objects are named by the caller (sha256 hex); the manifest is a
/// single blob with a revision so concurrent pushes from two machines can be detected.
pub trait Remote {
    /// The encrypted manifest and its revision, or `None` when nothing has been pushed yet.
    fn get_manifest(&self) -> Result<Option<(Vec<u8>, Rev)>>;
}

pub fn open(config: &library::Remote) -> Result<Box<dyn Remote>> {
    match config.backend.as_str() {
        "local" => {
            let Some(path) = &config.path else {
                bail!("remote.path is required for the local backend");
            };
            Ok(Box::new(local::LocalDir::new(PathBuf::from(path))))
        }
        "dropbox" => bail!("the dropbox backend is not implemented yet"),
        other => bail!("unknown remote backend: {other:?}"),
    }
}
