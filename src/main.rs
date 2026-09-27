use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod archive;
mod commands;
mod dat;
mod dir;
mod hash;
mod index;
mod input;
mod library;
mod password;
mod system;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the ROM file against the database
    Check {
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

    /// Initialize a library root for cloud sync (creates .grch/)
    Init {
        /// Library root (default: current directory)
        dir: Option<PathBuf>,
    },

    /// Debug: pack / unpack a single file the way sync does
    #[command(hide = true)]
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
}

#[derive(Subcommand)]
enum ArchiveCommand {
    /// Pack one file into an encrypted 7z
    Pack {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long)]
        output: PathBuf,
    },

    /// Extract an encrypted 7z into a directory
    Unpack {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long)]
        output: PathBuf,
    },
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
        Command::Check { input, refresh } => commands::check::run(&input, refresh)?,
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
        Command::Init { dir } => {
            let dir = match dir {
                Some(dir) => dir,
                None => std::env::current_dir()?,
            };
            commands::init::run(&dir)?
        }
        Command::Archive { command } => match command {
            ArchiveCommand::Pack { input, output } => commands::archive::pack(&input, &output)?,
            ArchiveCommand::Unpack { input, output } => commands::archive::unpack(&input, &output)?,
        },
    }
    Ok(())
}
