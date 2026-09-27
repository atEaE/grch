use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::archive;
use crate::format::human_size;
use crate::index::{self, Index, Synced};
use crate::library::Library;
use crate::manifest::{self, Manifest};
use crate::password;
use crate::prompt;
use crate::remote::{self, ManifestConflict, Remote, Rev};
use crate::sync::{self, Action, Kind, Selection};
use crate::system::System;

const TMP_DIR: &str = "tmp";
/// Attempts at updating the manifest when other machines keep pushing in between.
const MANIFEST_RETRIES: usize = 3;

pub struct Options {
    pub system: Option<System>,
    pub patterns: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
}

/// One file to upload, with what it replaces on the remote.
struct Target {
    kind: Kind,
    system: System,
    name: String,
    path: PathBuf,
    size: u64,
    crc32: u32,
    replaces: Option<manifest::Entry>,
}

pub fn run(opts: Options) -> Result<()> {
    let selection = Selection::parse(opts.system, &opts.patterns)?;
    let library = Library::discover(&std::env::current_dir()?)?;
    let remote = remote::open(&library.config.remote)?;

    let mut index = Index::load_refreshed(&library)?;

    let fetched = Manifest::fetch(remote.as_ref())?;
    let first_push = fetched.is_none();
    let (mut manifest, mut rev) = match fetched {
        Some((manifest, rev)) => (manifest, Some(rev)),
        None => (Manifest::default(), None),
    };

    // Classify, then decide per conflict before anything is transferred.
    let mut targets = Vec::new();
    let mut mark_synced = Vec::new();
    let mut skipped_conflicts = Vec::new();
    for change in sync::diff(&index, &manifest) {
        if !selection.matches(change.system, change.name) {
            continue;
        }
        let local = change.local;
        match change.action {
            Action::PushNew | Action::PushModified => {
                targets.push(target(
                    &library,
                    change.kind,
                    local.unwrap(),
                    change.remote,
                )?);
            }
            Action::MarkSynced => {
                mark_synced.push((change.kind, change.system, change.name.to_string()))
            }
            Action::Conflict => {
                let local = local.unwrap();
                let remote = change.remote.unwrap();
                println!(
                    "{} {} changed on both sides (local {:08X}, remote {:08X} pushed by {})",
                    "!".red(),
                    change.key(),
                    local.crc32,
                    remote.crc32,
                    remote.pushed_by
                );
                let overwrite =
                    opts.yes || prompt::confirm("  overwrite the remote with the local file?")?;
                if overwrite {
                    targets.push(target(&library, change.kind, local, change.remote)?);
                } else {
                    skipped_conflicts.push(change.key());
                }
            }
            _ => {}
        }
    }

    if targets.is_empty() {
        if !mark_synced.is_empty() {
            record_synced(&mut index, &manifest, &mark_synced);
            index.save(&library)?;
            println!(
                "{} {} files already on the remote, index updated",
                "✓".green(),
                mark_synced.len()
            );
        } else {
            println!("{} nothing to push", "✓".green());
        }
        report_skipped(&skipped_conflicts);
        return Ok(());
    }

    let total: u64 = targets.iter().map(|t| t.size).sum();
    println!(
        "{} ({}, {})",
        "push".bold(),
        targets.len(),
        human_size(total)
    );
    for t in &targets {
        let note = if t.replaces.is_some() {
            " (modified)"
        } else {
            " (new)"
        };
        println!("  {} {}{}", "↑".green(), t.key(), note);
    }
    if opts.dry_run {
        report_skipped(&skipped_conflicts);
        return Ok(());
    }
    if first_push {
        println!();
        println!("This is the first push to this remote. Everything is encrypted with the archive");
        println!("password; if it is lost, nothing on the remote can be recovered.");
    }
    if !opts.yes && !prompt::confirm(&format!("Push {} files?", targets.len()))? {
        println!("aborted");
        return Ok(());
    }

    let password = password::resolve()?;
    let tmp_dir = library.grch_dir().join(TMP_DIR);
    fs::create_dir_all(&tmp_dir)?;

    // Objects first. The manifest is only updated once every archive is on the remote,
    // so a crash mid-way leaves orphan objects, never a manifest pointing at nothing.
    let mut pushed: Vec<(Kind, manifest::Entry)> = Vec::new();
    let mut old_objects: Vec<String> = Vec::new();
    for t in &targets {
        let archive_path = tmp_dir.join(format!("{}.7z", t.crc32));
        let packed = archive::pack_file(&t.path, &t.name, &archive_path, &password)
            .with_context(|| format!("pack {}", t.path.display()))?;
        remote
            .upload(&packed.sha256, &archive_path)
            .with_context(|| format!("upload {}", t.key()))?;
        let _ = fs::remove_file(&archive_path);
        println!(
            "{} {}  {} → {}",
            "↑".green(),
            t.key(),
            human_size(t.size),
            human_size(packed.size)
        );
        if let Some(old) = &t.replaces {
            old_objects.push(old.object.clone());
        }
        pushed.push((
            t.kind,
            manifest::Entry {
                system: t.system,
                name: t.name.clone(),
                size: t.size,
                crc32: t.crc32,
                object: packed.sha256,
                archive_size: packed.size,
                pushed_at: now(),
                pushed_by: hostname(),
            },
        ));
    }

    let (manifest, orphaned) =
        update_manifest(remote.as_ref(), &mut manifest, &mut rev, &pushed, opts.yes)?;
    old_objects.extend(orphaned);

    record_pushed(&mut index, &pushed);
    record_synced(&mut index, &manifest, &mark_synced);
    index.save(&library)?;

    // Anything still referenced by the final manifest stays, whatever we thought earlier.
    let referenced: HashSet<&str> = manifest.objects().collect();
    for object in old_objects
        .iter()
        .filter(|o| !referenced.contains(o.as_str()))
    {
        if let Err(e) = remote.delete(object) {
            eprintln!("could not delete old object {object}: {e}");
        }
    }

    println!();
    println!("{} pushed {} files", "✓".green(), pushed.len());
    report_skipped(&skipped_conflicts);
    Ok(())
}

