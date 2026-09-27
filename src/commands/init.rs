use std::path::Path;

use colored::Colorize;

use crate::index::Index;
use crate::library::{Library, Remote};

/// `local` switches the remote to a plain directory (testing / no account).
pub fn run(dir: &Path, local: Option<&Path>) -> anyhow::Result<()> {
    let remote = match local {
        Some(path) => {
            let path = std::path::absolute(path)?;
            std::fs::create_dir_all(&path)?;
            Remote {
                backend: "local".to_string(),
                path: Some(path.to_string_lossy().into_owned()),
            }
        }
        None => Remote {
            backend: "dropbox".to_string(),
            path: None,
        },
    };
    let (library, report) = Library::init(dir, remote)?;

    println!("initialized {}", library.grch_dir().display());
    match &library.config.remote.path {
        Some(path) => println!("remote: {} ({})", library.config.remote.backend, path),
        None => println!("remote: {}", library.config.remote.backend),
    }
    for (system, folder) in &library.config.dirs {
        let mark = if report.matched.iter().any(|(s, _)| s == system) {
            "✓".green()
        } else {
            "+".dimmed()
        };
        println!("{} {:<4} -> {}/", mark, system.name(), folder);
    }
    if !report.ignored.is_empty() {
        println!();
        println!(
            "ignored (not a system folder): {}",
            report.ignored.join(", ")
        );
    }

    println!();
    let index = Index::load_refreshed(&library)?;
    println!(
        "indexed {} files, {} custom DATs",
        index.roms.len(),
        index.dats.len()
    );
    Ok(())
}
