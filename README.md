# grch

![License](https://img.shields.io/github/license/atEaE/grch)

## Data source

On first use, grch downloads the DAT file for the target system from [libretro-database](https://github.com/libretro/libretro-database) (No-Intro data for cartridge systems, Redump data for PSP) and caches it locally. Subsequent runs read from the cache, so no network access occurs. The cache can be managed with the `cache` subcommand.

Nintendo DS and Nintendo 3DS have no downloaded DAT. libretro-database carries the No-Intro "(Decrypted)" sets for them, while cartridges dumped with GodMode9 are encrypted, so those sets do not match typical dumps. Download the "(Encrypted)" set for the system from [DAT-o-MATIC](https://datomatic.no-intro.org/) yourself and register it:

```terminal
grch dat add --system 3ds -i "Nintendo - Nintendo 3DS (Encrypted) (yyyymmdd-hhmmss).dat"
```

Both the clrmamepro and Logiqx XML DAT formats are accepted.

## How to use

```terminal
game rom managment tool

Usage: grch <COMMAND>

Commands:
  check   Check the ROM file against the database
  rename  Rename to the official name registered in the ROM file database
  cache   Control the cache
  dat     Manage custom DAT files
  info    Show grch information
  init    Initialize a library root for cloud sync (creates .grch/)
  status  Show what push / pull would transfer (no transfer)
  push    Upload local additions and changes to the remote
  pull    Download from the remote (only files already here, unless --all / --system / PATTERN)
  remote  Remote account and credentials
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## Cloud sync

grch can keep a ROM library in sync across several machines through Dropbox. The remote is the source of truth; each machine holds whatever subset it wants.

Everything on the remote is unreadable without the archive password: each file is stored as its own 7z (LZMA2 + AES-256 with header encryption, so even the file list inside is hidden), objects are named by the SHA-256 of the archive rather than by title, and the manifest that maps titles to objects is encrypted the same way. Nothing on the remote reveals what the library contains.

### Library layout

A library is a directory with one folder per system. `init` matches existing folders case-insensitively and writes the mapping to `.grch/config.toml`, where it can be edited:

```text
<library>/
├── .grch/          # config.toml (remote, folder mapping), index.json (hash cache + sync state)
├── SFC/
├── GB/
├── GBC/
├── GBA/
├── DS/
├── 3DS/
└── PSP/
```

Only files directly under those folders are synced. Custom DATs registered with `dat add` are synced too, so every machine names unregistered dumps the same way.

### Dropbox app (once)

grch ships without a Dropbox app of its own, so register one and use its key for every machine that shares the library:

1. Open <https://www.dropbox.com/developers/apps> and choose "Create app".
2. Pick "Scoped access" and, for the access type, **"App folder"**. grch then only ever sees `Apps/<your app name>/` in your Dropbox.
3. On the "Permissions" tab enable `files.metadata.read`, `files.metadata.write`, `files.content.read` and `files.content.write`, then submit.
4. Copy the **App key** from the "Settings" tab. No app secret and no redirect URI are needed.

The app key is a public identifier, so it is kept in `.grch/config.toml` next to the rest of the remote settings (`remote.app_key`). A library on a different Dropbox app just has a different key in its own `.grch/`.

### Setup on each machine

```terminal
grch init ~/roms --app-key <KEY>    # or: grch init ~/roms --local /path/to/dir  (a directory as the remote, for testing)
cd ~/roms
grch remote login                   # Dropbox OAuth in the browser, then set the archive password
grch remote info
```

`grch remote login --app-key <KEY>` stores the key into an existing library, so an `init` without one can be completed later. The refresh token and the archive password are kept in the OS credential store (Keychain / Credential Manager / Secret Service), never in the library. Use the same archive password on every machine: if it is lost, nothing on the remote can be decrypted. `GRCH_ARCHIVE_PASSWORD` overrides the stored password for one run.

### Daily use

```terminal
grch status                 # what push / pull would do, no transfer
grch push                   # upload new and changed files
grch pull                   # refresh the files this machine already has
grch pull --all             # everything on the remote
grch pull --system sfc      # one system
grch pull "pocket*"         # titles matching a glob (name with or without extension)
grch remote ls              # what the remote holds (✓ = present here)
```

Deleting a file locally never deletes it from the remote. To remove something for every machine:

```terminal
grch remote rm "bad dump*"  # always asks; local copies stay
grch pull                   # on each machine, offers to delete the local copy
```

When the same file changed on two machines, `push` and `pull` ask which side wins (`-y` picks local for push and remote for pull). Deletions always ask, even with `-y`.

## License

- [MIT License](./LICENSE.md)

The MIT License applies to the source code of grch itself. The DAT files downloaded at runtime are **not** part of grch and are **not** covered by this license. They are provided by the [libretro-database](https://github.com/libretro/libretro-database) project and originate from [No-Intro](https://no-intro.org/) and [Redump](http://redump.org/), and remain subject to their respective terms.

## Acknowledgements

- [libretro-database](https://github.com/libretro/libretro-database) — ROM database that grch relies on
- [No-Intro](https://no-intro.org/) — the original source of the cartridge DAT files
- [Redump](http://redump.org/) — the original source of the disc DAT files
