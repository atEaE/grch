use anyhow::{Context, Result, bail};
use roxmltree::{Document, Node, ParsingOptions};

use super::{Dat, DatEntry, hex_array};

pub fn parse(body: &str) -> Result<Dat> {
    // No-Intro and Redump DATs carry a `<!DOCTYPE datafile PUBLIC ...>` line,
    // which roxmltree rejects by default. The external DTD is not fetched.
    let options = ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = Document::parse_with_options(body, options).context("parse Logiqx XML")?;

    let root = doc.root_element();
    if root.tag_name().name() != "datafile" {
        bail!(
            "unexpected root element <{}> (expected <datafile>)",
            root.tag_name().name()
        );
    }

    let version = child(root, "header")
        .and_then(|header| child_text(header, "version"))
        .map(str::to_string);
    let mut dat = Dat::new(version);

    for game in root.children().filter(|n| n.has_tag_name("game")) {
        // No-Intro puts serial on <rom>, Redump on <game>.
        let game_serial = child_text(game, "serial").or_else(|| game.attribute("serial"));

        for rom in game.children().filter(|n| n.has_tag_name("rom")) {
            if rom.attribute("status") == Some("nodump") {
                continue;
            }
            let (Some(name), Some(crc)) = (rom.attribute("name"), rom.attribute("crc")) else {
                continue;
            };
            let Ok(crc) = u32::from_str_radix(crc, 16) else {
                continue;
            };

            let entry = DatEntry {
                name: name.to_string(),
                size: rom.attribute("size").and_then(|s| s.parse().ok()),
                crc,
                md5: rom.attribute("md5").and_then(hex_array),
                sha1: rom.attribute("sha1").and_then(hex_array),
                serial: rom.attribute("serial").or(game_serial).map(str::to_string),
            };
            dat.push(entry);
        }
    }
    Ok(dat)
}

fn child<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|n| n.has_tag_name(name))
}

fn child_text<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    child(node, name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_all_fields() {
        // arrange
        let body = r#"<?xml version="1.0"?>
<!DOCTYPE datafile PUBLIC "-//Logiqx//DTD ROM Management Datafile//EN" "http://www.logiqx.com/Dats/datafile.dtd">
<datafile>
	<header>
		<name>Nintendo - Nintendo 3DS (Decrypted)</name>
		<description>Nintendo - Nintendo 3DS (Decrypted)</description>
		<version>20260901-000000</version>
		<author>No-Intro</author>
	</header>
	<game name="007 - Everything or Nothing (Japan)">
		<description>007 - Everything or Nothing (Japan)</description>
		<rom name="007 - Everything or Nothing (Japan).gba" size="8388608" crc="D793E969" md5="55354D9E3BC9C1FA682B5110E5ED1544" sha1="6E4E9BE9A07580EF267BE9C2EA1BD0730B3BE44A" serial="BJBJ"/>
	</game>
</datafile>
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert_eq!(dat.version.as_deref(), Some("20260901-000000"));
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
    fn parse_serial_from_game_element_and_lowercase_hex() {
        // arrange
        let body = r#"<?xml version="1.0"?>
<datafile>
	<header><name>Sony - PlayStation Portable</name></header>
	<game name="-8 (Japan)">
		<category>Games</category>
		<description>-8 (Japan)</description>
		<serial>ULJM-06340</serial>
		<rom name="-8 (Japan).iso" size="1207468032" crc="b71764f4"/>
	</game>
</datafile>
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        assert_eq!(dat.version, None);
        assert_eq!(dat.entries.len(), 1);
        let entry = &dat.entries[0];
        assert_eq!(entry.name, "-8 (Japan).iso");
        assert_eq!(entry.crc, 0xB71764F4);
        assert_eq!(entry.md5, None);
        assert_eq!(entry.sha1, None);
        assert_eq!(entry.serial.as_deref(), Some("ULJM-06340"));
    }

    #[test]
    fn parse_skips_nodump_and_rom_without_crc() {
        // arrange
        let body = r#"<?xml version="1.0"?>
<datafile>
	<game name="Good">
		<rom name="Good.gb" size="1" crc="00000001" status="verified"/>
	</game>
	<game name="No Dump">
		<rom name="No Dump.gb" size="1" status="nodump"/>
	</game>
	<game name="No Dump With Crc">
		<rom name="No Dump With Crc.gb" size="1" crc="00000002" status="nodump"/>
	</game>
	<game name="No Crc">
		<rom name="No Crc.gb" size="1"/>
	</game>
	<game name="Bad Crc">
		<rom name="Bad Crc.gb" size="1" crc="zzzzzzzz"/>
	</game>
</datafile>
"#;

        // act
        let dat = parse(body).unwrap();

        // assert
        let names: Vec<&str> = dat.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Good.gb"]);
    }

    #[test]
    fn parse_rejects_non_datafile_root() {
        // arrange
        let body = r#"<?xml version="1.0"?><root><game name="x"/></root>"#;

        // act & assert
        assert!(parse(body).is_err());
    }

    #[test]
    fn parse_rejects_broken_xml() {
        // arrange
        let body = r#"<?xml version="1.0"?><datafile><game name="x">"#;

        // act & assert
        assert!(parse(body).is_err());
    }
}
