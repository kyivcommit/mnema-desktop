//! The stored shortcut, as the person on this platform reads it.
//!
//! 🔴 **This is the second implementation of one function, and that is the
//! defect it is built against rather than an accident.** `ui/src/i18n/
//! shortcut.ts` has drawn the shortcut for the settings window since Task 7;
//! the tray menu is native and cannot call it, and until now it did not try —
//! it formatted the literal `(⌥Space)`, so a person who changed the shortcut
//! got a tray that went on naming the old one. Two formatters that agree by
//! nobody's construction would drift the same way in slower motion.
//!
//! What holds them together is `ui/src/i18n/shortcut.fixtures.json`: one file,
//! two readers. The TypeScript suite iterates it and this module's tests
//! `include_str!` it, so an expected value edited in that file must go red on
//! both sides or one of the two has stopped reading it. That is a weaker bond
//! than a shared implementation and a much stronger one than a comment asking
//! the next person to keep two files in step.
//!
//! Everything below is a token-for-token mirror of that module: the same
//! canonical [`ORDER`], the same four glyphs, the same per-platform word for
//! the command key, the same twelve aliases, and the same rule for a token
//! neither side understands. The strings are the DISPLAY vocabulary the two
//! formatters agree on — not the parser's, which is a different set of twelve
//! (`prefs.rs`'s `MODIFIER_SPELLINGS`; see [`alias`] for where the two differ
//! and what that costs) — plus the glyphs Apple prints. Protocol tokens rather
//! than prose either way, which is why nothing here belongs in the catalogue.

use crate::models::Platform;

/// The canonical order, and it is canonical rather than incidental. The parser
/// is indifferent to the order of the modifiers AMONG THEMSELVES but not to the
/// key, which must come last; what one fixed order buys is that two people who
/// press the same keys store the same string and read the same label — and that
/// this side and the window's side read one stored string the same way.
const ORDER: [Modifier; 4] = [
    Modifier::Ctrl,
    Modifier::Alt,
    Modifier::Shift,
    Modifier::Super,
];

/// The four modifiers this formatter knows by name. Everything else in a stored
/// string is an unknown token and is passed through — see [`format_shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl Modifier {
    /// ⌃⌥⇧⌘ — the order Apple prints them in, which is [`ORDER`].
    fn mac_glyph(self) -> &'static str {
        match self {
            Modifier::Ctrl => "⌃",
            Modifier::Alt => "⌥",
            Modifier::Shift => "⇧",
            Modifier::Super => "⌘",
        }
    }

    /// How this modifier is written where it is not a glyph. Linux keeps the
    /// parser's own spelling throughout; Windows says `Win` for the command
    /// key, which is what is printed on the key a person is looking at.
    ///
    /// `Platform::Mac` never reaches here — that branch returns glyphs — and
    /// answers with the glyph rather than with a panic, because a formatter
    /// that aborts the tray build over an unreachable arm would trade a wrong
    /// label for no menu at all.
    fn word(self, platform: Platform) -> &'static str {
        match (self, platform) {
            (Modifier::Ctrl, _) => "Ctrl",
            (Modifier::Alt, _) => "Alt",
            (Modifier::Shift, _) => "Shift",
            (Modifier::Super, Platform::Windows) => "Win",
            (Modifier::Super, Platform::Linux) => "Super",
            (Modifier::Super, Platform::Mac) => Modifier::Super.mac_glyph(),
        }
    }
}

