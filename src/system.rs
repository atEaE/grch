use clap::ValueEnum;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum System {
    /// Game Boy
    Gb,
    /// Game Boy Color
    Gbc,
    /// Game Boy Advance
    Gba,
    /// Super Family Computer
    Sfc,
    /// Nintendo DS
    Nds,
    /// Nintendo 3DS
    // Rust identifiers cannot start with a digit, so the variant is `N3ds`.
    // The CLI value is pinned to "3ds" so that `--system 3ds` matches the file extension.
    #[value(name = "3ds")]
    N3ds,
    /// PlayStation Portable
    Psp,
}

impl System {
    // Returns the name of the target system.
    pub fn name(&self) -> &'static str {
        match self {
            System::Gb => "gb",
            System::Gbc => "gbc",
            System::Gba => "gba",
            System::Sfc => "sfc",
            System::Nds => "nds",
            System::N3ds => "3ds",
            System::Psp => "psp",
        }
    }

    /// Returns the corresponding system based on the input file extension, if supported
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        match ext.as_deref() {
            Some("gb") => Some(System::Gb),
            Some("gbc") => Some(System::Gbc),
            Some("gba") => Some(System::Gba),
            Some("sfc") => Some(System::Sfc),
            Some("nds") => Some(System::Nds),
            Some("3ds") => Some(System::N3ds),
            // .iso is not PSP-specific, but PSP is the only disc-based system supported so far.
            // Revisit this mapping if another .iso-based system is added.
            Some("iso") => Some(System::Psp),
            _ => None,
        }
    }

    /// URL of the official DAT from https://github.com/libretro/libretro-database .
    /// `None` means the system has no official DAT and relies on a custom DAT.
    pub fn dat_url(&self) -> Option<&'static str> {
        match self {
            System::Gb => Some(
                "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy.dat",
            ),
            System::Gbc => Some(
                "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Color.dat",
            ),
            System::Gba => Some(
                "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Advance.dat",
            ),
            System::Sfc => Some(
                "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System.dat",
            ),
            // libretro-database builds its DS / 3DS DATs from the No-Intro "(Decrypted)" sets
            // (see dats.json in robloach/libretro-dats). Cartridges dumped with GodMode9 are
            // encrypted (its cart reads apply no decryption), and decrypted copies are trimmed
            // or not depending on the tool, so the Decrypted sets rarely match real files.
            // The No-Intro "(Encrypted)" sets cannot be redistributed, so users register one
            // themselves with `dat add`.
            System::Nds | System::N3ds => None,
            // Redump is used instead of No-Intro: the No-Intro PSP DAT has not been updated since 2021-10
            // (No-Intro defers disc-based systems to Redump), while the Redump DAT is maintained.
            // Redump lists the same ISO once per serial, so duplicate (name, crc) lines are expected.
            // Compressed images (.cso) cannot be matched because their CRC differs from the ISO.
            System::Psp => Some(
                "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/redump/Sony%20-%20PlayStation%20Portable.dat",
            ),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn from_path_to_system() {
        // act & assert
        assert_eq!(System::from_path(Path::new("a.gb")), Some(System::Gb));
        assert_eq!(System::from_path(Path::new("b.gbc")), Some(System::Gbc));
        assert_eq!(System::from_path(Path::new("c.gba")), Some(System::Gba));
        assert_eq!(System::from_path(Path::new("d.sfc")), Some(System::Sfc));
        assert_eq!(System::from_path(Path::new("e.nds")), Some(System::Nds));
        assert_eq!(System::from_path(Path::new("f.3ds")), Some(System::N3ds));
        assert_eq!(System::from_path(Path::new("g.iso")), Some(System::Psp));
    }

    #[test]
    fn from_path_to_upper_char() {
        // arrange
        let upper_ext_path = Path::new("test.GBA");

        // act & assert
        assert_eq!(System::from_path(upper_ext_path), Some(System::Gba))
    }

    #[test]
    fn from_path_unknown_ext_is_none() {
        // arrange
        let unknown_ext_path = Path::new("test.png");

        // act & assert
        assert_eq!(System::from_path(unknown_ext_path), None);
    }

    #[test]
    fn from_path_none_ext() {
        // arrange
        let none_ext_path = Path::new("test");

        // act & assert
        assert_eq!(System::from_path(none_ext_path), None);
    }

    #[test]
    fn dat_url_is_none_only_for_ds_and_3ds() {
        // act & assert
        for system in System::value_variants() {
            let expected_none = matches!(system, System::Nds | System::N3ds);
            assert_eq!(system.dat_url().is_none(), expected_none, "{:?}", system);
        }
    }

    #[test]
    fn cli_value_name_matches_system_name() {
        // arrange & act & assert
        // `dat add --system <name>` must accept the same string as the file extension / cache file name.
        for system in System::value_variants() {
            let value = system.to_possible_value().unwrap();
            assert_eq!(value.get_name(), system.name());
        }
    }
}
