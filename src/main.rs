use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod archive;
mod commands;
mod credentials;
mod dat;
mod dir;
mod format;
mod hash;
mod hex;
mod index;
mod input;
mod library;
mod manifest;
mod password;
mod prompt;
mod remote;
mod sync;
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

        /// Use a local directory as the remote instead of Dropbox (for testing)
        #[arg(long, value_name = "DIR")]
        local: Option<PathBuf>,
    },

    /// Remote account and credentials
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },

    /// Download from the remote: files already here by default, more with --all / --system / PATTERN
    Pull {
        /// Fetch everything on the remote
        #[arg(long)]
        all: bool,

        /// Fetch every file of one system
        #[arg(long)]
        system: Option<system::System>,

        /// Fetch files matching these globs (name with or without extension)
        pattern: Vec<String>,

        /// Show what would be pulled without transferring
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompts (conflicts are resolved in favor of the remote; deletions still ask)
        #[arg(short, long)]
        yes: bool,
    },

    /// Upload local additions and changes to the remote
    Push {
        /// Limit to one system
        #[arg(long)]
        system: Option<system::System>,

        /// Limit to files matching these globs (name with or without extension)
        pattern: Vec<String>,

        /// Show what would be pushed without transferring
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompts (conflicts are resolved in favor of local files)
        #[arg(short, long)]
        yes: bool,
    },

    /// Show what push / pull would transfer (no transfer)
    Status {
        /// Limit to one system
        #[arg(long)]
        system: Option<system::System>,

        /// Limit to files matching these globs (name with or without extension)
        pattern: Vec<String>,
    },

    /// Debug: pack / unpack a single file the way sync does
    #[command(hide = true)]
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
}

#[derive(Subcommand)]
enum RemoteCommand {
    /// Authorize this machine with the remote and store the archive password
    Login,
    /// Remove the remote token and archive password from this machine
    Logout,
    /// Show the remote, account and manifest summary
    Info,
    /// List files on the remote (✓ = present on this machine)
    Ls {
        /// Limit to one system
        #[arg(long)]
        system: Option<system::System>,

        /// Limit to files matching these globs (name with or without extension)
        pattern: Vec<String>,
    },
    /// Set or change the stored archive password
    Password,
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
        Command::Init { dir, local } => {
            let dir = match dir {
                Some(dir) => dir,
                None => std::env::current_dir()?,
            };
            commands::init::run(&dir, local.as_deref())?
        }
        Command::Remote { command } => match command {
            RemoteCommand::Login => commands::remote::login()?,
            RemoteCommand::Logout => commands::remote::logout()?,
            RemoteCommand::Info => commands::remote::info()?,
            RemoteCommand::Ls { system, pattern } => commands::remote::ls(system, &pattern)?,
            RemoteCommand::Password => commands::remote::set_password()?,
        },
        Command::Pull {
            all,
            system,
            pattern,
            dry_run,
            yes,
        } => commands::pull::run(commands::pull::Options {
            all,
            system,
            patterns: pattern,
            dry_run,
            yes,
        })?,
        Command::Push {
            system,
            pattern,
            dry_run,
            yes,
        } => commands::push::run(commands::push::Options {
            system,
            patterns: pattern,
            dry_run,
            yes,
        })?,
        Command::Status { system, pattern } => commands::status::run(system, &pattern)?,
        Command::Archive { command } => match command {
            ArchiveCommand::Pack { input, output } => commands::archive::pack(&input, &output)?,
            ArchiveCommand::Unpack { input, output } => commands::archive::unpack(&input, &output)?,
        },
    }
    Ok(())
}
