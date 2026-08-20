use std::collections::HashMap;
use std::fs;

use anyhow::{Ok, Result};
use regex::Regex;

use crate::dir;
use crate::system::System;

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
        let body = fetch_body(system)?;
        cache(system, &body)?;
        Ok(body)
    }
}

/// Cache the DAT file of the target system
fn cache(system: &System, body: &str) -> Result<()> {
    let dir = dir::romdat_cache_dir()?;
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(system.name()), body)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dat_extracts_name_and_crc() {
        // arrange
        let body = r#"
game (
	name "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced)"
	region "Japan"
	rom ( name "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced).gb" size 1048576 crc A2545D33 md5 96C1F411671B6E1761CF31884DDE0DBB sha1 28E4B8531EA4EA1DE5A396FCCB0CFBA51B06B149 )
)  
game (
	name "Super Mario Land (World)"
	rom ( name "Super Mario Land (World).gb" size 65536 crc 90776841 md5 B48161623F12F86FEC88320166A21FCE sha1 3A4DDB39B234A67FFB361EE7ABC3D23E0A8B1C89 )
)
"#;

        // act
        let results = parse_dat(body);

        // assert
        assert_eq!(results.len(), 2);

        {
            let pokemon_crc = 0xA2545D33;
            let entry = results.get(&pokemon_crc).unwrap();
            assert_eq!(
                entry.name,
                "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced).gb"
            );
        }
        {
            let mario_crc = 0x90776841;
            let entry = results.get(&mario_crc).unwrap();
            assert_eq!(entry.name, "Super Mario Land (World).gb");
        }
    }
}
