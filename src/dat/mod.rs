use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::dir;
use crate::hash::FileHashes;
use crate::system::System;

mod clrmamepro;
mod logiqx;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatEntry {
    pub name: String,
    pub size: Option<u64>,
    pub crc: u32,
    pub md5: Option<[u8; 16]>,
    pub sha1: Option<[u8; 20]>,
    pub serial: Option<String>,
}

/// Lookups prefer entries pushed later, so `load_merged` can append the custom DAT
/// after the official one and have it take precedence.
#[derive(Debug)]
pub struct Dat {
    pub version: Option<String>,
    pub entries: Vec<DatEntry>,
    by_crc: HashMap<u32, Vec<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedBy {
    Sha1,
    Md5,
    Crc,
}

impl fmt::Display for MatchedBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MatchedBy::Sha1 => "sha1",
            MatchedBy::Md5 => "md5",
            MatchedBy::Crc => "crc",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match<'a> {
    pub entry: &'a DatEntry,
    pub by: MatchedBy,
}

impl Dat {
    pub fn new(version: Option<String>) -> Self {
        Dat {
            version,
            entries: Vec::new(),
            by_crc: HashMap::new(),
        }
    }

    pub fn push(&mut self, entry: DatEntry) {
        let index = self.entries.len();
        self.by_crc.entry(entry.crc).or_default().push(index);
        self.entries.push(entry);
    }

    pub fn append(&mut self, other: Dat) {
        for entry in other.entries {
            self.push(entry);
        }
    }

    pub fn candidates_by_crc(&self, crc: u32) -> impl Iterator<Item = &DatEntry> {
        self.by_crc
            .get(&crc)
            .into_iter()
            .flatten()
            .map(|&i| &self.entries[i])
    }

    /// Match by crc and size, then confirm with sha1 or md5 when the entry has one.
    pub fn find(&self, hashes: &FileHashes) -> Option<Match<'_>> {
        self.candidates_by_crc(hashes.crc)
            .filter(|e| e.size.is_none_or(|size| size == hashes.size))
            .filter_map(|e| {
                let by = match (e.sha1, e.md5) {
                    (Some(sha1), _) if sha1 == hashes.sha1 => MatchedBy::Sha1,
                    (Some(_), _) => return None,
                    (None, Some(md5)) if md5 == hashes.md5 => MatchedBy::Md5,
                    (None, Some(_)) => return None,
                    (None, None) => MatchedBy::Crc,
                };
                Some(Match { entry: e, by })
            })
            .last()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Clrmamepro,
    Logiqx,
}

/// Sniff the format from the body. Custom DATs from `dat add` can be either format,
/// so this cannot be decided from the system or URL.
fn detect_format(body: &str) -> Result<Format> {
    let head = body.trim_start_matches('\u{feff}').trim_start();
    if head.starts_with("<?xml") || head.starts_with("<!DOCTYPE") || head.starts_with("<datafile") {
        return Ok(Format::Logiqx);
    }
    // Hand-written clrmamepro DATs may omit the header block.
    if head.starts_with("clrmamepro") || head.starts_with("game (") {
        return Ok(Format::Clrmamepro);
    }
    bail!("unrecognized DAT format (expected clrmamepro or Logiqx XML)")
}

/// Parse DAT data to a model
pub fn parse(body: &str) -> Result<Dat> {
    match detect_format(body)? {
        Format::Clrmamepro => clrmamepro::parse(body),
        Format::Logiqx => logiqx::parse(body),
    }
}

fn hex_array<const N: usize>(hex: &str) -> Option<[u8; N]> {
    if hex.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// Obtain the DAT file of the ROM corresponding to the target system
fn fetch_body(system: &System) -> Result<String> {
    let url = system.dat_url();
    let mut res = ureq::get(&url).call()?;
    let body = res.body_mut().read_to_string()?;
    Ok(body)
}

/// Load DAT file information of the target system
/// If a cache exists, read from the cache unless refresh is set
pub fn load(system: &System, refresh: bool) -> Result<Dat> {
    let body = read_official_body(system, refresh)?;
    parse(&body).with_context(|| format!("parse official DAT for {}", system.name()))
}

/// Load custom DAT file information of the target system
pub fn load_custom(system: &System) -> Result<Option<Dat>> {
    let Some(body) = read_custom_body(system)? else {
        return Ok(None);
    };
    let dat = parse(&body).with_context(|| format!("parse custom DAT for {}", system.name()))?;
    Ok(Some(dat))
}

/// Load with the official DAT and custom DAT merged.
/// Entries from the custom DAT take priority over the official ones.
pub fn load_merged(system: &System, refresh: bool) -> Result<Dat> {
    let mut dat = load(system, refresh)?;
    if let Some(custom) = load_custom(system)? {
        dat.append(custom);
    }
    Ok(dat)
}

fn read_official_body(system: &System, refresh: bool) -> Result<String> {
    let dir = dir::romdat_cache_dir()?;
    if !refresh && let Some(body) = read_cache(&dir, system)? {
        return Ok(body);
    }
    let body = fetch_body(system)?;
    write_cache(&dir, system, &body)?;
    Ok(body)
}

pub fn read_custom_body(system: &System) -> Result<Option<String>> {
    let dir = dir::custom_dat_dir()?;
    let path = dir.join(format!("{}.dat", system.name()));
    if !path.exists() {
        return Ok(None);
    }

    let body = fs::read_to_string(&path)?;
    Ok(Some(body))
}

/// Generate a .DAT file path for caching
fn dat_path(dir: &Path, system: &System) -> PathBuf {
    dir.join(format!("{}.dat", system.name()))
}

/// Cache read
fn read_cache(dir: &Path, system: &System) -> Result<Option<String>> {
    let file = dat_path(dir, system);
    if !file.exists() {
        return Ok(None);
    }
    let body = fs::read_to_string(&file)?;
    Ok(Some(body))
}

/// Cache the DAT file of the target system
fn write_cache(dir: &Path, system: &System, body: &str) -> Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dat_path(dir, system), body)?;
    Ok(())
}

