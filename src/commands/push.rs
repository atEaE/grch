use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// The manifest is updated during a push once this much has been uploaded since the last
/// update, or once this much time has passed, whichever comes first. Large files are thus
/// recorded one by one, while thousands of small ones do not rewrite the manifest each time.
const CHECKPOINT_BYTES: u64 = 256 * 1024 * 1024;
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(60);

pub struct Options {
    pub system: Option<System>,
    pub patterns: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
}

/// An uploaded file and the manifest entry describing it.
type Pushed = (Kind, manifest::Entry);

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
    let (manifest, rev) = match fetched {
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

    // Objects first, then the manifest: a crash leaves orphan objects, never a manifest
    // pointing at nothing. The manifest is updated at checkpoints along the way, so a
    // long push that fails or is interrupted keeps everything recorded so far and the
    // next push only has the rest to do.
    let mut checkpoint = Checkpoint {
        remote: remote.as_ref(),
        library: &library,
        password: &password,
        index: &mut index,
        manifest,
        rev,
        mark_synced: &mark_synced,
        yes: opts.yes,
        pending: Vec::new(),
        pending_old: Vec::new(),
        pending_bytes: 0,
        last_commit: Instant::now(),
        recorded: 0,
    };
    let mut failure = None;
    for t in &targets {
        match upload(remote.as_ref(), t, &tmp_dir, &password) {
            Ok(entry) => {
                println!(
                    "{} {}  {} → {}",
                    "↑".green(),
                    t.key(),
                    human_size(t.size),
                    human_size(entry.archive_size)
                );
                checkpoint.add(t, entry);
                if checkpoint.due() {
                    checkpoint.commit()?;
                }
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }

    // Also on failure: what did get uploaded is recorded before the error is reported.
    let flushed = checkpoint.commit();
    let recorded = checkpoint.recorded;
    if let Some(e) = failure {
        if let Err(flush) = flushed {
            eprintln!("could not record the files uploaded before the failure: {flush:#}");
        }
        println!();
        println!(
            "{} pushed {} of {} files before failing; run push again for the rest",
            "!".red(),
            recorded,
            targets.len()
        );
        report_skipped(&skipped_conflicts);
        return Err(e);
    }
    flushed?;

    println!();
    println!("{} pushed {} files", "✓".green(), recorded);
    report_skipped(&skipped_conflicts);
    Ok(())
}

/// Pack one file and put the archive on the remote. Returns its manifest entry, which is
/// not on the remote manifest yet.
fn upload(
    remote: &dyn Remote,
    t: &Target,
    tmp_dir: &Path,
    password: &str,
) -> Result<manifest::Entry> {
    let archive_path = tmp_dir.join(format!("{}.7z", t.crc32));
    let packed = archive::pack_file(&t.path, &t.name, &archive_path, password)
        .with_context(|| format!("pack {}", t.path.display()));
    let uploaded = packed.and_then(|packed| {
        remote
            .upload(&packed.sha256, &archive_path)
            .with_context(|| format!("upload {}", t.key()))?;
        Ok(packed)
    });
    let _ = fs::remove_file(&archive_path);
    let packed = uploaded?;
    Ok(manifest::Entry {
        system: t.system,
        name: t.name.clone(),
        size: t.size,
        crc32: t.crc32,
        object: packed.sha256,
        archive_size: packed.size,
        pushed_at: now(),
        pushed_by: hostname(),
    })
}

/// Uploaded files waiting to be written to the manifest, and the state needed to do so.
struct Checkpoint<'a> {
    remote: &'a dyn Remote,
    library: &'a Library,
    password: &'a str,
    index: &'a mut Index,
    manifest: Manifest,
    rev: Option<Rev>,
    mark_synced: &'a [(Kind, System, String)],
    yes: bool,
    pending: Vec<Pushed>,
    /// Objects replaced by `pending`, deleted once the manifest no longer references them.
    pending_old: Vec<String>,
    pending_bytes: u64,
    last_commit: Instant,
    /// Files written to the manifest so far.
    recorded: usize,
}

impl Checkpoint<'_> {
    fn add(&mut self, target: &Target, entry: manifest::Entry) {
        if let Some(old) = &target.replaces {
            self.pending_old.push(old.object.clone());
        }
        self.pending_bytes += entry.archive_size;
        self.pending.push((target.kind, entry));
    }

    fn due(&self) -> bool {
        self.pending_bytes >= CHECKPOINT_BYTES || self.last_commit.elapsed() >= CHECKPOINT_INTERVAL
    }

    /// Write the pending files to the manifest, save the index, and delete the objects
    /// they replaced. With nothing pending only the index is brought up to date.
    fn commit(&mut self) -> Result<()> {
        let pushed = std::mem::take(&mut self.pending);
        let mut old_objects = std::mem::take(&mut self.pending_old);
        self.pending_bytes = 0;

        if !pushed.is_empty() {
            let orphaned = update_manifest(
                self.remote,
                &mut self.manifest,
                &mut self.rev,
                &pushed,
                self.password,
                self.yes,
            )?;
            old_objects.extend(orphaned);
            record_pushed(self.index, &pushed);
        }
        record_synced(self.index, &self.manifest, self.mark_synced);
        if !pushed.is_empty() || !self.mark_synced.is_empty() {
            self.index.save(self.library)?;
        }
        self.recorded += pushed.len();

        // Anything still referenced by the manifest stays, whatever we thought earlier.
        let referenced: HashSet<&str> = self.manifest.objects().collect();
        for object in old_objects
            .iter()
            .filter(|o| !referenced.contains(o.as_str()))
        {
            if let Err(e) = self.remote.delete(object) {
                eprintln!("could not delete old object {object}: {e}");
            }
        }
        self.last_commit = Instant::now();
        Ok(())
    }
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

/// Write the manifest with the pushed entries applied, leaving `manifest` and `rev` at
/// what is now on the remote. When another machine pushed in between, fetch its manifest
/// and merge ours on top; a file changed by both sides is asked about (or overwritten
/// with `yes`). Returns the objects we uploaded that ended up unused.
fn update_manifest(
    remote: &dyn Remote,
    manifest: &mut Manifest,
    rev: &mut Option<Rev>,
    pushed: &[Pushed],
    password: &str,
    yes: bool,
) -> Result<Vec<String>> {
    let mut pushed: Vec<Pushed> = pushed.to_vec();
    let mut orphaned = Vec::new();
    for _ in 0..MANIFEST_RETRIES {
        let mut next = manifest.clone();
        apply(&mut next, &pushed);
        let bytes = next.to_encrypted(password)?;
        match remote.put_manifest(&bytes, rev.as_ref()) {
            Ok(new_rev) => {
                *manifest = next;
                *rev = Some(new_rev);
                return Ok(orphaned);
            }
            Err(e) if e.downcast_ref::<ManifestConflict>().is_some() => {
                println!("another machine pushed in the meantime, merging");
                let Some((bytes, fresh_rev)) = remote.get_manifest()? else {
                    bail!("the remote manifest disappeared while pushing");
                };
                let fresh = Manifest::from_encrypted(&bytes, password)?;
                let (keep, drop) = reconcile(manifest, &fresh, pushed, yes)?;
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
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;
    use crate::library;
    use crate::remote::local::LocalDir;

    const PASSWORD: &str = "secret";

    fn library_in(temp: &TempDir) -> Library {
        let root = temp.path().join("library");
        fs::create_dir_all(root.join(library::GRCH_DIR)).unwrap();
        Library {
            root,
            config: library::Config {
                remote: library::Remote {
                    backend: "local".to_string(),
                    path: None,
                    app_key: None,
                },
                dirs: BTreeMap::new(),
            },
        }
    }

    fn local(name: &str, crc32: u32) -> index::Entry {
        index::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 1,
            mtime: 0,
            crc32,
            synced: None,
        }
    }

    fn target_for(name: &str, crc32: u32, replaces: Option<manifest::Entry>) -> Target {
        Target {
            kind: Kind::Rom,
            system: System::Sfc,
            name: name.to_string(),
            path: PathBuf::new(),
            size: 1,
            crc32,
            replaces,
        }
    }

    fn checkpoint<'a>(
        remote: &'a LocalDir,
        library: &'a Library,
        index: &'a mut Index,
        manifest: Manifest,
        rev: Option<Rev>,
    ) -> Checkpoint<'a> {
        Checkpoint {
            remote,
            library,
            password: PASSWORD,
            index,
            manifest,
            rev,
            mark_synced: &[],
            yes: false,
            pending: Vec::new(),
            pending_old: Vec::new(),
            pending_bytes: 0,
            last_commit: Instant::now(),
            recorded: 0,
        }
    }

    fn remote_manifest(remote: &LocalDir) -> Manifest {
        let (bytes, _) = remote.get_manifest().unwrap().unwrap();
        Manifest::from_encrypted(&bytes, PASSWORD).unwrap()
    }

    fn put_object(temp: &TempDir, remote: &LocalDir, object: &str) {
        let src = temp.path().join("object.7z");
        fs::write(&src, b"payload").unwrap();
        remote.upload(object, &src).unwrap();
    }

    fn object_names(remote: &LocalDir) -> Vec<String> {
        let mut names: Vec<String> = remote
            .list_objects()
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        names.sort();
        names
    }

    #[test]
    fn checkpoint_commits_build_on_each_other() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let library = library_in(&temp);
        let mut index = Index {
            roms: vec![local("a", 1), local("b", 2), local("c", 3)],
            ..Index::default()
        };
        let mut cp = checkpoint(&remote, &library, &mut index, Manifest::default(), None);

        // act: a first checkpoint, then the rest of the push never gets recorded
        cp.add(&target_for("a", 1, None), entry("a", 1, "oa", "me"));
        cp.commit().unwrap();
        let after_first = remote_manifest(&remote);
        let saved_after_first = Index::load(&library).unwrap();
        cp.add(&target_for("b", 2, None), entry("b", 2, "ob", "me"));
        cp.commit().unwrap();
        cp.add(&target_for("c", 3, None), entry("c", 3, "oc", "me"));

        // assert
        assert_eq!(after_first.roms.len(), 1);
        assert_eq!(
            saved_after_first.roms[0].synced.as_ref().unwrap().object,
            "oa"
        );
        assert!(saved_after_first.roms[1].synced.is_none());
        let names: Vec<String> = remote_manifest(&remote)
            .roms
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(cp.recorded, 2);
        let saved = Index::load(&library).unwrap();
        assert!(saved.roms[1].synced.is_some());
        assert!(saved.roms[2].synced.is_none());
    }

    #[test]
    fn checkpoint_commit_deletes_the_object_it_replaced() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let library = library_in(&temp);
        let old = entry("a", 1, "old", "other");
        let before = manifest(vec![old.clone(), entry("b", 2, "ob", "other")]);
        let rev = remote
            .put_manifest(&before.to_encrypted(PASSWORD).unwrap(), None)
            .unwrap();
        for object in ["old", "ob", "new"] {
            put_object(&temp, &remote, object);
        }
        let mut index = Index {
            roms: vec![local("a", 5)],
            ..Index::default()
        };
        let mut cp = checkpoint(&remote, &library, &mut index, before, Some(rev));

        // act
        cp.add(&target_for("a", 5, Some(old)), entry("a", 5, "new", "me"));
        cp.commit().unwrap();

        // assert
        assert_eq!(object_names(&remote), vec!["new", "ob"]);
        assert_eq!(remote_manifest(&remote).roms[0].object, "new");
    }

    #[test]
    fn checkpoint_commit_merges_what_another_machine_pushed_meanwhile() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let library = library_in(&temp);
        let mut index = Index {
            roms: vec![local("a", 1), local("b", 2)],
            ..Index::default()
        };
        let mut cp = checkpoint(&remote, &library, &mut index, Manifest::default(), None);
        cp.add(&target_for("a", 1, None), entry("a", 1, "oa", "me"));
        cp.commit().unwrap();
        let mut theirs = remote_manifest(&remote);
        theirs.roms.push(entry("z", 9, "oz", "other"));
        let (_, rev) = remote.get_manifest().unwrap().unwrap();
        remote
            .put_manifest(&theirs.to_encrypted(PASSWORD).unwrap(), Some(&rev))
            .unwrap();

        // act
        cp.add(&target_for("b", 2, None), entry("b", 2, "ob", "me"));
        cp.commit().unwrap();

        // assert
        let names: Vec<String> = remote_manifest(&remote)
            .roms
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["a", "b", "z"]);
    }

    #[test]
    fn checkpoint_commit_without_pending_leaves_the_remote_untouched() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let library = library_in(&temp);
        let mut index = Index::default();
        let mut cp = checkpoint(&remote, &library, &mut index, Manifest::default(), None);

        // act
        cp.commit().unwrap();

        // assert
        assert!(remote.get_manifest().unwrap().is_none());
        assert_eq!(cp.recorded, 0);
    }

    #[test]
    fn checkpoint_is_due_once_enough_was_uploaded() {
        // arrange
        let temp = TempDir::new().unwrap();
        let remote = LocalDir::new(temp.path().join("remote"));
        let library = library_in(&temp);
        let mut index = Index::default();
        let mut cp = checkpoint(&remote, &library, &mut index, Manifest::default(), None);
        let mut big = entry("big", 2, "obig", "me");
        big.archive_size = CHECKPOINT_BYTES;

        // act
        cp.add(
            &target_for("small", 1, None),
            entry("small", 1, "osmall", "me"),
        );
        let after_small = cp.due();
        cp.add(&target_for("big", 2, None), big);
        let after_big = cp.due();

        // assert
        assert!(!after_small);
        assert!(after_big);
    }

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
