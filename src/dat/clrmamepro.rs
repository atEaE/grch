use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use super::{Dat, DatEntry, hex_array};

static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"name "([^"]+)""#).unwrap());
static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bsize (\d+)\b").unwrap());
static CRC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bcrc ([0-9A-Fa-f]{8})\b").unwrap());
static MD5: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bmd5 ([0-9A-Fa-f]{32})\b").unwrap());
static SHA1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bsha1 ([0-9A-Fa-f]{40})\b").unwrap());
static SERIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bserial "([^"]*)""#).unwrap());
static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bversion "([^"]+)""#).unwrap());

pub fn parse(body: &str) -> Result<Dat> {
    let mut dat = Dat::new(None);

    let mut in_header = true;
    for line in body.lines() {
        if in_header {
            if line.trim_start().starts_with("game (") {
                in_header = false;
            } else if dat.version.is_none()
                && let Some(caps) = VERSION.captures(line)
            {
                dat.version = Some(caps[1].to_string());
            }
        }

        let Some(name_caps) = NAME.captures(line) else {
            continue;
        };
        // Search after the name so a name containing "crc" or "size" is not picked up.
        let rest = &line[name_caps.get(0).unwrap().end()..];
        let Some(crc_caps) = CRC.captures(rest) else {
            continue;
        };

        let entry = DatEntry {
            name: name_caps[1].to_string(),
            size: SIZE.captures(rest).and_then(|c| c[1].parse().ok()),
            crc: u32::from_str_radix(&crc_caps[1], 16).unwrap(),
            md5: MD5.captures(rest).and_then(|c| hex_array(&c[1])),
            sha1: SHA1.captures(rest).and_then(|c| hex_array(&c[1])),
            serial: SERIAL.captures(rest).map(|c| c[1].to_string()),
        };
        dat.push(entry);
    }
    Ok(dat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_all_fields() {
        // arrange
        let body = r#"
clrmamepro (
	name "Nintendo - Game Boy Advance"
	description "Nintendo - Game Boy Advance"
	version "2026.08.01"
	homepage "http://github.com/robloach/libretro-dats"
)

game (
	name "007 - Everything or Nothing (Japan)"
	region "Japan"
	serial "BJBJ"
	rom ( name "007 - Everything or Nothing (Japan).gba" size 8388608 crc D793E969 md5 55354D9E3BC9C1FA682B5110E5ED1544 sha1 6E4E9BE9A07580EF267BE9C2EA1BD0730B3BE44A serial "BJBJ" )
)
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert_eq!(dat.version.as_deref(), Some("2026.08.01"));
        assert_eq!(dat.entries.len(), 1);
        let entry = &dat.entries[0];
        assert_eq!(entry.name, "007 - Everything or Nothing (Japan).gba");
        assert_eq!(entry.size, Some(8388608));
        assert_eq!(entry.crc, 0xD793E969);
        assert_eq!(
            entry.md5,
            Some([
                0x55, 0x35, 0x4D, 0x9E, 0x3B, 0xC9, 0xC1, 0xFA, 0x68, 0x2B, 0x51, 0x10, 0xE5, 0xED,
                0x15, 0x44
            ])
        );
        assert_eq!(
            entry.sha1,
            Some([
                0x6E, 0x4E, 0x9B, 0xE9, 0xA0, 0x75, 0x80, 0xEF, 0x26, 0x7B, 0xE9, 0xC2, 0xEA, 0x1B,
                0xD0, 0x73, 0x0B, 0x3B, 0xE4, 0x4A
            ])
        );
        assert_eq!(entry.serial.as_deref(), Some("BJBJ"));
    }

    #[test]
    fn parse_without_header_and_optional_fields() {
        // arrange
        let body = r#"
game (
	name "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced)"
	region "Japan"
	rom ( name "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced).gb" size 1048576 crc A2545D33 md5 96C1F411671B6E1761CF31884DDE0DBB sha1 28E4B8531EA4EA1DE5A396FCCB0CFBA51B06B149 )
)
game (
	name "Super Mario Land (World)"
	rom ( name "Super Mario Land (World).gb" crc 90776841 )
)
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert_eq!(dat.version, None);
        assert_eq!(dat.entries.len(), 2);
        {
            let entry = dat.entries.iter().find(|e| e.crc == 0xA2545D33).unwrap();
            assert_eq!(
                entry.name,
                "Pocket Monsters - Pikachu (Japan) (Rev 1) (SGB Enhanced).gb"
            );
            assert_eq!(entry.serial, None);
        }
        {
            let entry = dat.entries.iter().find(|e| e.crc == 0x90776841).unwrap();
            assert_eq!(entry.name, "Super Mario Land (World).gb");
            assert_eq!(entry.size, None);
            assert_eq!(entry.md5, None);
            assert_eq!(entry.sha1, None);
            assert_eq!(entry.serial, None);
        }
    }

    #[test]
    fn parse_skips_rom_without_crc() {
        // arrange
        let body = r#"
game (
	name "No Dump (Japan)"
	rom ( name "No Dump (Japan).gb" size 65536 flags nodump )
)
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert!(dat.entries.is_empty());
    }

    #[test]
    fn parse_version_only_from_header() {
        // arrange
        let body = r#"
game (
	name "Some Game version "9.9.9" (Japan)"
	rom ( name "Some Game.gb" crc 00000001 )
)
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert_eq!(dat.version, None);
        assert_eq!(dat.entries.len(), 1);
    }
}
