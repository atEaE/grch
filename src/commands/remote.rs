use colored::Colorize;

use crate::credentials::{self, Secret};
use crate::library::Library;
use crate::manifest::Manifest;
use crate::password;
use crate::remote::{self, dropbox};

fn current_library() -> anyhow::Result<Library> {
    Library::discover(&std::env::current_dir()?)
}

pub fn login() -> anyhow::Result<()> {
    let library = current_library()?;
    match library.config.remote.backend.as_str() {
        "dropbox" => {
            dropbox::login()?;
            let remote = remote::open(&library.config.remote)?;
            println!("{} logged in: {}", "✓".green(), remote.describe()?);
        }
        other => println!("the {other} backend needs no login"),
    }

    if credentials::get(Secret::ArchivePassword)?.is_some() {
        println!("archive password already stored (use `grch remote password` to change it)");
    } else {
        password::prompt_and_store()?;
        println!("{} archive password stored", "✓".green());
    }
    Ok(())
}

pub fn logout() -> anyhow::Result<()> {
    dropbox::logout()?;
    credentials::delete(Secret::ArchivePassword)?;
    println!(
        "{} removed the Dropbox token and archive password from this machine",
        "✓".green()
    );
    Ok(())
}

pub fn set_password() -> anyhow::Result<()> {
    password::prompt_and_store()?;
    println!("{} archive password stored", "✓".green());
    Ok(())
}

pub fn info() -> anyhow::Result<()> {
    let library = current_library()?;
    println!("library: {}", library.root.display());
    println!("backend: {}", library.config.remote.backend);

    let remote = match remote::open(&library.config.remote) {
        Ok(remote) => remote,
        Err(e) => {
            println!("remote:  {} {}", "✗".red(), e);
            return Ok(());
        }
    };
    println!("remote:  {}", remote.describe()?);

    let password_source = if std::env::var_os(password::ENV_VAR).is_some() {
        "environment"
    } else if credentials::get(Secret::ArchivePassword)?.is_some() {
        "credential store"
    } else {
        "prompt"
    };
    println!("archive password: {password_source}");

    match remote.get_manifest()? {
        None => println!("manifest: none (nothing pushed yet)"),
        Some((bytes, _)) => {
            let manifest = Manifest::from_encrypted(&bytes, &password::resolve()?)?;
            println!(
                "manifest: {} roms, updated {} by {}",
                manifest.roms.len(),
                manifest.updated_at,
                manifest.updated_by
            );
        }
    }
    Ok(())
}
