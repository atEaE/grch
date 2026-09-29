use std::collections::HashSet;
use std::time::{Duration, SystemTime};

use colored::Colorize;

use crate::credentials::{self, Secret};
use crate::format::human_size;
use crate::library::Library;
use crate::manifest::{self, Manifest};
use crate::password;
use crate::prompt;
use crate::remote::{self, ManifestConflict, ObjectInfo, dropbox};
use crate::sync::{Kind, Selection};
use crate::system::System;

/// Attempts at updating the manifest when other machines keep pushing in between.
const MANIFEST_RETRIES: usize = 3;
/// Objects younger than this may belong to a push still running on another machine
/// (objects land before the manifest). A large system can take hours, hence a day.
const GC_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

fn current_library() -> anyhow::Result<Library> {
    Library::discover(&std::env::current_dir()?)
}

/// `app_key` (from `--app-key`) is written to `.grch/config.toml` first, so it is only
/// needed the first time a library is set up.
pub fn login(app_key: Option<&str>) -> anyhow::Result<()> {
    let mut library = current_library()?;
    match library.config.remote.backend.as_str() {
        "dropbox" => {
            if let Some(key) = app_key {
                let key = super::init::clean_app_key(key)?;
                if library.config.remote.app_key.as_deref() != Some(key.as_str()) {
                    library.config.remote.app_key = Some(key);
                    library.save_config()?;
                    println!("app key stored in {}", library.grch_dir().display());
                }
            }
            let key = dropbox::app_key(&library.config.remote)?;
            dropbox::login(key)?;
            let remote = remote::open(&library.config.remote)?;
            println!("{} logged in: {}", "✓".green(), remote.describe()?);
        }
        other => {
            if app_key.is_some() {
                anyhow::bail!("--app-key is for Dropbox; the {other} backend has no app key");
            }
            println!("the {other} backend needs no login");
        }
    }

    if credentials::get(Secret::ArchivePassword)?.is_some() {
        println!("archive password already stored (use `grch remote password` to change it)");
    } else {
        password::prompt_and_store()?;
        println!("{} archive password stored", "✓".green());
    }
    Ok(())
}

pub fn logout() -> anyhow::Result<()> {
    let library = current_library()?;
    if library.config.remote.backend == "dropbox"
        && let Some(key) = library.config.remote.app_key.as_deref()
    {
        dropbox::logout(key)?;
    }
    credentials::delete(Secret::ArchivePassword)?;
    println!(
        "{} removed the Dropbox token and archive password from this machine",
        "✓".green()
    );
    Ok(())
}

pub fn set_password() -> anyhow::Result<()> {
    password::prompt_and_store()?;
    println!("{} archive password stored", "✓".green());
    Ok(())
}

pub fn info() -> anyhow::Result<()> {
    let library = current_library()?;
    println!("library: {}", library.root.display());
    println!("backend: {}", library.config.remote.backend);
    if library.config.remote.backend == "dropbox" {
        match library.config.remote.app_key.as_deref() {
            Some(key) => println!("app key: {key}"),
            None => println!(
                "app key: {} none (run `grch remote login --app-key <KEY>`)",
                "✗".red()
            ),
        }
    }

    let remote = match remote::open(&library.config.remote) {
        Ok(remote) => remote,
        Err(e) => {
            println!("remote:  {} {}", "✗".red(), e);
            return Ok(());
        }
    };
    println!("remote:  {}", remote.describe()?);

    let password_source = if std::env::var_os(password::ENV_VAR).is_some() {
        "environment"
    } else if credentials::get(Secret::ArchivePassword)?.is_some() {
        "credential store"
    } else {
        "prompt"
    };
    println!("archive password: {password_source}");

    match Manifest::fetch(remote.as_ref())? {
        None => println!("manifest: none (nothing pushed yet)"),
        Some((manifest, _)) => {
            println!(
                "manifest: {} roms, updated {} by {}",
                manifest.roms.len(),
                manifest.updated_at,
                manifest.updated_by
            );
        }
    }
    Ok(())
}

