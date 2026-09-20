use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use colored::Colorize;

use crate::dat;
use crate::hash;
use crate::input;
use crate::system::System;

pub fn run(input: &[PathBuf], refresh: bool) -> anyhow::Result<()> {
    let files = input::collect_files(input);
    if files.is_empty() {
        anyhow::bail!("no files matched");
    }

    let mut systems = HashSet::new();
    for path in &files {
        if let Some(system) = System::from_path(path) {
            systems.insert(system);
        }
    }

    let mut dats = HashMap::new();
    for system in &systems {
        let merged = dat::load_merged(system, refresh)?;
        dats.insert(*system, merged);
    }

    for path in &files {
        let filename = path.file_name().unwrap_or(path.as_os_str()).display();
        let Some(system) = System::from_path(path) else {
            eprintln!("{} {} (unsupported file type)", "-".dimmed(), filename);
            continue;
        };

        match hash::hash_file(path) {
            Ok(hashes) => {
                if let Some(found) = dats[&system].find(&hashes) {
                    println!("{} {}", "✓".green(), filename);
                    println!(
                        "   └ {:08X} | {} ({})",
                        found.entry.crc, found.entry.name, found.by
                    );
                } else {
                    println!("{} {}", "✗".red(), filename);
                    println!("   └ {:08X} | (no match)", hashes.crc);
                }
            }
            Err(e) => {
                eprintln!("skip: {} ({})", path.display(), e);
                continue;
            }
        }
    }
    Ok(())
}
