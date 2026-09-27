use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hash;
use crate::library::{Library, ScannedFile};
use crate::sync::Kind;
use crate::system::System;

const VERSION: u32 = 1;

/// Per-machine sync state plus a hash cache, stored at `.grch/index.json`.
/// Only files currently present on this machine are listed: a file removed locally simply
/// drops out on the next refresh, so "in the index" always means "on disk here".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub roms: Vec<Entry>,
    /// Custom DATs from `custom_dat/`, keyed by system like ROMs.
    #[serde(default)]
    pub dats: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub system: System,
    pub name: String,
    pub size: u64,
    pub mtime: u64,
    #[serde(with = "crate::hex::u32")]
    pub crc32: u32,
    /// State as of the last successful push / pull of this file. `None` until first synced.
    #[serde(default)]
    pub synced: Option<Synced>,
}

/// What the remote held for this file when it was last synced from this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Synced {
    pub size: u64,
    #[serde(with = "crate::hex::u32")]
    pub crc32: u32,
    /// Remote object name (sha256 hex of the archive).
    pub object: String,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            version: VERSION,
            roms: Vec::new(),
            dats: Vec::new(),
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

    pub fn entries_mut(&mut self, kind: Kind) -> &mut Vec<Entry> {
        match kind {
            Kind::Rom => &mut self.roms,
            Kind::Dat => &mut self.dats,
        }
    }

    pub fn find_mut(&mut self, kind: Kind, system: System, name: &str) -> Option<&mut Entry> {
        self.entries_mut(kind)
            .iter_mut()
            .find(|e| e.system == system && e.name == name)
    }

    /// Load, scan the library and `custom_dat/`, refresh and save. Reports hashed files.
    pub fn load_refreshed(library: &Library) -> Result<Index> {
        let mut index = Index::load(library)?;
        let hashed = index.refresh(&library.scan()?, &crate::dat::scan_custom()?)?;
        index.save(library)?;
        if hashed > 0 {
            eprintln!("hashed {} changed files", hashed);
        }
        Ok(index)
    }

    /// Rebuild both lists from scans. crc32 is reused when size and mtime are unchanged
    /// and recomputed otherwise; sync state is carried over by (system, name).
    /// Returns the number of files that were hashed.
    pub fn refresh(&mut self, roms: &[ScannedFile], dats: &[ScannedFile]) -> Result<usize> {
        let hashed_roms = refresh_list(&mut self.roms, roms)?;
        let hashed_dats = refresh_list(&mut self.dats, dats)?;
        Ok(hashed_roms + hashed_dats)
    }
}

fn refresh_list(list: &mut Vec<Entry>, scanned: &[ScannedFile]) -> Result<usize> {
    let previous: HashMap<(System, &str), &Entry> = list
        .iter()
        .map(|e| ((e.system, e.name.as_str()), e))
        .collect();

    let mut hashed = 0;
    let mut entries = Vec::with_capacity(scanned.len());
    for file in scanned {
        let prev = previous.get(&(file.system, file.name.as_str())).copied();
        let crc32 = match prev {
            Some(p) if p.size == file.size && p.mtime == file.mtime => p.crc32,
            _ => {
                hashed += 1;
                crc32_of(&file.path)?
            }
        };
        entries.push(Entry {
            system: file.system,
            name: file.name.clone(),
            size: file.size,
            mtime: file.mtime,
            crc32,
            synced: prev.and_then(|p| p.synced.clone()),
        });
    }
    *list = entries;
    Ok(hashed)
}