/// List what the remote holds, marking files already present on this machine.
pub fn ls(system: Option<System>, patterns: &[String]) -> anyhow::Result<()> {
    let selection = Selection::parse(system, patterns)?;
    let library = current_library()?;
    let remote = remote::open(&library.config.remote)?;
    let Some((manifest, _)) = Manifest::fetch(remote.as_ref())? else {
        println!("remote has no manifest yet (nothing pushed)");
        return Ok(());
    };

    let mut entries: Vec<_> = manifest
        .roms
        .iter()
        .filter(|e| selection.matches(e.system, &e.name))
        .collect();
    if entries.is_empty() && manifest.dats.is_empty() {
        println!("no matching files on the remote");
        return Ok(());
    }
    entries.sort_by(|a, b| (a.system, &a.name).cmp(&(b.system, &b.name)));

    let mut total_size = 0;
    let mut present = 0;
    let mut current: Option<System> = None;
    for entry in &entries {
        if current != Some(entry.system) {
            if current.is_some() {
                println!();
            }
            let count = entries.iter().filter(|e| e.system == entry.system).count();
            println!("{} ({})", entry.system.name().bold(), count);
            current = Some(entry.system);
        }
        let here = library.system_dir(entry.system).join(&entry.name).is_file();
        let mark = if here { "✓".green() } else { " ".normal() };
        println!(
            "  {} {}  {}",
            mark,
            entry.name,
            human_size(entry.size).dimmed()
        );
        total_size += entry.size;
        present += usize::from(here);
    }
    let dats: Vec<_> = manifest
        .dats
        .iter()
        .filter(|e| selection.matches(e.system, &e.name))
        .collect();
    if !dats.is_empty() {
        println!();
        println!("{} ({})", "dat".bold(), dats.len());
        for entry in &dats {
            let here = library
                .local_path(Kind::Dat, entry.system, &entry.name)?
                .is_file();
            let mark = if here { "✓".green() } else { " ".normal() };
            println!(
                "  {} {}  {}",
                mark,
                entry.name,
                human_size(entry.size).dimmed()
            );
        }
    }

    println!();
    println!(
        "{} files, {} ({} on this machine)",
        entries.len(),
        human_size(total_size),
        present
    );
    Ok(())
}

/// Remove files from the remote. Local copies are left alone; every machine (this one
/// included) is offered the local deletion on its next `pull`.
pub fn rm(system: Option<System>, patterns: &[String], dry_run: bool) -> anyhow::Result<()> {
    let selection = Selection::parse(system, patterns)?;
    if selection.is_empty() {
        anyhow::bail!("give a PATTERN or --system to say what to remove");
    }
    let library = current_library()?;
    let remote = remote::open(&library.config.remote)?;
    let Some((mut manifest, mut rev)) = Manifest::fetch(remote.as_ref())? else {
        println!("remote has no manifest yet (nothing pushed)");
        return Ok(());
    };

    let mut victims: Vec<manifest::Entry> = manifest
        .roms
        .iter()
        .filter(|e| selection.matches(e.system, &e.name))
        .cloned()
        .collect();
    if victims.is_empty() {
        println!("no matching files on the remote");
        return Ok(());
    }
    victims.sort_by(|a, b| (a.system, &a.name).cmp(&(b.system, &b.name)));

    let total: u64 = victims.iter().map(|e| e.archive_size).sum();
    println!(
        "{} ({}, {} on the remote)",
        "remove from remote".bold(),
        victims.len(),
        human_size(total)
    );
    for e in &victims {
        println!("  {} {}/{}", "✗".red(), e.system.name(), e.name);
    }
    if dry_run {
        return Ok(());
    }
    // Always asks: this removes data for every machine.
    if !prompt::confirm(&format!("Remove {} files from the remote?", victims.len()))? {
        println!("aborted");
        return Ok(());
    }

    let password = password::resolve()?;
    let mut removed = Vec::new();
    for attempt in 0..MANIFEST_RETRIES {
        let (keep, skip) = partition_unchanged(&manifest, &victims);
        removed = keep;
        for e in &skip {
            println!(
                "{} {}/{} was pushed again by {} since it was listed, not removed",
                "!".yellow(),
                e.system.name(),
                e.name,
                e.pushed_by
            );
        }
        manifest.roms.retain(|e| {
            !removed
                .iter()
                .any(|v| v.system == e.system && v.name == e.name)
        });
        manifest.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        manifest.updated_by = gethostname::gethostname().to_string_lossy().into_owned();
        match remote.put_manifest(&manifest.to_encrypted(&password)?, Some(&rev)) {
            Ok(_) => break,
            Err(e)
                if e.downcast_ref::<ManifestConflict>().is_some()
                    && attempt + 1 < MANIFEST_RETRIES =>
            {
                println!("another machine pushed in the meantime, retrying");
                let Some((fresh, fresh_rev)) = Manifest::fetch(remote.as_ref())? else {
                    anyhow::bail!("the remote manifest disappeared");
                };
                manifest = fresh;
                rev = fresh_rev;
            }
            Err(e) => return Err(e.context("update manifest")),
        }
    }

    let referenced: std::collections::HashSet<&str> =
        manifest.roms.iter().map(|e| e.object.as_str()).collect();
    for e in removed
        .iter()
        .filter(|e| !referenced.contains(e.object.as_str()))
    {
        if let Err(err) = remote.delete(&e.object) {
            eprintln!("could not delete object {}: {err}", e.object);
        }
    }

    println!();
    println!(
        "{} removed {} files from the remote",
        "✓".green(),
        removed.len()
    );
    println!("local copies are untouched; `grch pull` offers to delete them on each machine");
    Ok(())
}

