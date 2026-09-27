use anyhow::{Context, Result};

const SERVICE: &str = "grch";

/// Per-machine secrets kept in the OS credential store (Keychain / Credential Manager /
/// Secret Service), never in `.grch/` or a config file.
#[derive(Debug, Clone, Copy)]
pub enum Secret {
    DropboxRefreshToken,
    ArchivePassword,
}

impl Secret {
    fn username(self) -> &'static str {
        match self {
            Secret::DropboxRefreshToken => "dropbox-refresh-token",
            Secret::ArchivePassword => "archive-password",
        }
    }
}

fn entry(secret: Secret) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, secret.username()).context("open credential store")
}

pub fn get(secret: Secret) -> Result<Option<String>> {
    match entry(secret)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            Err(e).with_context(|| format!("read {} from credential store", secret.username()))
        }
    }
}

pub fn set(secret: Secret, value: &str) -> Result<()> {
    entry(secret)?
        .set_password(value)
        .with_context(|| format!("store {} in credential store", secret.username()))
}

pub fn delete(secret: Secret) -> Result<()> {
    match entry(secret)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => {
            Err(e).with_context(|| format!("delete {} from credential store", secret.username()))
        }
    }
}
