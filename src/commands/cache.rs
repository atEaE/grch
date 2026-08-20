use std::fs;

use anyhow::Context;

use crate::dir;

use colored::Colorize;

pub fn clean() -> anyhow::Result<()> {
    let parent = dir::grch_cache_dir()?;
    if !parent.exists() {
        println!("grch cache not exists. may be already deleted?");
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
