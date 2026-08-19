use std::path::PathBuf;

use crate::input;

pub fn run(input: &[PathBuf]) {
	let files = input::collect_files(input);

	for path in &files {
		match std::fs::read(path) {
            Ok(data) => {
                let crc = crc32fast::hash(&data);
                println!("{:08X}  {}", crc, path.display());
            }
            Err(e) => {
                eprintln!("skip: {} ({})", path.display(), e);
            }
        }
	}
}