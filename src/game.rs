use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;

use crate::system::System;

/// A single file that makes up a game (bin/cue/cartridge rom, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub crc32: u32,
}

/// A DAT entry (one disc or one cartridge) and the local files belonging to it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Official entry name in the DAT (ex. "Xxx (Japan) (Disc 1)")
    pub dat_name: String,
    pub files: Vec<FileInfo>,
}

/// Unit of transfer and archiving.
/// Multi-disc titles bundle several DAT entries (Disc 1, Disc 2, ...) into one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub system: System,
    /// Grouping key: the DAT entry name with disc tokens removed
    pub name: String,
    pub entries: Vec<Entry>,
}

/// Generate the grouping key from a DAT entry name.
/// Removes every ` (Disc <X>)` token wherever it appears, so that
/// "Xxx (Disc 1) (Rev 1)" and "Xxx (Disc 2) (Rev 1)" share the key "Xxx (Rev 1)".
pub fn grouping_key(dat_name: &str) -> String {
    let rgx = Regex::new(r" \(Disc [^)]+\)").unwrap();
    rgx.replace_all(dat_name, "").into_owned()
}

/// Bundle DAT entries into games by (system, grouping key).
/// Entries within a game and games themselves are sorted by name for deterministic output.
pub fn group_entries(system: System, entries: Vec<Entry>) -> Vec<Game> {
    let mut grouped: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    for entry in entries {
        let key = grouping_key(&entry.dat_name);
        grouped.entry(key).or_default().push(entry);
    }

    let mut games = Vec::new();
    for (name, mut entries) in grouped {
        entries.sort_by(|a, b| a.dat_name.cmp(&b.dat_name));
        games.push(Game {
            system,
            name,
            entries,
        });
    }
    games
}

