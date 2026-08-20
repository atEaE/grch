use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum System {
    /// Game Boy Advance
    Gba,
    /// Super Family Computer
    Sfc,
}

impl System {
    // Returns the name of the target system.
    pub fn name(&self) -> &'static str {
        match self {
            System::Gba => "gba",
            System::Sfc => "sfc",
        }
    }

    /// Returns the corresponding system based on the input file extension, if supported
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|e| e.to_str());
        match ext {
            Some("gba") => Some(System::Gba),
            Some("sfc") => Some(System::Sfc),
            _ => None,
        }
    }

    /// Obtain the DAT file corresponding to the system from https://github.com/libretro/libretro-database .
    pub fn dat_url(&self) -> String {
        match self {
			System::Gba => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Advance.dat".to_string(),
            System::Sfc => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System.dat".to_string(),
		}
    }
}
