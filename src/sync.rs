use std::collections::BTreeMap;

use crate::index::{self, Index};
use crate::manifest::{self, Manifest};
use crate::system::System;

/// Verdict for one (system, name) after comparing local (L), last-synced (B) and remote (R).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// L = B = R.
    InSync,
    /// Local file the remote has never seen.
    PushNew,
    /// L differs from B, R still equals B: only this machine changed it.
    PushModified,
    /// R differs from B, L still equals B: another machine changed it.
    Pull,
    /// L and R agree but B is stale (or absent): record the sync, transfer nothing.
    MarkSynced,
    /// L and R both differ from B and from each other.
    Conflict,
    /// This machine synced it before, the remote no longer lists it (`remote rm` elsewhere).
    DeletedRemotely,
    /// On the remote, never fetched here. A pull target only when explicitly selected.
    NotFetched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change<'a> {
    pub system: System,
    pub name: &'a str,
    pub local: Option<&'a index::Entry>,
    pub remote: Option<&'a manifest::Entry>,
    pub action: Action,
}

type Pair<'a> = (Option<&'a index::Entry>, Option<&'a manifest::Entry>);

/// Classify every (system, name) known locally or remotely. Sorted by system, then name.
pub fn diff<'a>(index: &'a Index, manifest: &'a Manifest) -> Vec<Change<'a>> {
    let mut keys: BTreeMap<(System, &'a str), Pair<'a>> = BTreeMap::new();
    for entry in &index.roms {
        keys.entry((entry.system, entry.name.as_str()))
            .or_default()
            .0 = Some(entry);
    }
    for entry in &manifest.roms {
        keys.entry((entry.system, entry.name.as_str()))
            .or_default()
            .1 = Some(entry);
    }

    keys.into_iter()
        .map(|((system, name), (local, remote))| Change {
            system,
            name,
            local,
            remote,
            action: classify(local, remote),
        })
        .collect()
}

fn classify(local: Option<&index::Entry>, remote: Option<&manifest::Entry>) -> Action {
    let l = local.map(|e| (e.size, e.crc32));
    let b = local.and_then(|e| e.synced.as_ref().map(|s| (s.size, s.crc32)));
    let r = remote.map(|e| (e.size, e.crc32));

    match (l, b, r) {
        (None, _, Some(_)) => Action::NotFetched,
        (None, _, None) => Action::InSync,
        (Some(_), None, None) => Action::PushNew,
        (Some(l), None, Some(r)) => {
            if l == r {
                Action::MarkSynced
            } else {
                Action::Conflict
            }
        }
        (Some(_), Some(_), None) => Action::DeletedRemotely,
        (Some(l), Some(b), Some(r)) => {
            let local_changed = l != b;
            let remote_changed = r != b;
            match (local_changed, remote_changed) {
                (false, false) => Action::InSync,
                (true, false) => Action::PushModified,
                (false, true) => Action::Pull,
                (true, true) if l == r => Action::MarkSynced,
                (true, true) => Action::Conflict,
            }
        }
    }
}

/// `--system` and PATTERN filters shared by status / push / pull.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub system: Option<System>,
    pub patterns: Vec<glob::Pattern>,
}

