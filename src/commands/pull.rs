use std::fs;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::archive::{self, Expected};
use crate::format::human_size;
use crate::index::{self, Index, Synced};
use crate::library::Library;
use crate::manifest::{self, Manifest};
use crate::password;
use crate::prompt;
use crate::remote;
use crate::sync::{self, Action, Kind, Selection};
use crate::system::System;

const TMP_DIR: &str = "tmp";

pub struct Options {
    pub all: bool,
    pub system: Option<System>,
    pub patterns: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
}

/// One file to download and place.
struct Target {
    kind: Kind,
    entry: manifest::Entry,
    note: &'static str,
}

impl Target {
    fn key(&self) -> String {
        sync::key(self.kind, self.entry.system, &self.entry.name)
    }
}

type Key = (Kind, System, String);

pub fn run(opts: Options) -> Result<()> {
    let selection = Selection::parse(opts.system, &opts.patterns)?;
    // Without --all / --system / PATTERN, only files this machine already has are refreshed.
    let fetch_new = opts.all || !selection.is_empty();

    let library = Library::discover(&std::env::current_dir()?)?;
    let remote = remote::open(&library.config.remote)?;

    let mut index = Index::load_refreshed(&library)?;

    let Some((manifest, _rev)) = Manifest::fetch(remote.as_ref())? else {
        println!("remote has no manifest yet (nothing pushed)");
        return Ok(());
    };

    let mut targets: Vec<Target> = Vec::new();
    let mut mark_synced: Vec<Key> = Vec::new();
    let mut deletions: Vec<Key> = Vec::new();
    let mut kept_local: Vec<Key> = Vec::new();
    let mut skipped_conflicts: Vec<String> = Vec::new();
    for change in sync::diff(&index, &manifest) {
        if !selection.matches(change.system, change.name) {
            continue;
        }
        let key: Key = (change.kind, change.system, change.name.to_string());
        match change.action {
            Action::Pull => targets.push(Target {
                kind: change.kind,
                entry: change.remote.unwrap().clone(),
                note: " (updated)",
            }),
            Action::NotFetched if fetch_new => targets.push(Target {
                kind: change.kind,
                entry: change.remote.unwrap().clone(),
                note: "",
            }),
            Action::MarkSynced => mark_synced.push(key),
            Action::Conflict => {
                let local = change.local.unwrap();
                let remote = change.remote.unwrap();
                println!(
                    "{} {} changed on both sides (local {:08X}, remote {:08X} pushed by {})",
                    "!".red(),
                    change.key(),
                    local.crc32,
                    remote.crc32,
                    remote.pushed_by
                );
                if opts.yes || prompt::confirm("  overwrite the local file with the remote?")? {
                    targets.push(Target {
                        kind: change.kind,
                        entry: remote.clone(),
                        note: " (conflict, remote wins)",
                    });
                } else {
                    skipped_conflicts.push(change.key());
                }
            }
            Action::DeletedRemotely => {
                let local = change.local.unwrap();
                let synced = local.synced.as_ref().unwrap();
                if (local.size, local.crc32) == (synced.size, synced.crc32) {
                    deletions.push(key);
                } else {
                    // Changed here after the last sync: keep it, and let push offer it as new.
                    kept_local.push(key);
                }
            }
            _ => {}
        }
    }

    if targets.is_empty() && deletions.is_empty() {
        if !mark_synced.is_empty() {
            record_synced(&mut index, &manifest, &mark_synced);
        }
        for key in &kept_local {
            forget_synced(&mut index, key);
        }
        index.save(&library)?;
        println!("{} nothing to pull", "✓".green());
        report_notes(&kept_local, &skipped_conflicts);
        return Ok(());
    }

    if !targets.is_empty() {
        let total: u64 = targets.iter().map(|t| t.entry.archive_size).sum();
        println!(
            "{} ({}, {} to download)",
            "pull".bold(),
            targets.len(),
            human_size(total)
        );
        for t in &targets {
            println!("  {} {}{}", "↓".cyan(), t.key(), t.note);
        }
    }
    if !deletions.is_empty() {
        println!("{} ({})", "deleted on remote".bold(), deletions.len());
        for (kind, system, name) in &deletions {
            println!("  {} {}", "✗".yellow(), sync::key(*kind, *system, name));
        }
    }
    if opts.dry_run {
        report_notes(&kept_local, &skipped_conflicts);
        return Ok(());
    }

    if !targets.is_empty() {
        if !opts.yes && !prompt::confirm(&format!("Pull {} files?", targets.len()))? {
            println!("aborted");
            return Ok(());
        }
        let password = password::resolve()?;
        let tmp_dir = library.grch_dir().join(TMP_DIR);
        fs::create_dir_all(&tmp_dir)?;
        for t in &targets {
            let entry = &t.entry;
            let archive_path = tmp_dir.join(format!("{}.7z", entry.object));
            let result = fetch_one(
                &library,
                remote.as_ref(),
                t,
                &archive_path,
                &tmp_dir,
                &password,
            );
            let _ = fs::remove_file(&archive_path);
            let placed = result.with_context(|| format!("pull {}", t.key()))?;
            println!("{} {}  {}", "↓".cyan(), t.key(), human_size(entry.size));
            record_placed(&mut index, t.kind, entry, placed);
            index.save(&library)?;
        }
    }

    // Deletions always ask, even with --yes: a folder being reorganized elsewhere must not
    // silently take files away here.
    if !deletions.is_empty() {
        if prompt::confirm(&format!(
            "Delete {} local files removed from the remote?",
            deletions.len()
        ))? {
            for (kind, system, name) in &deletions {
                let path = library.local_path(*kind, *system, name)?;
                fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
                index
                    .entries_mut(*kind)
                    .retain(|e| !(e.system == *system && e.name == *name));
                println!("{} {}", "✗".yellow(), sync::key(*kind, *system, name));
            }
        } else {
            // Keep the files, but stop treating them as synced so push can offer them again.
            for key in &deletions {
                forget_synced(&mut index, key);
            }
            println!("kept local files (they will show as new for push)");
        }
    }

    record_synced(&mut index, &manifest, &mark_synced);
    for key in &kept_local {
        forget_synced(&mut index, key);
    }
    index.save(&library)?;

    println!();
    println!("{} pulled {} files", "✓".green(), targets.len());
    report_notes(&kept_local, &skipped_conflicts);
    Ok(())
}

