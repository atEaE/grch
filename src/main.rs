use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod dat;
mod dir;
mod input;
mod system;

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
        input: Vec<PathBuf>,

        /// Ignore the cache and fetch the latest DAT file (the cache is updated)
        #[arg(long)]
        refresh: bool,
    },

    /// Rename to the official name registered in the ROM file database
    Rename {
        /// Target rom file (ex. ./hoge/piyo.gba or ./piyo/hoge/*.gb)
        #[arg(short, long, num_args = 1.., required = true)]
        input: Vec<PathBuf>,

        /// Ignore the cache and fetch the latest DAT file (the cache is updated)
        #[arg(long)]
        refresh: bool,
    },

    /// Control the cache
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Remove all cache files
    Clean,
    /// List cache
    Ls,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Crc { input, refresh } => commands::crc::run(&input, refresh)?,
        Command::Rename { input, refresh } => commands::rename::run(&input, refresh)?,
        Command::Cache { command } => match command {
            CacheCommand::Clean => commands::cache::clean()?,
            CacheCommand::Ls => commands::cache::ls()?,
        },
    }
    Ok(())
}
