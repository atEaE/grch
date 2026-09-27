use colored::Colorize;

use crate::index::Index;
use crate::library::Library;
use crate::manifest::Manifest;
use crate::remote;
use crate::sync::{self, Action, Change, Selection};
use crate::system::System;

pub fn run(system: Option<System>, patterns: &[String]) -> anyhow::Result<()> {
    let selection = Selection::parse(system, patterns)?;
    let library = Library::discover(&std::env::current_dir()?)?;
    let remote = remote::open(&library.config.remote)?;

    let mut index = Index::load(&library)?;
    let hashed = index.refresh(&library.scan()?)?;
    index.save(&library)?;
    if hashed > 0 {
        eprintln!("hashed {} changed files", hashed);
    }

    let manifest = match Manifest::fetch(remote.as_ref())? {
        Some((manifest, _rev)) => manifest,
        None => {
            println!("remote has no manifest yet (nothing pushed)");
            Manifest::default()
        }
    };

    let changes: Vec<Change> = sync::diff(&index, &manifest)
        .into_iter()
        .filter(|c| selection.matches(c.system, c.name))
        .collect();
    print_report(&changes);
    Ok(())
}

fn print_report(changes: &[Change]) {
    let count = |action: Action| changes.iter().filter(|c| c.action == action).count();

    let mut sections: Vec<(&str, colored::ColoredString, Vec<&Change>)> = Vec::new();
    let mut section = |title: &'static str, mark: colored::ColoredString, actions: &[Action]| {
        let items: Vec<&Change> = changes
            .iter()
            .filter(|c| actions.contains(&c.action))
            .collect();
        if !items.is_empty() {
            sections.push((title, mark, items));
        }
    };
    section(
        "push",
        "↑".green(),
        &[Action::PushNew, Action::PushModified],
    );
    section("pull", "↓".cyan(), &[Action::Pull]);
    section("conflict", "!".red(), &[Action::Conflict]);
    section(
        "deleted on remote",
        "✗".yellow(),
        &[Action::DeletedRemotely],
    );
    section("mark synced", "=".dimmed(), &[Action::MarkSynced]);

    for (title, mark, items) in &sections {
        println!("{} ({})", title.bold(), items.len());
        for change in items {
            let note = match change.action {
                Action::PushNew => " (new)",
                Action::PushModified => " (modified)",
                _ => "",
            };
            println!(
                "  {} {}/{}{}",
                mark,
                change.system.name(),
                change.name,
                note
            );
        }
        println!();
    }

    let not_fetched = count(Action::NotFetched);
    if not_fetched > 0 {
        println!(
            "not fetched: {} (pull --all, --system <SYS> or PATTERN to get them)",
            not_fetched
        );
    }
    println!("in sync: {}", count(Action::InSync));
    if sections.is_empty() && not_fetched == 0 {
        println!("{} nothing to push or pull", "✓".green());
    }
}
