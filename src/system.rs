use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum System {
    /// Game Boy
    Gb,
    /// Game Boy Color
    Gbc,
    /// Game Boy Advance
    Gba,
    /// Super Family Computer
    Sfc,
}

impl System {
    // Returns the name of the target system.
    pub fn name(&self) -> &'static str {
        match self {
            System::Gb => "gb",
            System::Gbc => "gbc",
            System::Gba => "gba",
            System::Sfc => "sfc",
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
            _ => None,
        }
    }

    /// Obtain the DAT file corresponding to the system from https://github.com/libretro/libretro-database .
    pub fn dat_url(&self) -> String {
        match self {
            System::Gb => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy.dat".to_string(),
            System::Gbc => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Color.dat".to_string(),
			System::Gba => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Advance.dat".to_string(),
            System::Sfc => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System.dat".to_string(),
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
}