impl Target {
    fn key(&self) -> String {
        sync::key(self.kind, self.system, &self.name)
    }
}

fn target(
    library: &Library,
    kind: Kind,
    local: &index::Entry,
    remote: Option<&manifest::Entry>,
) -> Result<Target> {
    Ok(Target {
        kind,
        system: local.system,
        name: local.name.clone(),
        path: library.local_path(kind, local.system, &local.name)?,
        size: local.size,
        crc32: local.crc32,
        replaces: remote.cloned(),
    })
}

/// Write the manifest with the pushed entries applied. When another machine pushed in
/// between, fetch its manifest and merge ours on top; a file changed by both sides is
/// asked about (or overwritten with `yes`). Returns the final manifest and the objects
/// we uploaded that ended up unused.
fn update_manifest(
    remote: &dyn Remote,
    manifest: &mut Manifest,
    rev: &mut Option<Rev>,
    pushed: &[(Kind, manifest::Entry)],
    yes: bool,
) -> Result<(Manifest, Vec<String>)> {
    let mut pushed: Vec<(Kind, manifest::Entry)> = pushed.to_vec();
    let mut orphaned = Vec::new();
    for _ in 0..MANIFEST_RETRIES {
        let seen = manifest.clone();
        apply(manifest, &pushed);
        let bytes = manifest.to_encrypted(&password::resolve()?)?;
        match remote.put_manifest(&bytes, rev.as_ref()) {
            Ok(new_rev) => {
                *rev = Some(new_rev);
                return Ok((manifest.clone(), orphaned));
            }
            Err(e) if e.downcast_ref::<ManifestConflict>().is_some() => {
                println!("another machine pushed in the meantime, merging");
                let Some((fresh, fresh_rev)) = Manifest::fetch(remote)? else {
                    bail!("the remote manifest disappeared while pushing");
                };
                let (keep, drop) = reconcile(&seen, &fresh, pushed, yes)?;
                orphaned.extend(drop.iter().map(|(_, e)| e.object.clone()));
                pushed = keep;
                *manifest = fresh;
                *rev = Some(fresh_rev);
            }
            Err(e) => return Err(e).context("update manifest"),
        }
    }
    bail!("could not update the manifest after {MANIFEST_RETRIES} attempts; try again")
}

/// Replace or insert `pushed` entries by (kind, system, name); stamps updated_at / updated_by.
fn apply(manifest: &mut Manifest, pushed: &[(Kind, manifest::Entry)]) {
    for (kind, entry) in pushed {
        let list = manifest.entries_mut(*kind);
        match list
            .iter_mut()
            .find(|e| e.system == entry.system && e.name == entry.name)
        {
            Some(existing) => *existing = entry.clone(),
            None => list.push(entry.clone()),
        }
        list.sort_by(|a, b| (a.system, &a.name).cmp(&(b.system, &b.name)));
    }
    manifest.updated_at = now();
    manifest.updated_by = hostname();
}

/// Split `pushed` into entries to keep applying and entries to drop, given that the
/// remote moved from `seen` to `fresh` under us. Only a file the other machine also
/// changed needs a decision; identical content is dropped silently (theirs wins, our
/// object becomes an orphan).
type Pushed = (Kind, manifest::Entry);