/// Delete objects the manifest does not reference. `push` uploads objects before it
/// updates the manifest, so an interrupted push (or a lost merge in `update_manifest`)
/// leaves objects behind; names are content hashes of a freshly encrypted archive, so
/// a rerun never reuses them and they would stay forever otherwise.
pub fn gc(dry_run: bool, yes: bool) -> anyhow::Result<()> {
    let library = current_library()?;
    let remote = remote::open(&library.config.remote)?;
    let objects = remote.list_objects()?;
    if objects.is_empty() {
        println!("{} no objects on the remote", "✓".green());
        return Ok(());
    }
    // No manifest means nothing was ever pushed to completion: everything is an orphan.
    let manifest = Manifest::fetch(remote.as_ref())?.map(|(m, _)| m);
    let (mut orphans, recent) = partition_orphans(&objects, manifest.as_ref(), SystemTime::now());
    if !recent.is_empty() {
        println!(
            "{} {} unreferenced objects uploaded within the last {}h were left alone (a push may be in progress)",
            "!".yellow(),
            recent.len(),
            GC_GRACE.as_secs() / 3600
        );
    }
    if orphans.is_empty() {
        println!("{} nothing to clean up", "✓".green());
        return Ok(());
    }
    orphans.sort_by_key(|o| o.modified);

    let total: u64 = orphans.iter().map(|o| o.size).sum();
    println!(
        "{} ({}, {} on the remote)",
        "unreferenced objects".bold(),
        orphans.len(),
        human_size(total)
    );
    for o in &orphans {
        println!(
            "  {} {}  {}  {}",
            "✗".red(),
            &o.name[..o.name.len().min(12)],
            local_date(o.modified).dimmed(),
            human_size(o.size).dimmed()
        );
    }
    if dry_run {
        return Ok(());
    }
    if !yes
        && !prompt::confirm(&format!(
            "Delete {} objects from the remote?",
            orphans.len()
        ))?
    {
        println!("aborted");
        return Ok(());
    }

    // A push elsewhere may have finished while the list was on screen; anything its
    // manifest now references is off the table, whatever we listed.
    let manifest = Manifest::fetch(remote.as_ref())?.map(|(m, _)| m);
    let referenced: HashSet<&str> = manifest.iter().flat_map(|m| m.objects()).collect();
    let mut deleted = 0;
    let mut freed = 0;
    for o in orphans
        .iter()
        .filter(|o| !referenced.contains(o.name.as_str()))
    {
        if let Err(e) = remote.delete(&o.name) {
            eprintln!("could not delete object {}: {e}", o.name);
            continue;
        }
        deleted += 1;
        freed += o.size;
    }

    println!();
    println!(
        "{} deleted {} objects, {} freed",
        "✓".green(),
        deleted,
        human_size(freed)
    );
    Ok(())
}

