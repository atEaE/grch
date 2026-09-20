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
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## License

- [MIT License](./LICENSE.md)

The MIT License applies to the source code of grch itself. The DAT files downloaded at runtime are **not** part of grch and are **not** covered by this license. They are provided by the [libretro-database](https://github.com/libretro/libretro-database) project and originate from [No-Intro](https://no-intro.org/) and [Redump](http://redump.org/), and remain subject to their respective terms.

## Acknowledgements

- [libretro-database](https://github.com/libretro/libretro-database) — ROM database that grch relies on
- [No-Intro](https://no-intro.org/) — the original source of the cartridge DAT files
- [Redump](http://redump.org/) — the original source of the disc DAT files
