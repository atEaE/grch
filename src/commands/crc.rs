use std::path::PathBuf;

use anyhow::Ok;

use crate::{dat, input, system::System};

pub fn run(input: &[PathBuf]) -> anyhow::Result<()> {
	let files = input::collect_files(input);

	for path in &files {
        match System::from_path(path) {
            Some(system) => {       
                let body = dat::fetch_body(&system)?;
                println!("{}", body)
            }
            None => {

            }
        }
	}
    Ok(())
}