use colored::Colorize;

use crate::credentials::{self, Secret};
use crate::format::human_size;
use crate::library::Library;
use crate::manifest::Manifest;
use crate::password;
use crate::remote::{self, dropbox};
use crate::sync::Selection;
use crate::system::System;

fn current_library() -> anyhow::Result<Library> {
    Library::discover(&std::env::current_dir()?)
}

pub fn login() -> anyhow::Result<()> {
    let library = current_library()?;
    match library.config.remote.backend.as_str() {
        "dropbox" => {
            dropbox::login()?;
            let remote = remote::open(&library.config.remote)?;
            println!("{} logged in: {}", "✓".green(), remote.describe()?);
        }
        other => println!("the {other} backend needs no login"),
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
    dropbox::logout()?;
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
    if entries.is_empty() {
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
    println!();
    println!(
        "{} files, {} ({} on this machine)",
        entries.len(),
        human_size(total_size),
        present
    );
    Ok(())
}
