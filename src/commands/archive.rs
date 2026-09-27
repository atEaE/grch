use std::path::Path;

use anyhow::bail;
use colored::Colorize;

use crate::archive;
use crate::password;

/// Debug helper: pack one file the way `push` will, so the result can be inspected
/// with 7-Zip (the entry list must not be readable without the password).
pub fn pack(input: &Path, output: &Path) -> anyhow::Result<()> {
    let Some(name) = input.file_name().and_then(|n| n.to_str()) else {
        bail!("input has no file name: {}", input.display());
    };
    let password = password::resolve()?;
    let packed = archive::pack_file(input, name, output, &password)?;
    println!("{} {}", "✓".green(), output.display());
    println!("   └ sha256: {}", packed.sha256);
    println!("   └ size:   {}", packed.size);
    Ok(())
}

/// Debug helper: extract into `output` without checking against an index entry.
pub fn unpack(input: &Path, output: &Path) -> anyhow::Result<()> {
    let password = password::resolve()?;
    let written = archive::extract_file(input, &password, None, output, output)?;
    println!("{} {}", "✓".green(), written.display());
    Ok(())
}