/// The spellings the two FORMATTERS fold onto the one this module emits, so
/// that two spellings of one shortcut do not read as two different shortcuts. A
/// stored string need not be one this application built — `prefs.json` is a file
/// on the person's own disk, and the parser accepts any order of modifiers as
/// long as the key is last.
///
/// ⚠️ **This is a display vocabulary, and it is NOT the parser's.** The parser's
/// own twelve are `prefs::MODIFIER_SPELLINGS`, transcribed from the modifier
/// arms of `global-hotkey-0.8.0/src/hotkey.rs`. The two sets overlap and neither
/// contains the other: this table adds `ctl`, `altgr`, `meta` and `win`, which
/// the parser refuses, and omits `CommandOrControl`, `CommandOrCtrl`,
/// `CmdOrCtrl` and `CmdOrCommand`, which it accepts and resolves per platform.
/// So a `prefs.json` holding `CmdOrCtrl+Space` is accepted by `accept_shortcut`,
/// registered, and then drawn as the literal `CmdOrCtrl+Space` rather than as
/// `⌘Space` — the passthrough rule below doing exactly what it says, not a
/// falsehood. Widening this table is a decision about what a person reads, and
/// it would have to be made on both sides and given a fixture row.
///
/// ⚠️ A `match` and not a map lookup, which is where the mirror on the
/// TypeScript side needed a guard this one never did: its table is an object
/// literal and inherits `Object.prototype`, so a bare index answered truthy for
/// `constructor`, `__proto__` and four more, and swallowed them as modifiers
/// mapping to nothing (review round 1, Minor 1). The fixture carries a row for
/// that class now, so the two answers are pinned together rather than argued
/// about here.
fn alias(token: &str) -> Option<Modifier> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "ctl" => Some(Modifier::Ctrl),
        "alt" | "option" | "altgr" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        "super" | "meta" | "cmd" | "command" | "win" => Some(Modifier::Super),
        _ => None,
    }
}

