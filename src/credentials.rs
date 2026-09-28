use anyhow::{Context, Result};

const SERVICE: &str = "grch";

/// Per-machine secrets kept in the OS credential store (Keychain / Credential Manager /
/// Secret Service), never in `.grch/` or a config file.
#[derive(Debug, Clone, Copy)]
pub enum Secret<'a> {
    /// Issued by Dropbox for one app, so it is stored per app key: two libraries on
    /// different Dropbox apps can both be logged in on the same machine.
    DropboxRefreshToken { app_key: &'a str },
    ArchivePassword,
}

impl Secret<'_> {
    fn username(self) -> String {
        match self {
            Secret::DropboxRefreshToken { app_key } => format!("dropbox-refresh-token:{app_key}"),
            Secret::ArchivePassword => "archive-password".to_string(),
        }
    }
}

fn entry(secret: Secret<'_>) -> Result<(keyring::Entry, String)> {
    let username = secret.username();
    let entry = keyring::Entry::new(SERVICE, &username).context("open credential store")?;
    Ok((entry, username))
}

pub fn get(secret: Secret<'_>) -> Result<Option<String>> {
    let (entry, username) = entry(secret)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {username} from credential store")),
    }
}

pub fn set(secret: Secret<'_>, value: &str) -> Result<()> {
    let (entry, username) = entry(secret)?;
    entry
        .set_password(value)
        .with_context(|| format!("store {username} in credential store"))
}

pub fn delete(secret: Secret<'_>) -> Result<()> {
    let (entry, username) = entry(secret)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).with_context(|| format!("delete {username} from credential store")),
    }
}
