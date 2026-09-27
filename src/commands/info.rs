use clap::ValueEnum;

use crate::dir;
use crate::system::System;

pub fn run() -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    println!("grch v{}", version);
    println!();

    let cache_dir = dir::romdat_cache_dir()?;
    let custom_dat_dir = dir::custom_dat_dir()?;
    let custom_only: Vec<&str> = System::value_variants()
        .iter()
        .filter(|s| s.dat_url().is_none())
        .map(|s| s.name())
        .collect();

    println!("cache dir:    {}", cache_dir.display());
    println!("custom dir:   {}", custom_dat_dir.display());
    println!("dat official: https://github.com/libretro/libretro-database");
    println!(
        "dat custom:   {} (no official DAT; register a No-Intro \"(Encrypted)\" set from https://datomatic.no-intro.org/ with `dat add`)",
        custom_only.join(", ")
    );
    Ok(())
}