/// Find the DAT entry a local file belongs to.
/// The file stem must start with the entry name, followed by nothing or another
/// ` (...)` tag (ex. "Xxx (Disc 1) (Track 5).bin" and "Xxx (Disc 1).cue" both
/// belong to "Xxx (Disc 1)"). When several entry names qualify, the longest wins.
pub fn entry_for_file<'a>(file_name: &str, entry_names: &'a [String]) -> Option<&'a str> {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy())?;

    entry_names
        .iter()
        .filter(|name| {
            stem.as_ref() == name.as_str()
                || stem
                    .strip_prefix(name.as_str())
                    .is_some_and(|rest| rest.starts_with(" ("))
        })
        .max_by_key(|name| name.len())
        .map(|name| name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dat_name: &str) -> Entry {
        Entry {
            dat_name: dat_name.to_string(),
            files: Vec::new(),
        }
    }

    #[test]
    fn grouping_key_removes_disc_token() {
        // act & assert
        // pattern 1, 2, 3
        assert_eq!(
            grouping_key("Final Fantasy VII (Japan) (Disc 1)"),
            "Final Fantasy VII (Japan)"
        );

        // pattern A, B, C
        assert_eq!(
            grouping_key("Sister Princess 2 - Premium Fan Disc (Japan) (Disc A)"),
            "Sister Princess 2 - Premium Fan Disc (Japan)"
        );
    }

    #[test]
    fn grouping_key_removes_disc_token_in_the_middle() {
        // arrange
        let name = "Ace Combat 3 - Electrosphere (Japan, Asia) (Disc 2) (Rev 1)";

        // act & assert
        assert_eq!(
            grouping_key(name),
            "Ace Combat 3 - Electrosphere (Japan, Asia) (Rev 1)"
        );
    }

    #[test]
    fn grouping_key_keeps_name_without_disc_token() {
        // act & assert
        assert_eq!(
            grouping_key("Super Mario Land (World)"),
            "Super Mario Land (World)"
        );
    }

    #[test]
    fn group_entries_bundles_multi_disc_game() {
        // arrange
        let entries = vec![
            entry("Final Fantasy VII (Japan) (Disc 2)"),
            entry("Final Fantasy VII (Japan) (Disc 1)"),
            entry("Final Fantasy VII (Japan) (Disc 3)"),
            entry("Super Mario Land (World)"),
        ];

        // act
        let games = group_entries(System::Sfc, entries);

        // assert
        assert_eq!(games.len(), 2);

        {
            // expected FF7
            let game = &games[0];
            assert_eq!(game.name, "Final Fantasy VII (Japan)");
            assert_eq!(game.entries.len(), 3);
            // sorted by dat_name
            assert_eq!(
                game.entries[0].dat_name,
                "Final Fantasy VII (Japan) (Disc 1)"
            );
            assert_eq!(
                game.entries[2].dat_name,
                "Final Fantasy VII (Japan) (Disc 3)"
            );
        }

        {
            // expected mario
            let game = &games[1];
            assert_eq!(game.name, "Super Mario Land (World)");
            assert_eq!(game.entries.len(), 1);
        }
    }

    #[test]
    fn group_entries_bundles_disc_with_trailing_tag() {
        // arrange
        let entries = vec![
            entry("Ace Combat 3 - Electrosphere (Japan, Asia) (Disc 1) (Rev 1)"),
            entry("Ace Combat 3 - Electrosphere (Japan, Asia) (Disc 2) (Rev 1)"),
        ];

        // act
        let games = group_entries(System::Sfc, entries);

        // assert
        assert_eq!(games.len(), 1);
        assert_eq!(
            games[0].name,
            "Ace Combat 3 - Electrosphere (Japan, Asia) (Rev 1)"
        );
        assert_eq!(games[0].entries.len(), 2);
    }

    #[test]
    fn group_entries_known_collision_case() {
        // arrange
        // Real-world case found by surveying all 13,593 entries of the libretro-database
        // PS1 DAT (Sony - PlayStation.dat): "Dungeon Creator (Japan)" is the only title
        // where stripping the disc token collides with a different release — the
        // standalone SLPS-00349 vs the 2-disc SLPS-00370.
        // We accept the resulting mis-grouping (Disc 1 merges with the standalone,
        // Disc 2 splits off with its own tag) because the file names never collide;
        // only the archive bundling is affected.
        let entries = vec![
            entry("Dungeon Creator (Japan)"),
            entry("Dungeon Creator (Japan) (Disc 1)"),
            entry("Dungeon Creator (Japan) (Disc 2) (Memory Bank Disc)"),
        ];

        // act
        let games = group_entries(System::Sfc, entries);

        // assert
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Dungeon Creator (Japan)");
        assert_eq!(games[0].entries.len(), 2);
        assert_eq!(games[1].name, "Dungeon Creator (Japan) (Memory Bank Disc)");
        assert_eq!(games[1].entries.len(), 1);
    }

    #[test]
    fn entry_for_file_matches_track_and_cue() {
        // arrange
        // libretro-database is a game-identification DB for RetroArch's scanner,
        // not a file inventory: its PS1 DAT keeps one rom line per game (the data
        // track used for identification) and never lists cue sheets or audio tracks.
        // A local bin/cue rip therefore has files the DAT never lists; they can only
        // be attributed to their entry by file name. These are the two such shapes:
        // a track past the first, and the cue (entry name with a different extension).
        let names = vec![
            "Final Fantasy VII (Japan) (Disc 1)".to_string(),
            "Final Fantasy VII (Japan) (Disc 2)".to_string(),
        ];

        // act & assert
        assert_eq!(
            entry_for_file("Final Fantasy VII (Japan) (Disc 1) (Track 5).bin", &names),
            Some("Final Fantasy VII (Japan) (Disc 1)")
        );
        assert_eq!(
            entry_for_file("Final Fantasy VII (Japan) (Disc 2).cue", &names),
            Some("Final Fantasy VII (Japan) (Disc 2)")
        );
    }

    #[test]
    fn entry_for_file_prefers_longest_match() {
        // arrange
        let names = vec![
            "Dungeon Creator (Japan)".to_string(),
            "Dungeon Creator (Japan) (Disc 1)".to_string(),
        ];

        // act & assert
        assert_eq!(
            entry_for_file("Dungeon Creator (Japan) (Disc 1) (Track 01).bin", &names),
            Some("Dungeon Creator (Japan) (Disc 1)")
        );
        assert_eq!(
            entry_for_file("Dungeon Creator (Japan) (Track 01).bin", &names),
            Some("Dungeon Creator (Japan)")
        );
    }

    #[test]
    fn entry_for_file_requires_tag_boundary() {
        // arrange
        let names = vec!["Dungeon Creator (Japan)".to_string()];

        // act & assert: a longer, different title must not match by raw prefix
        assert_eq!(
            entry_for_file("Dungeon Creator (Japan) 2.bin", &names),
            None
        );
        assert_eq!(entry_for_file("Unknown Game (Japan).bin", &names), None);
    }
}
