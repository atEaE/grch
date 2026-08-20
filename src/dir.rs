use std::path::PathBuf;

use anyhow::{Context, Ok, Result};

pub fn grch_cache_dir() -> Result<PathBuf> {
    let cached = dirs::cache_dir().context("cache dir not found")?;
    Ok(cached.join("grch"))
}

pub fn romdat_cache_dir() -> Result<PathBuf> {
    let grch_cache = grch_cache_dir()?;
    let rom_dir = grch_cache.join("rom_dat");
    Ok(rom_dir)
}
