use anyhow::{Context, Result, bail};

use crate::credentials::{self, Secret};

pub const ENV_VAR: &str = "GRCH_ARCHIVE_PASSWORD";

/// Archive password: environment variable (one-off override) → credential store
/// (set by `remote login` / `remote password`) → interactive prompt.
pub fn resolve() -> Result<String> {
    if let Ok(password) = std::env::var(ENV_VAR) {
        if password.is_empty() {
            bail!("{} is set but empty", ENV_VAR);
        }
        return Ok(password);
    }
    if let Some(password) = credentials::get(Secret::ArchivePassword)? {
        return Ok(password);
    }
    let password = rpassword::prompt_password("Archive password: ").context("read password")?;
    if password.is_empty() {
        bail!("password must not be empty");
    }
    Ok(password)
}

/// Ask twice and store in the credential store. Every machine must use the same password,
/// and a forgotten one makes the remote unreadable, so the warning is printed here.
pub fn prompt_and_store() -> Result<()> {
    println!("The archive password encrypts everything on the remote.");
    println!(
        "Use the same password on every machine. If it is lost, the remote cannot be decrypted."
    );
    let first = rpassword::prompt_password("Archive password: ").context("read password")?;
    if first.is_empty() {
        bail!("password must not be empty");
    }
    let second = rpassword::prompt_password("Confirm password: ").context("read password")?;
    if first != second {
        bail!("passwords do not match");
    }
    credentials::set(Secret::ArchivePassword, &first)
}
