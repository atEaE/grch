use std::fs;
use std::collections::HashMap;

use anyhow::{Context, Ok, Result};
use regex::Regex;

use crate::system::System;
use crate::dir;

/// Obtain the DAT file of the ROM corresponding to the target system
fn fetch_body(system: &System) -> Result<String> {
	let url = system.dat_url();
	let mut res = ureq::get(&url).call()?;
	let body = res.body_mut().read_to_string()?;
	Ok(body)
}

/// Load DAT file information of the target system
/// If a cache exists, read from the cache
pub fn load(system: &System) -> Result<String> {
    let rom_cache = dir::romdat_cache_dir()?;
	let file = rom_cache.join(format!("{}.dat", system.name()));
	
	if file.exists() {
		let body = fs::read_to_string(&file)?;
		Ok(body)
	} else {
		let body = fetch_body(&system)?;
		cache(system, &body)?;
		Ok(body)
	}
}

/// Cache the DAT file of the target system
fn cache(system: &System, body: &str) -> Result<()> {
	let dir = dir::romdat_cache_dir()?;
	fs::create_dir_all(&dir)?;
	fs::write(dir.join(format!("{}", system.name())), body)?;
	Ok(())
}

pub struct DatEntry {
	pub name: String,
	pub crc: u32,
}

/// Parse DAT data to a model
pub fn parse_dat(body: &str) -> HashMap<u32, DatEntry> {
	let rgx = Regex::new(r#"name "([^"]+)".+crc ([0-9A-Fa-f]{8})"#).unwrap();

	let mut map = HashMap::new();
	for line in body.lines() {
		if let Some(caps) = rgx.captures(line) {
			let name = caps[1].to_string();
			let crc = u32::from_str_radix(&caps[2], 16).unwrap();
			map.insert(crc, DatEntry { name, crc });
		}
	}
	map
}