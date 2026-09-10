use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use colored::Colorize;

use crate::dat;
use crate::input;
use crate::system::System;

pub fn run(input: &[PathBuf], refresh: bool, yes: bool) -> anyhow::Result<()> {
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
        let map = dat::load_merged(system, refresh)?;
        dats.insert(*system, map);
    }

    let mut plan: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut already_correct = 0;
    let mut unknown = 0;
    for path in &files {
        let filename = path.file_name().unwrap_or(path.as_os_str()).display();
        let Some(system) = System::from_path(path) else {
            continue;
        };

        match fs::read(path) {
            Ok(data) => {
                let crc = crc32fast::hash(&data);
                let Some(entry) = dats[&system].get(&crc) else {
                    unknown += 1;
                    continue;
                };

                let new_path = new_path_for(path, &entry.name);
                if *path != new_path {
                    println!("- {}", filename);
                    println!("   └ {}", entry.name);
                    plan.push((path.clone(), new_path));
                } else {
                    already_correct += 1;
                }
            }
            Err(e) => {
                eprintln!("skip: {} ({})", path.display(), e);
                continue;
            }
        }
    }

    if plan.is_empty() {
        if already_correct > 0 && unknown == 0 {
            println!("all files already match their official No-Intro names");
        } else if already_correct > 0 && unknown > 0 {
            println!(
                "no files to rename ({} already correct, {} unknown)",
                already_correct, unknown
            );
        } else if unknown > 0 {
            println!("no files matched the database");
        } else {
            println!("no supported files found");
        }
        return Ok(());
    }

    if !yes {
        print!("Rename {} files? [y/N]: ", plan.len());
        io::stdout().flush()?;

        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("aborted");
            return Ok(());
        }
    }

    for (from, to) in plan {
        fs::rename(&from, &to).with_context(|| format!("rename file: {}", from.display()))?;

        let filename = to.file_name().unwrap_or(to.as_os_str()).display();
        println!("{} {}", "✓".green(), filename);
    }

    Ok(())
}

fn new_path_for(path: &Path, entry_name: &str) -> PathBuf {
    path.with_file_name(entry_name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::commands::rename::new_path_for;

    #[test]
    fn new_path_for_with_ext() {
        // arrange
        let path = Path::new("foo/bar/hoge.gba");
        let entry_name = "Pocket Monsters - Aka (Japan) (Rev 1) (SGB Enhanced).gb";

        // act
        let new_path = new_path_for(path, entry_name);

        // assert
        assert_eq!(
            new_path,
            Path::new("foo/bar/Pocket Monsters - Aka (Japan) (Rev 1) (SGB Enhanced).gb")
        );
    }
}
