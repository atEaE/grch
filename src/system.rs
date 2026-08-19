use std::path::Path;

pub enum System {
	Gba,
}

impl System {
	/// Returns the corresponding system based on the input file extension, if supported
	pub fn from_path(path: &Path) -> Option<Self> {
		let ext = path.extension().and_then(|e| e.to_str());
		match ext {
			Some("gba") => Some(System::Gba),
			_ => None,
		}
	}

	/// Obtain the DAT file corresponding to the system from https://github.com/libretro/libretro-database .
	pub fn dat_url(&self) -> String {
		match self {
			System::Gba => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Advance.dat".to_string(),
		}
	}
}