impl Selection {
    pub fn parse(system: Option<System>, patterns: &[String]) -> anyhow::Result<Selection> {
        let patterns = patterns
            .iter()
            .map(|p| {
                glob::Pattern::new(p).map_err(|e| anyhow::anyhow!("invalid pattern {p:?}: {e}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Selection { system, patterns })
    }

    /// Patterns match the file name with or without its extension, ignoring case,
    /// so `pokemon*` finds "Pokemon - Red (Japan).gb" and `*.gb` still works.
    pub fn matches(&self, system: System, name: &str) -> bool {
        if self.system.is_some_and(|s| s != system) {
            return false;
        }
        if self.patterns.is_empty() {
            return true;
        }
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        self.patterns
            .iter()
            .any(|p| p.matches_with(name, options) || p.matches_with(stem, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, crc32: u32, synced: Option<u32>) -> index::Entry {
        index::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 1,
            mtime: 0,
            crc32,
            synced: synced.map(|crc32| index::Synced {
                size: 1,
                crc32,
                object: "obj".to_string(),
            }),
        }
    }

    fn remote(name: &str, crc32: u32) -> manifest::Entry {
        manifest::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 1,
            crc32,
            object: "obj".to_string(),
            archive_size: 1,
            pushed_at: String::new(),
            pushed_by: String::new(),
        }
    }

    fn verdict(local: Option<index::Entry>, remote: Option<manifest::Entry>) -> Action {
        let index = Index {
            roms: local.into_iter().collect(),
            ..Index::default()
        };
        let manifest = Manifest {
            roms: remote.into_iter().collect(),
            ..Manifest::default()
        };
        let changes = diff(&index, &manifest);
        assert_eq!(changes.len(), 1);
        changes[0].action
    }

    #[test]
    fn classify_covers_every_row_of_the_table() {
        // L = B = R
        assert_eq!(
            verdict(Some(local("a", 1, Some(1))), Some(remote("a", 1))),
            Action::InSync
        );
        // L != B, R = B
        assert_eq!(
            verdict(Some(local("a", 2, Some(1))), Some(remote("a", 1))),
            Action::PushModified
        );
        // L = B, R != B
        assert_eq!(
            verdict(Some(local("a", 1, Some(1))), Some(remote("a", 2))),
            Action::Pull
        );
        // L != B, R != B, L = R
        assert_eq!(
            verdict(Some(local("a", 2, Some(1))), Some(remote("a", 2))),
            Action::MarkSynced
        );
        // L != B, R != B, L != R
        assert_eq!(
            verdict(Some(local("a", 2, Some(1))), Some(remote("a", 3))),
            Action::Conflict
        );
        // never synced, remote absent
        assert_eq!(verdict(Some(local("a", 1, None)), None), Action::PushNew);
        // never synced, remote present with same / different content
        assert_eq!(
            verdict(Some(local("a", 1, None)), Some(remote("a", 1))),
            Action::MarkSynced
        );
        assert_eq!(
            verdict(Some(local("a", 1, None)), Some(remote("a", 2))),
            Action::Conflict
        );
        // remote only
        assert_eq!(verdict(None, Some(remote("a", 1))), Action::NotFetched);
        // synced before, remote gone
        assert_eq!(
            verdict(Some(local("a", 1, Some(1))), None),
            Action::DeletedRemotely
        );
    }

    #[test]
    fn size_difference_counts_as_change() {
        // arrange
        let mut l = local("a", 1, Some(1));
        l.size = 2;
        let r = remote("a", 1);

        // act & assert
        assert_eq!(verdict(Some(l), Some(r)), Action::PushModified);
    }

    #[test]
    fn diff_is_sorted_and_keyed_by_system_and_name() {
        // arrange
        let mut gb = local("z.gb", 1, None);
        gb.system = System::Gb;
        let index = Index {
            roms: vec![local("b.sfc", 1, None), gb],
            ..Index::default()
        };
        let manifest = Manifest {
            roms: vec![remote("a.sfc", 1)],
            ..Manifest::default()
        };

        // act
        let changes = diff(&index, &manifest);

        // assert
        let keys: Vec<(System, &str)> = changes.iter().map(|c| (c.system, c.name)).collect();
        assert_eq!(
            keys,
            vec![
                (System::Gb, "z.gb"),
                (System::Sfc, "a.sfc"),
                (System::Sfc, "b.sfc")
            ]
        );
    }

    #[test]
    fn selection_matches_stem_or_full_name_case_insensitively() {
        // arrange
        let sel = Selection::parse(None, &["pocket*".to_string()]).unwrap();
        let ext = Selection::parse(None, &["*.gb".to_string()]).unwrap();
        let sys = Selection::parse(Some(System::Gb), &[]).unwrap();

        // act & assert
        assert!(sel.matches(System::Gb, "Pocket Monsters - Aka (Japan).gb"));
        assert!(!sel.matches(System::Gb, "Tetris (Japan).gb"));
        assert!(ext.matches(System::Gb, "Tetris (Japan).gb"));
        assert!(!ext.matches(System::Sfc, "Tetris (Japan).sfc"));
        assert!(sys.matches(System::Gb, "anything"));
        assert!(!sys.matches(System::Sfc, "anything"));
        assert!(Selection::parse(None, &["[".to_string()]).is_err());
    }
}
