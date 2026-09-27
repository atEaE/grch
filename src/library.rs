use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::dir;
use crate::sync::Kind;
use crate::system::System;

pub const GRCH_DIR: &str = ".grch";
const CONFIG_FILE: &str = "config.toml";
pub const INDEX_FILE: &str = "index.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub remote: Remote,
    /// System -> folder name directly under the library root.
    pub dirs: BTreeMap<System, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub backend: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A file found under one of the configured system folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub system: System,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime: u64,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub root: PathBuf,
    pub config: Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    pub matched: Vec<(System, String)>,
    /// Existing folders that do not correspond to any system and are ignored.
    pub ignored: Vec<String>,
}

impl Library {
    /// Create `.grch/` in `dir`. Existing folders are matched to systems case-insensitively;
    /// systems without a folder get their default folder name so that `pull` can create it.
    pub fn init(dir: &Path, remote: Remote) -> Result<(Library, InitReport)> {
        let grch = dir.join(GRCH_DIR);
        if grch.exists() {
            bail!("already initialized: {}", grch.display());
        }
        if let Ok(parent) = Library::discover(dir) {
            bail!(
                "{} is inside the library at {}; nested libraries are not supported",
                dir.display(),
                parent.root.display()
            );
        }

        let mut existing: Vec<String> = Vec::new();
        for entry in fs::read_dir(dir).with_context(|| format!("read dir: {}", dir.display()))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                existing.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        existing.sort();

        let mut dirs = BTreeMap::new();
        let mut matched = Vec::new();
        let mut used = vec![false; existing.len()];
        for system in System::value_variants() {
            let found = existing.iter().position(|folder| {
                folder.eq_ignore_ascii_case(system.default_dir_name())
                    || folder.eq_ignore_ascii_case(system.name())
            });
            let folder = match found {
                Some(i) => {
                    used[i] = true;
                    matched.push((*system, existing[i].clone()));
                    existing[i].clone()
                }
                None => system.default_dir_name().to_string(),
            };
            dirs.insert(*system, folder);
        }
        let ignored = existing
            .iter()
            .zip(&used)
            .filter(|(_, used)| !**used)
            .map(|(folder, _)| folder.clone())
            .collect();

        let config = Config { remote, dirs };
        fs::create_dir(&grch).with_context(|| format!("create {}", grch.display()))?;
        let body = toml::to_string(&config)?;
        fs::write(grch.join(CONFIG_FILE), body)?;

        let library = Library {
            root: dir.to_path_buf(),
            config,
        };
        Ok((library, InitReport { matched, ignored }))
    }

    /// Walk up from `start` until a directory containing `.grch/` is found.
    pub fn discover(start: &Path) -> Result<Library> {
        let mut dir = Some(start);
        while let Some(current) = dir {
            if current.join(GRCH_DIR).is_dir() {
                return Library::open(current);
            }
            dir = current.parent();
        }
        bail!(
            "not a grch library (no {} found in {} or any parent). Run `grch init` first.",
            GRCH_DIR,
            start.display()
        );
    }

    fn open(root: &Path) -> Result<Library> {
        let path = root.join(GRCH_DIR).join(CONFIG_FILE);
        let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let config: Config =
            toml::from_str(&body).with_context(|| format!("parse {}", path.display()))?;
        Ok(Library {
            root: root.to_path_buf(),
            config,
        })
    }

    pub fn grch_dir(&self) -> PathBuf {
        self.root.join(GRCH_DIR)
    }

    pub fn system_dir(&self, system: System) -> PathBuf {
        let folder = self
            .config
            .dirs
            .get(&system)
            .map(String::as_str)
            .unwrap_or(system.default_dir_name());
        self.root.join(folder)
    }

    /// Directory a synced file of `kind` / `system` lives in on this machine.
    pub fn dest_dir(&self, kind: Kind, system: System) -> Result<PathBuf> {
        match kind {
            Kind::Rom => Ok(self.system_dir(system)),
            Kind::Dat => dir::custom_dat_dir(),
        }
    }

    pub fn local_path(&self, kind: Kind, system: System, name: &str) -> Result<PathBuf> {
        Ok(self.dest_dir(kind, system)?.join(name))
    }