fn crc32_of(path: &Path) -> Result<u32> {
    hash::crc32_file(path).with_context(|| format!("hash {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use tempfile::TempDir;

    use super::*;

    fn synced(crc32: u32) -> Synced {
        Synced {
            size: 3,
            crc32,
            object: "obj".to_string(),
        }
    }

    fn library_with(files: &[(&str, &[u8])]) -> (TempDir, Library) {
        let temp = TempDir::new().unwrap();
        let sfc = temp.path().join("SFC");
        fs::create_dir(&sfc).unwrap();
        for (name, body) in files {
            fs::write(sfc.join(name), body).unwrap();
        }
        let remote = crate::library::Remote {
            backend: "dropbox".to_string(),
            path: None,
        };
        let (library, _) = Library::init(temp.path(), remote).unwrap();
        (temp, library)
    }

    #[test]
    fn refresh_hashes_new_files() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();

        // act
        let hashed = index.refresh(&library.scan().unwrap(), &[]).unwrap();

        // assert
        assert_eq!(hashed, 1);
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.roms[0].crc32, 0x352441C2);
        assert_eq!(index.roms[0].synced, None);
    }

    #[test]
    fn refresh_skips_unchanged_files() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap(), &[]).unwrap();
        index.roms[0].synced = Some(synced(0x352441C2));

        // act
        let hashed = index.refresh(&library.scan().unwrap(), &[]).unwrap();

        // assert
        assert_eq!(hashed, 0);
        assert_eq!(index.roms[0].synced, Some(synced(0x352441C2)));
    }

    #[test]
    fn refresh_rehashes_when_mtime_changes_and_keeps_sync_state() {
        // arrange
        let (temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap(), &[]).unwrap();
        index.roms[0].synced = Some(synced(0x352441C2));
        let path = temp.path().join("SFC").join("a.sfc");
        fs::write(&path, b"abd").unwrap();
        let later = SystemTime::now() + Duration::from_secs(10);

        // Windows rejects set_modified on a read-only handle, so open for writing.
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(later).unwrap();

        // act
        let hashed = index.refresh(&library.scan().unwrap(), &[]).unwrap();

        // assert
        assert_eq!(hashed, 1);
        assert_ne!(index.roms[0].crc32, 0x352441C2);
        assert_eq!(index.roms[0].synced, Some(synced(0x352441C2)));
    }

    #[test]
    fn refresh_drops_files_removed_from_disk() {
        // arrange
        let (temp, library) = library_with(&[("a.sfc", b"a"), ("b.sfc", b"b")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap(), &[]).unwrap();
        fs::remove_file(temp.path().join("SFC").join("a.sfc")).unwrap();

        // act
        index.refresh(&library.scan().unwrap(), &[]).unwrap();

        // assert
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.roms[0].name, "b.sfc");
    }

    #[test]
    fn index_roundtrips_through_json_with_hex_crc() {
        // arrange
        let (_temp, library) = library_with(&[("a.sfc", b"abc")]);
        let mut index = Index::default();
        index.refresh(&library.scan().unwrap(), &[]).unwrap();
        index.roms[0].synced = Some(synced(0x0000_00FF));

        // act
        index.save(&library).unwrap();
        let body = fs::read_to_string(library.grch_dir().join("index.json")).unwrap();
        let loaded = Index::load(&library).unwrap();

        // assert
        assert!(body.contains("\"crc32\": \"352441C2\""));
        assert!(body.contains("\"crc32\": \"000000FF\""));
        assert_eq!(loaded, index);
    }

    #[test]
    fn refresh_keeps_dats_separate_from_roms() {
        // arrange
        let (temp, library) = library_with(&[("a.sfc", b"abc")]);
        let dat_path = temp.path().join("3ds.dat");
        fs::write(&dat_path, b"dat").unwrap();
        let dat = ScannedFile {
            system: System::N3ds,
            name: "3ds.dat".to_string(),
            path: dat_path,
            size: 3,
            mtime: 1,
        };
        let mut index = Index::default();

        // act
        let hashed = index.refresh(&library.scan().unwrap(), &[dat]).unwrap();

        // assert
        assert_eq!(hashed, 2);
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.dats.len(), 1);
        assert_eq!(index.dats[0].name, "3ds.dat");
        assert_eq!(
            index
                .find_mut(Kind::Dat, System::N3ds, "3ds.dat")
                .unwrap()
                .crc32,
            crc32fast::hash(b"dat")
        );
        assert!(index.find_mut(Kind::Rom, System::N3ds, "3ds.dat").is_none());
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