/// Split objects the manifest does not reference into those old enough to delete and
/// those still inside `GC_GRACE`. Referenced objects are dropped.
fn partition_orphans(
    objects: &[ObjectInfo],
    manifest: Option<&Manifest>,
    now: SystemTime,
) -> (Vec<ObjectInfo>, Vec<ObjectInfo>) {
    let referenced: HashSet<&str> = manifest.iter().flat_map(|m| m.objects()).collect();
    let cutoff = now.checked_sub(GC_GRACE).unwrap_or(std::time::UNIX_EPOCH);
    let mut orphans = Vec::new();
    let mut recent = Vec::new();
    for o in objects {
        if referenced.contains(o.name.as_str()) {
            continue;
        }
        if o.modified > cutoff {
            recent.push(o.clone());
        } else {
            orphans.push(o.clone());
        }
    }
    (orphans, recent)
}

fn local_date(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// Split `victims` into those still on the remote as listed and those another machine
/// re-pushed with different content since (kept, so a fresh dump is not lost).
fn partition_unchanged(
    manifest: &Manifest,
    victims: &[manifest::Entry],
) -> (Vec<manifest::Entry>, Vec<manifest::Entry>) {
    let mut keep = Vec::new();
    let mut skip = Vec::new();
    for v in victims {
        match manifest
            .roms
            .iter()
            .find(|e| e.system == v.system && e.name == v.name)
        {
            Some(current) if (current.size, current.crc32) == (v.size, v.crc32) => {
                keep.push(current.clone())
            }
            Some(current) => skip.push(current.clone()),
            // Already gone: nothing to remove, nothing to report.
            None => {}
        }
    }
    (keep, skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, crc32: u32, by: &str) -> manifest::Entry {
        manifest::Entry {
            system: System::Sfc,
            name: name.to_string(),
            size: 1,
            crc32,
            object: format!("obj-{name}-{crc32}"),
            archive_size: 1,
            pushed_at: String::new(),
            pushed_by: by.to_string(),
        }
    }

    #[test]
    fn partition_keeps_unchanged_skips_repushed_ignores_gone() {
        // arrange
        let manifest = Manifest {
            roms: vec![entry("a", 1, "x"), entry("b", 2, "other")],
            ..Manifest::default()
        };
        let victims = vec![entry("a", 1, "x"), entry("b", 1, "x"), entry("c", 1, "x")];

        // act
        let (keep, skip) = partition_unchanged(&manifest, &victims);

        // assert
        assert_eq!(
            keep.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(
            skip.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(skip[0].pushed_by, "other");
    }

    fn object(name: &str, age: Duration, now: SystemTime) -> ObjectInfo {
        ObjectInfo {
            name: name.to_string(),
            size: 10,
            modified: now - age,
        }
    }

    #[test]
    fn partition_orphans_skips_referenced_and_holds_back_recent() {
        // arrange
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 60 * 60);
        let manifest = Manifest {
            roms: vec![entry("a", 1, "x")],
            dats: vec![entry("d", 2, "x")],
            ..Manifest::default()
        };
        let objects = vec![
            object("obj-a-1", 3 * day, now),
            object("obj-d-2", 3 * day, now),
            object("old", 3 * day, now),
            object("fresh", Duration::from_secs(60), now),
        ];

        // act
        let (orphans, recent) = partition_orphans(&objects, Some(&manifest), now);

        // assert
        assert_eq!(
            orphans.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            vec!["old"]
        );
        assert_eq!(
            recent.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            vec!["fresh"]
        );
    }

    #[test]
    fn partition_orphans_without_manifest_treats_everything_as_orphan() {
        // arrange
        let now = SystemTime::now();
        let objects = vec![object("x", Duration::from_secs(48 * 60 * 60), now)];

        // act
        let (orphans, recent) = partition_orphans(&objects, None, now);

        // assert
        assert_eq!(orphans.len(), 1);
        assert!(recent.is_empty());
    }
}
