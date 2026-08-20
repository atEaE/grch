use std::path::PathBuf;
use std::fs;

use anyhow::{Context, Result, Ok};

pub fn grch_cache_dir() -> Result<PathBuf> {
	let cached = dirs::cache_dir().context("cache dir not found")?;
	Ok(cached.join("grch"))
}

pub fn romdat_cache_dir() -> Result<PathBuf> {
	let grch_cache = grch_cache_dir()?;
	let rom_dir = grch_cache.join("rom");
	Ok(rom_dir)
}

pub fn init_cache_dir() -> Result<()> {
	fs::create_dir_all(romdat_cache_dir()?)?;
	Ok(())
}