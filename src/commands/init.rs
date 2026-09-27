use std::path::Path;

use colored::Colorize;

use crate::index::Index;
use crate::library::Library;

pub fn run(dir: &Path) -> anyhow::Result<()> {
    let (library, report) = Library::init(dir, "dropbox")?;

    println!("initialized {}", library.grch_dir().display());
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

    let files = library.scan()?;
    let mut index = Index::load(&library)?;
    let hashed = index.refresh(&files)?;
    index.save(&library)?;
    println!();
    println!("indexed {} files ({} hashed)", index.roms.len(), hashed);
    Ok(())
}
