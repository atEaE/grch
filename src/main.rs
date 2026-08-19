use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod input;
mod system;
mod dat;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the CRC32 of the ROM file
    Crc {
        /// Target rom file (ex. ./hoge/piyo.gba or ./piyo/hoge/*.gb)
        #[arg(short, long, num_args = 1.., required = true)]
        input: Vec<PathBuf>
    },

    /// Rename to the official name registered in the ROM file database
    Rename {
        /// Target rom file (ex. ./hoge/piyo.gba or ./piyo/hoge/*.gb)
        #[arg(short, long, num_args = 1.., required = true)]
        input: Vec<PathBuf>
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Crc { input } => commands::crc::run(&input)?,
        Command::Rename { input } => commands::rename::run(&input),
    }
    Ok(())
}
