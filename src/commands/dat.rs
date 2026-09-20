use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Ok, bail};

use crate::dat;
use crate::dir;
use crate::system::System;

pub fn add(system: &System, input: &PathBuf) -> anyhow::Result<()> {
    let body = fs::read_to_string(input)
        .with_context(|| format!("failed to read DAT file: {}", input.display()))?;

    let parsed = dat::parse(&body)
        .with_context(|| format!("failed to parse DAT file: {}", input.display()))?;
    if parsed.entries.is_empty() {
        bail!("no valid entries found in {}", input.display())
    }

    let data_dir = dir::custom_dat_dir()?;
    fs::create_dir_all(&data_dir)?;

    let path = data_dir.join(format!("{}.dat", system.name()));
    fs::write(path, body)?;

    println!(
        "registered {} entries for {}",
        parsed.entries.len(),
        system.name()
    );
    Ok(())
}

pub fn show(system: &System) -> anyhow::Result<()> {
    let Some(body) = dat::read_custom_body(system)? else {
        bail!("no custom DAT registered for {}", system.name());
    };

    print!("{}", body);
    Ok(())
}

pub fn ls() -> anyhow::Result<()> {
    let dir = dir::custom_dat_dir()?;
    if !dir.exists() {
        println!("no custom dat");
        return Ok(());
    }

    let mut lists = Vec::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("dat") {
            continue;
        }

        let filename = path.file_name().unwrap_or(path.as_os_str()).display();
        let body = fs::read_to_string(&path)?;
        let version = dat::parse(&body)
            .ok()
            .and_then(|d| d.version)
            .unwrap_or_else(|| "-".to_string());
        let modified = entry.metadata()?.modified()?;
        let modified_chrono: chrono::DateTime<chrono::Local> = modified.into();

        lists.push((
            filename.to_string(),
            version,
            modified_chrono.format("%Y-%m-%d %H:%M").to_string(),
        ));
    }

    if lists.is_empty() {
        println!("no custom dat");
        return Ok(());
    }

    lists.sort();
    for (filename, version, modified) in lists {
        println!("- {}", filename);
        println!("  version: {}   modified: {}", version, modified);
    }

    Ok(())
}
