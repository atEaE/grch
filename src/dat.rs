use anyhow::{Context, Result};

use crate::system::System;

/// Obtain the DAT file of the ROM corresponding to the target system
pub fn fetch_body(system: &System) -> Result<String> {
	let url = system.dat_url();
	let mut res = ureq::get(&url).call()?;
	let body = res.body_mut().read_to_string()?;
	Ok(body)
}