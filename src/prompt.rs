use std::io::{self, BufRead, Write};

use anyhow::Result;

/// `[y/N]` question on stdout. Anything but "y" / "yes" is a no.
pub fn confirm(question: &str) -> Result<bool> {
    print!("{question} [y/N]: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}
