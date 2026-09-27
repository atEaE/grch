use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hash;
use crate::library::{Library, ScannedFile};
use crate::system::System;

const VERSION: u32 = 1;

/// Per-machine sync state plus a hash cache, stored at `.grch/index.json`.
/// Only files currently present on this machine are listed: a file removed locally simply
/// drops out on the next refresh, so "in the index" always means "on disk here".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub roms: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub system: System,
    pub name: String,
    pub size: u64,
    pub mtime: u64,
    #[serde(with = "hex_u32")]
    pub crc32: u32,
    /// crc32 as of the last successful push / pull of this file. `None` until first synced.
    #[serde(default, with = "hex_u32_opt")]
    pub synced_crc32: Option<u32>,
    /// Remote object name (sha256 hex of the archive) the synced crc32 corresponds to.
    #[serde(default)]
    pub object: Option<String>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            version: VERSION,
            roms: Vec::new(),
        }
    }
}

impl Index {
    pub fn load(library: &Library) -> Result<Index> {
        let path = library.grch_dir().join(crate::library::INDEX_FILE);
        if !path.exists() {
            return Ok(Index::default());
        }
        let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self, library: &Library) -> Result<()> {
        let path = library.grch_dir().join(crate::library::INDEX_FILE);
        let body = serde_json::to_string_pretty(self)?;
        fs::write(&path, body).with_context(|| format!("write {}", path.display()))
    }

    /// Rebuild the entry list from a scan. crc32 is reused when size and mtime are unchanged
    /// and recomputed otherwise; sync state is carried over by (system, name).
    /// Returns the number of files that were hashed.
    pub fn refresh(&mut self, scanned: &[ScannedFile]) -> Result<usize> {
        let previous: HashMap<(System, &str), &Entry> = self
            .roms
            .iter()
            .map(|e| ((e.system, e.name.as_str()), e))
            .collect();

        let mut hashed = 0;
        let mut roms = Vec::with_capacity(scanned.len());
        for file in scanned {
            let prev = previous.get(&(file.system, file.name.as_str())).copied();
            let crc32 = match prev {
                Some(p) if p.size == file.size && p.mtime == file.mtime => p.crc32,
                _ => {
                    hashed += 1;
                    crc32_of(&file.path)?
                }
            };
            roms.push(Entry {
                system: file.system,
                name: file.name.clone(),
                size: file.size,
                mtime: file.mtime,
                crc32,
                synced_crc32: prev.and_then(|p| p.synced_crc32),
                object: prev.and_then(|p| p.object.clone()),
            });
        }
        self.roms = roms;
        Ok(hashed)
    }
}

fn crc32_of(path: &Path) -> Result<u32> {
    hash::crc32_file(path).with_context(|| format!("hash {}", path.display()))
}

mod hex_u32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:08X}", v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        let s = String::deserialize(d)?;
        u32::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
    }
}

mod hex_u32_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u32>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => s.serialize_some(&format!("{:08X}", v)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        s.map(|s| u32::from_str_radix(&s, 16).map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use tempfile::TempDir;

    use super::*;

    fn library_with(files: &[(&str, &[u8])]) -> (TempDir, Library) {
        let temp = TempDir::new().unwrap();
        let sfc = temp.path().join("SFC");
        fs::create_dir(&sfc).unwrap();
        for (name, body) in files {
            fs::write(sfc.join(name), body).unwrap();
        }
        let (library, _) = Library::init(temp.path(), "dropbox").unwrap();
        (temp, library)
    }

    #[test]
    fn refresh_hashes_new_files() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();

        // act
        let hashed = index.refresh(&library.scan().unwrap()).unwrap();

        // assert
        assert_eq!(hashed, 1);
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.roms[0].crc32, 0x352441C2);
        assert_eq!(index.roms[0].synced_crc32, None);
    }

    #[test]
    fn refresh_skips_unchanged_files() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap()).unwrap();
        index.roms[0].synced_crc32 = Some(0x352441C2);
        index.roms[0].object = Some("obj".to_string());

        // act
        let hashed = index.refresh(&library.scan().unwrap()).unwrap();

        // assert
        assert_eq!(hashed, 0);
        assert_eq!(index.roms[0].synced_crc32, Some(0x352441C2));
        assert_eq!(index.roms[0].object.as_deref(), Some("obj"));
    }

    #[test]
    fn refresh_rehashes_when_mtime_changes_and_keeps_sync_state() {
        // arrange
        let (temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap()).unwrap();
        index.roms[0].synced_crc32 = Some(0x352441C2);
        let path = temp.path().join("SFC").join("a.sfc");
        fs::write(&path, b"abd").unwrap();
        let later = SystemTime::now() + Duration::from_secs(10);
        fs::File::open(&path).unwrap().set_modified(later).unwrap();

        // act
        let hashed = index.refresh(&library.scan().unwrap()).unwrap();

        // assert
        assert_eq!(hashed, 1);
        assert_ne!(index.roms[0].crc32, 0x352441C2);
        assert_eq!(index.roms[0].synced_crc32, Some(0x352441C2));
    }

    #[test]
    fn refresh_drops_files_removed_from_disk() {
        // arrange
        let (temp, library) = library_with(&[("a.sfc", b"a"), ("b.sfc", b"b")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap()).unwrap();
        fs::remove_file(temp.path().join("SFC").join("a.sfc")).unwrap();

        // act
        index.refresh(&library.scan().unwrap()).unwrap();

        // assert
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.roms[0].name, "b.sfc");
    }

    #[test]
    fn index_roundtrips_through_json_with_hex_crc() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap()).unwrap();
        index.roms[0].synced_crc32 = Some(0x0000_00FF);

        // act
        index.save(&library).unwrap();
        let body = fs::read_to_string(library.grch_dir().join("index.json")).unwrap();
        let loaded = Index::load(&library).unwrap();

        // assert
        assert!(body.contains("\"crc32\": \"352441C2\""));
        assert!(body.contains("\"synced_crc32\": \"000000FF\""));
        assert_eq!(loaded, index);
    }

    #[test]
    fn load_missing_index_is_empty() {
        // arrange
        let (_temp, library) = library_with(&[]);

        // act
        let index = Index::load(&library).unwrap();

        // assert
        assert_eq!(index, Index::default());
    }
}