#[cfg(test)]
mod cache_tests {

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_cache_not_exists() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().join("grch");

        // act
        let result = read_cache(&dir, &System::Gba).unwrap();

        // assert
        assert_eq!(result, None);
    }

    #[test]
    fn read_cache_dat() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().join("grch");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dat_path(&dir, &System::Gb), "test body").unwrap();

        // act
        let result = read_cache(&dir, &System::Gb).unwrap();

        // assert
        assert_eq!(result.as_deref(), Some("test body"));
    }

    #[test]
    fn write_cache_new() {
        // arrange
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().join("grch");

        // act
        write_cache(&dir, &System::Gb, "test cache here").unwrap();

        // assert
        let rom_dat_path = dat_path(&dir, &System::Gb);
        assert!(rom_dat_path.exists());

        let body = fs::read_to_string(rom_dat_path).unwrap();
        assert_eq!(body, "test cache here")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, crc: u32, sha1: Option<[u8; 20]>) -> DatEntry {
        DatEntry {
            name: name.to_string(),
            size: None,
            crc,
            md5: None,
            sha1,
            serial: None,
        }
    }

    fn hashes(crc: u32, size: u64, md5: u8, sha1: u8) -> FileHashes {
        FileHashes {
            size,
            crc,
            md5: [md5; 16],
            sha1: [sha1; 20],
        }
    }

    #[test]
    fn detect_format_clrmamepro() {
        // act & assert
        assert_eq!(
            detect_format("clrmamepro (\n\tname \"x\"\n)\n").unwrap(),
            Format::Clrmamepro
        );
        assert_eq!(
            detect_format("\n\n  clrmamepro (\n").unwrap(),
            Format::Clrmamepro
        );
        assert_eq!(
            detect_format("game (\n\tname \"x\"\n)\n").unwrap(),
            Format::Clrmamepro
        );
    }

    #[test]
    fn detect_format_logiqx() {
        // act & assert
        assert_eq!(
            detect_format("<?xml version=\"1.0\"?>\n<datafile/>").unwrap(),
            Format::Logiqx
        );
        assert_eq!(
            detect_format("\u{feff}<?xml version=\"1.0\"?>\n<datafile/>").unwrap(),
            Format::Logiqx
        );
        assert_eq!(
            detect_format("  <datafile></datafile>").unwrap(),
            Format::Logiqx
        );
        assert_eq!(
            detect_format("<!DOCTYPE datafile>\n<datafile/>").unwrap(),
            Format::Logiqx
        );
    }

    #[test]
    fn detect_format_unknown_is_error() {
        // act & assert
        assert!(detect_format("").is_err());
        assert!(detect_format("   \n").is_err());
        assert!(detect_format("{\"name\": \"x\"}").is_err());
        assert!(detect_format("name,crc\nfoo,12345678\n").is_err());
    }

    #[test]
    fn parse_dispatches_by_format() {
        // arrange
        let clr = "game (\n\trom ( name \"a.gb\" crc 00000001 )\n)\n";
        let xml = "<?xml version=\"1.0\"?><datafile><game name=\"b\"><rom name=\"b.gb\" crc=\"00000002\"/></game></datafile>";

        // act
        let clr_dat = parse(clr).unwrap();
        let xml_dat = parse(xml).unwrap();

        // assert
        assert_eq!(clr_dat.entries[0].name, "a.gb");
        assert_eq!(xml_dat.entries[0].name, "b.gb");
        assert!(parse("not a dat").is_err());
    }

    #[test]
    fn hex_array_parses_fixed_length() {
        // act & assert
        assert_eq!(hex_array::<4>("DEADbeef"), Some([0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(hex_array::<4>("DEADBE"), None);
        assert_eq!(hex_array::<4>("DEADBEEF00"), None);
        assert_eq!(hex_array::<4>("GGGGGGGG"), None);
    }

    #[test]
    fn candidates_by_crc_returns_all_on_collision() {
        // arrange
        let mut dat = Dat::new(None);
        dat.push(entry("first.gb", 0xAAAAAAAA, None));
        dat.push(entry("other.gb", 0xBBBBBBBB, None));
        dat.push(entry("second.gb", 0xAAAAAAAA, None));

        // act
        let candidates: Vec<&str> = dat
            .candidates_by_crc(0xAAAAAAAA)
            .map(|e| e.name.as_str())
            .collect();

        // assert
        assert_eq!(candidates, vec!["first.gb", "second.gb"]);
        assert!(dat.candidates_by_crc(0xCCCCCCCC).next().is_none());
    }

    #[test]
    fn find_confirms_with_sha1() {
        // arrange
        let mut dat = Dat::new(None);
        dat.push(DatEntry {
            size: Some(3),
            md5: Some([0x22; 16]),
            ..entry("a.gb", 0x1, Some([0x11; 20]))
        });

        // act & assert
        let found = dat.find(&hashes(0x1, 3, 0x22, 0x11)).unwrap();
        assert_eq!(found.entry.name, "a.gb");
        assert_eq!(found.by, MatchedBy::Sha1);
        // crc collides but sha1 differs
        assert!(dat.find(&hashes(0x1, 3, 0x22, 0x99)).is_none());
        // size differs
        assert!(dat.find(&hashes(0x1, 4, 0x22, 0x11)).is_none());
        assert!(dat.find(&hashes(0x2, 3, 0x22, 0x11)).is_none());
    }

    #[test]
    fn find_falls_back_to_md5_then_crc() {
        // arrange
        let mut dat = Dat::new(None);
        dat.push(DatEntry {
            md5: Some([0x22; 16]),
            ..entry("md5-only.gb", 0x1, None)
        });
        dat.push(entry("crc-only.gb", 0x2, None));

        // act & assert
        let by_md5 = dat.find(&hashes(0x1, 3, 0x22, 0x00)).unwrap();
        assert_eq!(by_md5.entry.name, "md5-only.gb");
        assert_eq!(by_md5.by, MatchedBy::Md5);
        assert!(dat.find(&hashes(0x1, 3, 0x99, 0x00)).is_none());

        let by_crc = dat.find(&hashes(0x2, 3, 0x00, 0x00)).unwrap();
        assert_eq!(by_crc.entry.name, "crc-only.gb");
        assert_eq!(by_crc.by, MatchedBy::Crc);
    }

    #[test]
    fn find_prefers_later_entry_on_collision() {
        // arrange
        let mut dat = Dat::new(None);
        dat.push(entry("first.gb", 0x1, Some([0x11; 20])));
        dat.push(entry("second.gb", 0x1, Some([0x11; 20])));
        dat.push(entry("rejected.gb", 0x1, Some([0x99; 20])));

        // act & assert
        assert_eq!(
            dat.find(&hashes(0x1, 0, 0, 0x11)).unwrap().entry.name,
            "second.gb"
        );
    }

    #[test]
    fn append_prefers_appended_entries() {
        // arrange
        let mut official = Dat::new(Some("2026.08.01".to_string()));
        official.push(entry("Official Name.gb", 0xA2545D33, Some([0x01; 20])));
        official.push(entry("Only Official.gb", 0x90776841, None));

        let mut custom = Dat::new(Some("custom".to_string()));
        custom.push(entry("Custom Name.gb", 0xA2545D33, Some([0x01; 20])));
        custom.push(entry("Only Custom.gb", 0x12345678, None));

        // act
        official.append(custom);

        // assert
        assert_eq!(official.version.as_deref(), Some("2026.08.01"));
        assert_eq!(official.entries.len(), 4);
        let name = |crc, sha1| {
            official
                .find(&hashes(crc, 0, 0, sha1))
                .unwrap()
                .entry
                .name
                .clone()
        };
        assert_eq!(name(0xA2545D33, 0x01), "Custom Name.gb");
        assert_eq!(name(0x90776841, 0x00), "Only Official.gb");
        assert_eq!(name(0x12345678, 0x00), "Only Custom.gb");
    }

    #[test]
    fn append_custom_without_sha1_still_wins() {
        // arrange
        let mut official = Dat::new(None);
        official.push(entry("Official Name.gb", 0x1, Some([0x11; 20])));
        let mut custom = Dat::new(None);
        custom.push(entry("Custom Name.gb", 0x1, None));

        // act
        official.append(custom);

        // assert
        let found = official.find(&hashes(0x1, 0, 0, 0x11)).unwrap();
        assert_eq!(found.entry.name, "Custom Name.gb");
        assert_eq!(found.by, MatchedBy::Crc);
    }
}
