use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use colored::Colorize;

use crate::dat;
use crate::input;
use crate::system::System;

pub fn run(input: &[PathBuf]) -> anyhow::Result<()> {
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
        let body = dat::load(system)?;
        dats.insert(*system, dat::parse_dat(&body));
    }

    for path in &files {
        let filename = path.file_name().unwrap_or(path.as_os_str()).display();
        let Some(system) = System::from_path(path) else {
            eprintln!("{} {} (unsupported file type)", "-".dimmed(), filename);
            continue;
        };

        match fs::read(path) {
            Ok(data) => {
                let crc = crc32fast::hash(&data);
                if let Some(entry) = dats[&system].get(&crc) {
                    println!(
                        "{} {} -> {:08X} {}",
                        "✓".green(),
                        filename,
                        entry.crc,
                        entry.name
                    )
                } else {
                    println!("{} {} (unknown crc: {:08X})", "✗".red(), filename, crc)
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
