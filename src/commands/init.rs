use std::path::Path;

use colored::Colorize;

use crate::index::Index;
use crate::library::{Library, Remote};

/// Trim whitespace and reject an empty key.
pub fn clean_app_key(key: &str) -> anyhow::Result<String> {
    let key = key.trim();
    if key.is_empty() {
        anyhow::bail!("--app-key must not be empty");
    }
    Ok(key.to_string())
}

/// `local` switches the remote to a plain directory (testing / no account); `app_key` is
/// the Dropbox app the library talks to (can also be given later to `remote login`).
pub fn run(dir: &Path, local: Option<&Path>, app_key: Option<&str>) -> anyhow::Result<()> {
    let remote = match local {
        Some(path) => {
            if app_key.is_some() {
                anyhow::bail!("--app-key is for Dropbox; it has no meaning with --local");
            }
            let path = std::path::absolute(path)?;
            std::fs::create_dir_all(&path)?;
            Remote {
                backend: "local".to_string(),
                path: Some(path.to_string_lossy().into_owned()),
                app_key: None,
            }
        }
        None => Remote {
            backend: "dropbox".to_string(),
            path: None,
            app_key: app_key.map(clean_app_key).transpose()?,
        },
    };
    let (library, report) = Library::init(dir, remote)?;

    println!("initialized {}", library.grch_dir().display());
    let remote = &library.config.remote;
    match (&remote.path, &remote.app_key) {
        (Some(path), _) => println!("remote: {} ({})", remote.backend, path),
        (None, Some(key)) => println!("remote: {} (app key {})", remote.backend, key),
        (None, None) => println!(
            "remote: {} (no app key yet; pass it to `grch remote login --app-key <KEY>`)",
            remote.backend
        ),
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
