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

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Control the cache
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

    /// Manage custom DAT files
    Dat {
        #[command(subcommand)]
        command: DatCommand,
    },

    /// Show grch information
    Info,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Remove all cache files
    Clean,
    /// List cache
    Ls,
}

#[derive(Subcommand)]
enum DatCommand {
    /// Add a custom DAT file
    Add {
        #[arg(long)]
        system: system::System,

        #[arg(short, long)]
        input: PathBuf,
    },

    /// Show a custom DAT file
    Show {
        #[arg(long)]
        system: system::System,
    },

    /// List custom DAT files
    Ls,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Crc { input, refresh } => commands::crc::run(&input, refresh)?,
        Command::Rename {
            input,
            refresh,
            yes,
        } => commands::rename::run(&input, refresh, yes)?,
        Command::Cache { command } => match command {
            CacheCommand::Clean => commands::cache::clean()?,
            CacheCommand::Ls => commands::cache::ls()?,
        },
        Command::Dat { command } => match command {
            DatCommand::Add { system, input } => commands::dat::add(&system, &input)?,
            DatCommand::Show { system } => commands::dat::show(&system)?,
            DatCommand::Ls => commands::dat::ls()?,
        },
        Command::Info => commands::info::run()?,
    }
    Ok(())
}