    /// List the files directly under each configured system folder.
    /// Subdirectories and dotfiles (e.g. .DS_Store) are skipped. Missing folders are not an error.
    pub fn scan(&self) -> Result<Vec<ScannedFile>> {
        let mut files = Vec::new();
        for system in self.config.dirs.keys() {
            let dir = self.system_dir(*system);
            if !dir.is_dir() {
                continue;
            }
            for entry in
                fs::read_dir(&dir).with_context(|| format!("read dir: {}", dir.display()))?
            {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let metadata = entry.metadata()?;
                if !metadata.is_file() {
                    continue;
                }
                let mtime = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                files.push(ScannedFile {
                    system: *system,
                    name,
                    path: entry.path(),
                    size: metadata.len(),
                    mtime,
                });
            }
        }
        files.sort_by(|a, b| (a.system, &a.name).cmp(&(b.system, &b.name)));
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn dropbox() -> Remote {
        Remote {
            backend: "dropbox".to_string(),
            path: None,
        }
    }

    #[test]
    fn init_matches_existing_folders_case_insensitively() {
        // arrange
        let temp = TempDir::new().unwrap();
        for folder in ["SFC", "gba", "DS", "misc"] {
            fs::create_dir(temp.path().join(folder)).unwrap();
        }
        fs::write(temp.path().join("note.txt"), b"").unwrap();

        // act
        let (library, report) = Library::init(temp.path(), dropbox()).unwrap();

        // assert
        assert_eq!(library.config.dirs[&System::Sfc], "SFC");
        assert_eq!(library.config.dirs[&System::Gba], "gba");
        assert_eq!(library.config.dirs[&System::Nds], "DS");
        assert_eq!(library.config.dirs[&System::N3ds], "3DS");
        assert_eq!(library.config.dirs[&System::Psp], "PSP");
        assert_eq!(
            report.matched,
            vec![
                (System::Gba, "gba".to_string()),
                (System::Sfc, "SFC".to_string()),
                (System::Nds, "DS".to_string()),
            ]
        );
        assert_eq!(report.ignored, vec!["misc".to_string()]);
        assert!(temp.path().join(GRCH_DIR).join(CONFIG_FILE).is_file());
    }

    #[test]
    fn init_accepts_system_name_as_folder() {
        // arrange
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("nds")).unwrap();

        // act
        let (library, _) = Library::init(temp.path(), dropbox()).unwrap();

        // assert
        assert_eq!(library.config.dirs[&System::Nds], "nds");
    }

    #[test]
    fn init_twice_fails() {
        // arrange
        let temp = TempDir::new().unwrap();
        Library::init(temp.path(), dropbox()).unwrap();

        // act & assert
        assert!(Library::init(temp.path(), dropbox()).is_err());
    }

    #[test]
    fn init_inside_existing_library_fails() {
        // arrange
        let temp = TempDir::new().unwrap();
        Library::init(temp.path(), dropbox()).unwrap();
        let nested = temp.path().join("SFC");
        fs::create_dir(&nested).unwrap();

        // act & assert
        let err = Library::init(&nested, dropbox()).unwrap_err();
        assert!(err.to_string().contains("nested"));
    }

    #[test]
    fn config_roundtrips_through_toml() {
        // arrange
        let temp = TempDir::new().unwrap();
        let (library, _) = Library::init(temp.path(), dropbox()).unwrap();

        // act
        let reopened = Library::open(temp.path()).unwrap();

        // assert
        assert_eq!(reopened.config, library.config);
        assert_eq!(reopened.config.remote.backend, "dropbox");
        assert!(
            !fs::read_to_string(temp.path().join(GRCH_DIR).join(CONFIG_FILE))
                .unwrap()
                .contains("path")
        );
    }

    #[test]
    fn local_remote_path_roundtrips() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = Remote {
            backend: "local".to_string(),
            path: Some("/tmp/remote".to_string()),
        };
        Library::init(temp.path(), remote.clone()).unwrap();

        // act
        let reopened = Library::open(temp.path()).unwrap();

        // assert
        assert_eq!(reopened.config.remote, remote);
    }

    #[test]
    fn discover_walks_up_to_root() {
        // arrange
        let temp = TempDir::new().unwrap();
        Library::init(temp.path(), dropbox()).unwrap();
        let nested = temp.path().join("SFC").join("deeper");
        fs::create_dir_all(&nested).unwrap();

        // act
        let library = Library::discover(&nested).unwrap();

        // assert
        assert_eq!(library.root, temp.path());
    }

    #[test]
    fn discover_without_grch_dir_fails() {
        // arrange
        let temp = TempDir::new().unwrap();

        // act & assert
        let err = Library::discover(temp.path()).unwrap_err();
        assert!(err.to_string().contains("grch init"));
    }

    #[test]
    fn scan_lists_files_under_system_dirs_only() {
        // arrange
        let temp = TempDir::new().unwrap();
        let sfc = temp.path().join("SFC");
        let gba = temp.path().join("GBA");
        fs::create_dir_all(sfc.join("sub")).unwrap();
        fs::create_dir(&gba).unwrap();
        fs::write(sfc.join("b.sfc"), b"bb").unwrap();
        fs::write(sfc.join("a.sfc"), b"a").unwrap();
        fs::write(sfc.join(".DS_Store"), b"x").unwrap();
        fs::write(sfc.join("sub").join("nested.sfc"), b"n").unwrap();
        fs::write(gba.join("c.gba"), b"ccc").unwrap();
        fs::write(temp.path().join("loose.gb"), b"l").unwrap();
        let (library, _) = Library::init(temp.path(), dropbox()).unwrap();

        // act
        let files = library.scan().unwrap();

        // assert
        let summary: Vec<(System, &str, u64)> = files
            .iter()
            .map(|f| (f.system, f.name.as_str(), f.size))
            .collect();
        assert_eq!(
            summary,
            vec![
                (System::Gba, "c.gba", 3),
                (System::Sfc, "a.sfc", 1),
                (System::Sfc, "b.sfc", 2),
            ]
        );
        assert_eq!(files[0].path, gba.join("c.gba"));
        assert!(files.iter().all(|f| f.mtime > 0));
    }
}