fn reconcile(
    seen: &Manifest,
    fresh: &Manifest,
    pushed: Vec<Pushed>,
    yes: bool,
) -> Result<(Vec<Pushed>, Vec<Pushed>)> {
    let mut keep = Vec::new();
    let mut drop = Vec::new();
    for (kind, entry) in pushed {
        let before = seen.find(kind, entry.system, &entry.name).cloned();
        let after = fresh.find(kind, entry.system, &entry.name).cloned();
        let unchanged_by_others =
            before.as_ref().map(|e| (e.size, e.crc32)) == after.as_ref().map(|e| (e.size, e.crc32));
        if unchanged_by_others {
            keep.push((kind, entry));
            continue;
        }
        let after = after.unwrap();
        if (after.size, after.crc32) == (entry.size, entry.crc32) {
            drop.push((kind, entry));
            continue;
        }
        println!(
            "{} {} was also pushed by {} (theirs {:08X}, ours {:08X})",
            "!".red(),
            sync::key(kind, entry.system, &entry.name),
            after.pushed_by,
            after.crc32,
            entry.crc32
        );
        if yes || prompt::confirm("  overwrite theirs with ours?")? {
            keep.push((kind, entry));
        } else {
            drop.push((kind, entry));
        }
    }
    Ok((keep, drop))
}

fn record_pushed(index: &mut Index, pushed: &[(Kind, manifest::Entry)]) {
    for (kind, entry) in pushed {
        if let Some(local) = index.find_mut(*kind, entry.system, &entry.name) {
            local.synced = Some(Synced {
                size: entry.size,
                crc32: entry.crc32,
                object: entry.object.clone(),
            });
        }
    }
}

fn record_synced(index: &mut Index, manifest: &Manifest, keys: &[(Kind, System, String)]) {
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

fn report_skipped(skipped: &[String]) {
    if skipped.is_empty() {
        return;
    }
    println!();
    println!("skipped {} conflicts (remote kept):", skipped.len());
    for name in skipped {
        println!("  {} {}", "!".red(), name);
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, crc32: u32, object: &str, by: &str) -> manifest::Entry {
        manifest::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 1,
            crc32,
            object: object.to_string(),
            archive_size: 1,
            pushed_at: String::new(),
            pushed_by: by.to_string(),
        }
    }

    fn manifest(entries: Vec<manifest::Entry>) -> Manifest {
        Manifest {
            roms: entries,
            ..Manifest::default()
        }
    }

    #[test]
    fn apply_replaces_by_key_and_sorts() {
        // arrange
        let mut m = manifest(vec![entry("b", 1, "ob", "x"), entry("a", 1, "oa", "x")]);

        // act
        apply(
            &mut m,
            &[
                (Kind::Rom, entry("b", 2, "ob2", "me")),
                (Kind::Rom, entry("c", 3, "oc", "me")),
                (Kind::Dat, entry("sfc.dat", 4, "od", "me")),
            ],
        );

        // assert
        let names: Vec<&str> = m.roms.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(m.roms[1].object, "ob2");
        assert_eq!(m.dats.len(), 1);
        assert!(!m.updated_at.is_empty());
    }

    #[test]
    fn reconcile_keeps_entries_others_did_not_touch() {
        // arrange
        let seen = manifest(vec![entry("a", 1, "oa", "x")]);
        let fresh = manifest(vec![entry("a", 1, "oa", "x"), entry("z", 9, "oz", "other")]);
        let pushed = vec![
            (Kind::Rom, entry("a", 2, "oa2", "me")),
            (Kind::Rom, entry("b", 3, "ob", "me")),
        ];

        // act
        let (keep, drop) = reconcile(&seen, &fresh, pushed, false).unwrap();

        // assert
        assert_eq!(keep.len(), 2);
        assert!(drop.is_empty());
    }

    #[test]
    fn reconcile_drops_entry_when_other_pushed_identical_content() {
        // arrange
        let seen = manifest(vec![]);
        let fresh = manifest(vec![entry("a", 2, "theirs", "other")]);
        let pushed = vec![(Kind::Rom, entry("a", 2, "ours", "me"))];

        // act
        let (keep, drop) = reconcile(&seen, &fresh, pushed, false).unwrap();

        // assert
        assert!(keep.is_empty());
        assert_eq!(drop[0].1.object, "ours");
    }

    #[test]
    fn reconcile_with_yes_overwrites_a_real_conflict() {
        // arrange
        let seen = manifest(vec![entry("a", 1, "oa", "x")]);
        let fresh = manifest(vec![entry("a", 5, "theirs", "other")]);
        let pushed = vec![(Kind::Rom, entry("a", 2, "ours", "me"))];

        // act
        let (keep, drop) = reconcile(&seen, &fresh, pushed, true).unwrap();

        // assert
        assert_eq!(keep[0].1.object, "ours");
        assert!(drop.is_empty());
    }

    #[test]
    fn record_pushed_sets_synced_state() {
        // arrange
        let mut index = Index {
            roms: vec![index::Entry {
                system: System::Sfc,
                name: "a".to_string(),
                size: 1,
                mtime: 0,
                crc32: 2,
                synced: None,
            }],
            ..Index::default()
        };

        // act
        record_pushed(&mut index, &[(Kind::Rom, entry("a", 2, "oa", "me"))]);

        // assert
        assert_eq!(
            index.roms[0].synced,
            Some(Synced {
                size: 1,
                crc32: 2,
                object: "oa".to_string()
            })
        );
    }
}
