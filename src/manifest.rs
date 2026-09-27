use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive;
use crate::password;
use crate::remote::{Remote, Rev};
use crate::sync::Kind;
use crate::system::System;

const VERSION: u32 = 1;
const ENTRY_NAME: &str = "manifest.json";

/// Index of everything on the remote. Stored there as a single-entry encrypted 7z
/// (same scheme as the objects) so nothing on the remote is readable without the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub updated_at: String,
    pub updated_by: String,
    pub roms: Vec<Entry>,
    /// Custom DATs. Present from the start so the schema does not change when DAT sync lands.
    #[serde(default)]
    pub dats: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub system: System,
    pub name: String,
    pub size: u64,
    #[serde(with = "crate::hex::u32")]
    pub crc32: u32,
    /// Remote object name: sha256 hex of the archive.
    pub object: String,
    pub archive_size: u64,
    pub pushed_at: String,
    pub pushed_by: String,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            version: VERSION,
            updated_at: String::new(),
            updated_by: String::new(),
            roms: Vec::new(),
            dats: Vec::new(),
        }
    }
}

impl Manifest {
    pub fn entries(&self, kind: Kind) -> &[Entry] {
        match kind {
            Kind::Rom => &self.roms,
            Kind::Dat => &self.dats,
        }
    }

    pub fn entries_mut(&mut self, kind: Kind) -> &mut Vec<Entry> {
        match kind {
            Kind::Rom => &mut self.roms,
            Kind::Dat => &mut self.dats,
        }
    }

    pub fn find(&self, kind: Kind, system: System, name: &str) -> Option<&Entry> {
        self.entries(kind)
            .iter()
            .find(|e| e.system == system && e.name == name)
    }

    /// Every object the manifest references, across both sections.
    pub fn objects(&self) -> impl Iterator<Item = &str> {
        self.roms
            .iter()
            .chain(self.dats.iter())
            .map(|e| e.object.as_str())
    }

    /// Download and decrypt the remote manifest. `None` when nothing has been pushed yet.
    /// The password is only asked for when there is something to decrypt.
    pub fn fetch(remote: &dyn Remote) -> Result<Option<(Manifest, Rev)>> {
        let Some((bytes, rev)) = remote.get_manifest()? else {
            return Ok(None);
        };
        let manifest = Manifest::from_encrypted(&bytes, &password::resolve()?)?;
        Ok(Some((manifest, rev)))
    }

    pub fn from_encrypted(bytes: &[u8], password: &str) -> Result<Manifest> {
        let (_, body) = archive::unpack_bytes(bytes, password).context("decrypt manifest")?;
        serde_json::from_slice(&body).context("parse manifest")
    }
    pub fn to_encrypted(&self, password: &str) -> Result<Vec<u8>> {
        let body = serde_json::to_vec_pretty(self)?;
        archive::pack_bytes(ENTRY_NAME, &body, password).context("encrypt manifest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_roundtrip() {
        // arrange
        let manifest = Manifest {
            updated_at: "2026-09-27T00:00:00Z".to_string(),
            updated_by: "pc-a".to_string(),
            roms: vec![Entry {
                system: System::Sfc,
                name: "Xxx (Japan).sfc".to_string(),
                size: 4,
                crc32: 0x0000_00FF,
                object: "ab".repeat(32),
                archive_size: 300,
                pushed_at: "2026-09-27T00:00:00Z".to_string(),
                pushed_by: "pc-a".to_string(),
            }],
            ..Manifest::default()
        };

        // act
        let bytes = manifest.to_encrypted("pw").unwrap();
        let back = Manifest::from_encrypted(&bytes, "pw").unwrap();

        // assert
        assert_eq!(back, manifest);
        assert!(Manifest::from_encrypted(&bytes, "wrong").is_err());
    }

    #[test]
    fn json_uses_hex_crc_and_tolerates_missing_dats() {
        // arrange
        let body = r#"{"version":1,"updated_at":"","updated_by":"","roms":[{"system":"gb","name":"a.gb","size":1,"crc32":"0000000A","object":"o","archive_size":2,"pushed_at":"","pushed_by":""}]}"#;

        // act
        let manifest: Manifest = serde_json::from_str(body).unwrap();

        // assert
        assert_eq!(manifest.roms[0].crc32, 10);
        assert!(manifest.dats.is_empty());
        assert!(
            serde_json::to_string(&manifest)
                .unwrap()
                .contains("\"crc32\":\"0000000A\"")
        );
    }
}