/// The stored shortcut, as the person on `platform` reads it.
///
/// Unknown tokens are passed through as they are rather than dropped: a string
/// this module does not fully understand is still the string the operating
/// system is holding, and showing less of it than there is would be the one
/// mistake worse than showing it awkwardly.
pub fn format_shortcut(shortcut: &str, platform: Platform) -> String {
    let tokens: Vec<&str> = shortcut.split('+').collect();
    let (key, modifier_tokens) = tokens
        .split_last()
        .map_or(("", &[][..]), |(k, rest)| (*k, rest));

    let mut held: Vec<Modifier> = Vec::new();
    let mut unknown: Vec<&str> = Vec::new();
    for token in modifier_tokens {
        match alias(token) {
            // A set, not a list: `Ctrl+Control+A` names one held key twice and
            // must not draw it twice.
            Some(m) => {
                if !held.contains(&m) {
                    held.push(m);
                }
            }
            None => unknown.push(token),
        }
    }
    // Drawn from ORDER rather than from the order the tokens arrived in. This
    // filter is the whole of the normalisation, and the fixture's
    // `Shift+Ctrl+A` row is the only one that can see it go.
    let modifiers: Vec<Modifier> = ORDER.into_iter().filter(|m| held.contains(m)).collect();

    let mut tail: Vec<&str> = unknown;
    tail.push(key);

    if platform == Platform::Mac {
        // No separator at all: `⌥Space` is how the combination is written on
        // this platform, and a `+` between glyphs reads as a key of its own.
        let glyphs: String = modifiers.iter().map(|m| m.mac_glyph()).collect();
        return glyphs + &tail.join("+");
    }
    let mut parts: Vec<&str> = modifiers.iter().map(|m| m.word(platform)).collect();
    parts.extend(tail);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture both formatters are judged against, read from the file the
    /// TypeScript suite reads. `include_str!` rather than a runtime path: the
    /// file has to be there at COMPILE time, so a fixture moved or renamed
    /// fails to build here instead of being quietly skipped at run time.
    const FIXTURES: &str = include_str!("../../ui/src/i18n/shortcut.fixtures.json");

    #[derive(serde::Deserialize)]
    struct Case {
        shortcut: String,
        platform: String,
        expected: String,
    }

    /// The three platform names as they cross the wire (`Platform`'s own
    /// `camelCase` serialisation), read back. Written here rather than derived
    /// with `Deserialize` on `Platform` itself, which is a send-only type: a
    /// name the fixture invents must fail this test loudly rather than fall
    /// into a default arm and be formatted as Linux.
    fn platform_named(name: &str) -> Platform {
        match name {
            "mac" => Platform::Mac,
            "windows" => Platform::Windows,
            "linux" => Platform::Linux,
            other => panic!("the fixture names a platform this build has no arm for: {other}"),
        }
    }

    #[test]
    fn the_shared_fixture_is_what_this_formatter_produces() {
        let cases: Vec<Case> = serde_json::from_str(FIXTURES).expect("the fixture file parses");
        // A fixture that had lost its rows would make every assertion below
        // vacuous, and an empty array is valid JSON. The number is a floor and
        // not the count, so adding a row does not edit this line.
        assert!(
            cases.len() >= 10,
            "the shared fixture has shrunk to {} rows",
            cases.len()
        );
        for case in &cases {
            assert_eq!(
                format_shortcut(&case.shortcut, platform_named(&case.platform)),
                case.expected,
                "`{}` on {}",
                case.shortcut,
                case.platform
            );
        }
    }

    /// One assertion per platform, written out here rather than left to the
    /// fixture alone: the loop above is only as good as the file it reads, and
    /// these three are what say the file is describing this function.
    #[test]
    fn each_platform_draws_the_same_stored_string_its_own_way() {
        assert_eq!(format_shortcut("Ctrl+Alt+Space", Platform::Mac), "⌃⌥Space");
        assert_eq!(
            format_shortcut("Ctrl+Alt+Space", Platform::Windows),
            "Ctrl+Alt+Space"
        );
        assert_eq!(
            format_shortcut("Ctrl+Alt+Space", Platform::Linux),
            "Ctrl+Alt+Space"
        );
        // The command key is the one the three disagree about, positively:
        // a formatter that ignored its second argument would satisfy any one
        // of the three assertions above.
        assert_eq!(format_shortcut("Super+K", Platform::Mac), "⌘K");
        assert_eq!(format_shortcut("Super+K", Platform::Windows), "Win+K");
        assert_eq!(format_shortcut("Super+K", Platform::Linux), "Super+K");
        assert_ne!(
            format_shortcut("Super+K", Platform::Windows),
            format_shortcut("Super+K", Platform::Linux)
        );
    }

    #[test]
    fn the_modifiers_are_drawn_in_the_canonical_order_whichever_order_the_store_holds() {
        // Both directions: the canonical form is produced, and the input's own
        // order is not — a passthrough satisfies the first assertion on any
        // string that was already canonical.
        assert_eq!(format_shortcut("Shift+Ctrl+A", Platform::Mac), "⌃⇧A");
        assert_ne!(format_shortcut("Shift+Ctrl+A", Platform::Mac), "⇧⌃A");
        assert_eq!(
            format_shortcut("Super+Shift+Alt+Ctrl+A", Platform::Linux),
            "Ctrl+Alt+Shift+Super+A"
        );
    }

    #[test]
    fn a_token_no_alias_carries_is_kept_rather_than_dropped() {
        assert_eq!(format_shortcut("Hyper+Alt+X", Platform::Mac), "⌥Hyper+X");
        assert_eq!(
            format_shortcut("Hyper+Alt+X", Platform::Linux),
            "Alt+Hyper+X"
        );
    }

    #[test]
    fn one_held_key_spelled_twice_is_drawn_once() {
        // `prefs.json` is a file on the person's own disk and the parser takes
        // this; two spellings of one key must not become two glyphs.
        assert_eq!(format_shortcut("Ctrl+Control+A", Platform::Mac), "⌃A");
    }

    #[test]
    fn a_string_with_no_modifier_at_all_is_drawn_as_it_is() {
        assert_eq!(format_shortcut("Space", Platform::Mac), "Space");
        assert_eq!(format_shortcut("", Platform::Mac), "");
    }
}