/// Download one object, verify and place it. Returns the placed file's size and mtime.
fn fetch_one(
    library: &Library,
    remote: &dyn remote::Remote,
    target: &Target,
    archive_path: &std::path::Path,
    tmp_dir: &std::path::Path,
    password: &str,
) -> Result<(u64, u64)> {
    let entry = &target.entry;
    remote
        .download(&entry.object, archive_path)
        .with_context(|| format!("download object {}", entry.object))?;
    let expected = Expected {
        name: entry.name.clone(),
        size: entry.size,
        crc32: entry.crc32,
    };
    let dest_dir = library.dest_dir(target.kind, entry.system)?;
    let placed =
        archive::extract_file(archive_path, password, Some(&expected), &dest_dir, tmp_dir)?;
    let metadata = fs::metadata(&placed)?;
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Ok((metadata.len(), mtime))
}

fn record_placed(
    index: &mut Index,
    kind: Kind,
    entry: &manifest::Entry,
    (size, mtime): (u64, u64),
) {
    let synced = Synced {
        size: entry.size,
        crc32: entry.crc32,
        object: entry.object.clone(),
    };
    match index.find_mut(kind, entry.system, &entry.name) {
        Some(local) => {
            local.size = size;
            local.mtime = mtime;
            local.crc32 = entry.crc32;
            local.synced = Some(synced);
        }
        None => index.entries_mut(kind).push(index::Entry {
            system: entry.system,
            name: entry.name.clone(),
            size,
            mtime,
            crc32: entry.crc32,
            synced: Some(synced),
        }),
    }
}

fn record_synced(index: &mut Index, manifest: &Manifest, keys: &[Key]) {
    for (kind, system, name) in keys {
        let Some(remote) = manifest.find(*kind, *system, name) else {
            continue;
        };
        if let Some(local) = index.find_mut(*kind, *system, name) {
            local.synced = Some(Synced {
                size: remote.size,
                crc32: remote.crc32,
                object: remote.object.clone(),
            });
        }
    }
}

fn forget_synced(index: &mut Index, (kind, system, name): &Key) {
    if let Some(local) = index.find_mut(*kind, *system, name) {
        local.synced = None;
    }
}

fn report_notes(kept_local: &[Key], skipped_conflicts: &[String]) {
    if !kept_local.is_empty() {
        println!();
        println!("removed from the remote but changed here, kept as new:");
        for (kind, system, name) in kept_local {
            println!("  {} {}", "+".dimmed(), sync::key(*kind, *system, name));
        }
    }
    if !skipped_conflicts.is_empty() {
        println!();
        println!(
            "skipped {} conflicts (local kept):",
            skipped_conflicts.len()
        );
        for key in skipped_conflicts {
            println!("  {} {}", "!".red(), key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_entry(name: &str, crc32: u32) -> manifest::Entry {
        manifest::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 3,
            crc32,
            object: "obj".to_string(),
            archive_size: 1,
            pushed_at: String::new(),
            pushed_by: String::new(),
        }
    }

    #[test]
    fn record_placed_inserts_or_updates_with_synced_state() {
        // arrange
        let mut index = Index::default();

        // act
        record_placed(&mut index, Kind::Rom, &remote_entry("a", 7), (3, 100));
        record_placed(&mut index, Kind::Rom, &remote_entry("a", 8), (3, 200));

        // assert
        assert_eq!(index.roms.len(), 1);
        assert_eq!(index.roms[0].crc32, 8);
        assert_eq!(index.roms[0].mtime, 200);
        assert_eq!(index.roms[0].synced.as_ref().unwrap().crc32, 8);
    }

    #[test]
    fn forget_synced_clears_only_the_named_entry() {
        // arrange
        let mut index = Index::default();
        record_placed(&mut index, Kind::Rom, &remote_entry("a", 1), (3, 1));
        record_placed(&mut index, Kind::Rom, &remote_entry("b", 2), (3, 1));

        // act
        forget_synced(&mut index, &(Kind::Rom, System::Sfc, "a".to_string()));

        // assert
        assert!(index.roms[0].synced.is_none());
        assert!(index.roms[1].synced.is_some());
    }
}
