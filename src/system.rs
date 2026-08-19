use std::path::Path;

pub enum System {
	Gba,
}

impl System {
	/// 入力ファイルの拡張子から、対応可能な場合は対応システムを返す
	pub fn from_path(path: &Path) -> Option<Self> {
		let ext = path.extension().and_then(|e| e.to_str());
		match ext {
			Some("gba") => Some(System::Gba),
			_ => None,
		}
	}

	/// システムに対応するDATファイルを https://github.com/libretro/libretro-database から取得する。
	pub fn dat_url(&self) -> String {
		match self {
			System::Gba => "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Advance.dat".to_string(),
		}
	}
}