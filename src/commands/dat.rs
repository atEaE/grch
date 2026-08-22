use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Ok, bail};

use crate::{dat::parse_dat, dir, system};

pub fn add(system: &system::System, input: &PathBuf) -> anyhow::Result<()> {
    let body = fs::read_to_string(input)
        .with_context(|| format!("failed to read DAT file: {}", input.display()))?;

    let map = parse_dat(&body);
    if map.is_empty() {
        bail!("no valid entries found in {}", input.display())
    }

    let data_dir = dir::custom_dat_dir()?;
    fs::create_dir_all(&data_dir)?;

    let path = data_dir.join(format!("{}.dat", system.name()));
    fs::write(path, body)?;

    println!("registered {} entries for {}", map.len(), system.name());
    Ok(())
}
