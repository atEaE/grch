use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Ok, bail};

use crate::dat;
use crate::dir;
use crate::system::System;

pub fn add(system: &System, input: &PathBuf) -> anyhow::Result<()> {
    let body = fs::read_to_string(input)
        .with_context(|| format!("failed to read DAT file: {}", input.display()))?;

    let map = dat::parse_dat(&body);
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

pub fn show(system: &System) -> anyhow::Result<()> {
    let Some(body) = dat::read_custom_body(system)? else {
        bail!("no custom DAT registered for {}", system.name());
    };

    print!("{}", body);
    Ok(())
}
