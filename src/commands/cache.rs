use std::fs;

use anyhow::Context;
use colored::Colorize;

use crate::{dat, dir};

pub fn clean() -> anyhow::Result<()> {
    let parent = dir::grch_cache_dir()?;
    if !parent.exists() {
        println!("grch cache does not exist. Nothing to clean.");
        return Ok(());
    }

    let rom_dat = dir::romdat_cache_dir()?;
    if rom_dat.exists() {
        fs::remove_dir_all(&rom_dat)
            .with_context(|| format!("remove rom_dat cache: {}", rom_dat.display()))?;
    }
    println!("{} /rom_dat cleaned", "✓".green());

    Ok(())
}

pub fn ls() -> anyhow::Result<()> {
    let cache_dir = dir::romdat_cache_dir()?;
    if !cache_dir.exists() {
        println!("no cache");
        return Ok(());
    }

    let mut lists = Vec::new();
    for entry in fs::read_dir(&cache_dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("dat") {
            continue;
        }

        let filename = path.file_name().unwrap_or(path.as_os_str()).display();
        let body = fs::read_to_string(&path)?;
        let version = dat::extract_version(&body).unwrap_or_else(|| "-".to_string());
        let modified = entry.metadata()?.modified()?;
        let modified_chrono: chrono::DateTime<chrono::Local> = modified.into();

        lists.push((
            filename.to_string(),
            version,
            modified_chrono.format("%Y-%m-%d %H:%M").to_string(),
        ));
    }

    lists.sort();
    for (filename, version, modified) in lists {
        println!("- {}", filename);
        println!("  version: {}   modified: {}", version, modified);
    }

    Ok(())
}
