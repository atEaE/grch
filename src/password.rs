use anyhow::{Context, Result, bail};

pub const ENV_VAR: &str = "GRCH_ARCHIVE_PASSWORD";

/// Archive password: the environment variable first, then an interactive prompt.
/// A keychain lookup is planned to sit between the two once `remote login` exists.
pub fn resolve() -> Result<String> {
    if let Ok(password) = std::env::var(ENV_VAR) {
        if password.is_empty() {
            bail!("{} is set but empty", ENV_VAR);
        }
        return Ok(password);
    }
    let password = rpassword::prompt_password("Archive password: ").context("read password")?;
    if password.is_empty() {
        bail!("password must not be empty");
    }
    Ok(password)
}
