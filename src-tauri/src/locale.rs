//! Interface localization (§D129). English is the fallback for any locale the
//! app does not support (§D80, amended). This module owns the effective locale;
//! `mnema-core::Coordinate::render` is prompt-only and deliberately untouched.

use crate::paths;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Uk,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleChoice {
    Auto,
    Uk,
    En,
}

/// The OS may report `uk-UA`, `uk_UA.UTF-8` (POSIX), `UK-ua`, `C`, `POSIX`, or
/// nothing. Lowercase, cut at the first `-`/`_`/`.`, and reject the non-language
/// locales `c`/`posix`/empty.
pub fn primary_subtag(os: Option<&str>) -> Option<String> {
    let raw = os?.trim().to_lowercase();
    let head = raw.split(['-', '_', '.']).next().unwrap_or("");
    match head {
        "" | "c" | "posix" => None,
        tag => Some(tag.to_string()),
    }
}

pub fn resolve(choice: LocaleChoice, os: Option<&str>) -> Lang {
    match choice {
        LocaleChoice::Uk => Lang::Uk,
        LocaleChoice::En => Lang::En,
        LocaleChoice::Auto => match primary_subtag(os).as_deref() {
            Some("uk") => Lang::Uk, // the only subtag that does not fall to EN
            _ => Lang::En,          // en, de, zh, None → En (D80: English is the fallback)
        },
    }
}

/// The canonical set of translatable strings the Rust side owns. Every key must
/// resolve in both languages — the `match` in `t` is exhaustive over `(Lang,
/// Key)`, so a new variant without both arms fails to compile; the completeness
/// test below is the belt to that compiler-enforced brace. Translatable TEXT
/// only: no emoji, no shortcut hints (e.g. `(⌥Space)`), no endonyms — those are
/// composed at the call site or, for endonyms, live in `endonym` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    TrayStatus,        // "Проіндексовано —" / "Indexed —"
    TrayShowSearch,    // "Показати пошук" / "Show search"
    TrayOpenSettings,  // "Відкрити налаштування" / "Open settings"
    TrayPauseIndexing, // "Пауза індексації" / "Pause indexing"
    TrayCheckUpdates,  // "Перевірити оновлення" / "Check for updates"
    TrayQuit,          // "Вийти" / "Quit"
    MenuLanguage,      // submenu title "Мова" / "Language"
    LangAuto,          // "Авто (система)" / "Auto (system)"
    SettingsTitle,     // "Налаштування" / "Settings" (window title after "Mnema — ")
    CloseSettings,     // "Закрити налаштування" / "Close Settings"
    MenuEdit,          // "Редагувати" / "Edit"
    MenuWindow,        // "Вікно" / "Window"
}

pub const ALL_KEYS: &[Key] = &[
    Key::TrayStatus,
    Key::TrayShowSearch,
    Key::TrayOpenSettings,
    Key::TrayPauseIndexing,
    Key::TrayCheckUpdates,
    Key::TrayQuit,
    Key::MenuLanguage,
    Key::LangAuto,
    Key::SettingsTitle,
    Key::CloseSettings,
    Key::MenuEdit,
    Key::MenuWindow,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    use Key::*;
    match (lang, key) {
        (Lang::Uk, TrayStatus) => "Проіндексовано —",
        (Lang::En, TrayStatus) => "Indexed —",
        (Lang::Uk, TrayShowSearch) => "Показати пошук",
        (Lang::En, TrayShowSearch) => "Show search",
        (Lang::Uk, TrayOpenSettings) => "Відкрити налаштування",
        (Lang::En, TrayOpenSettings) => "Open settings",
        (Lang::Uk, TrayPauseIndexing) => "Пауза індексації",
        (Lang::En, TrayPauseIndexing) => "Pause indexing",
        (Lang::Uk, TrayCheckUpdates) => "Перевірити оновлення",
        (Lang::En, TrayCheckUpdates) => "Check for updates",
        (Lang::Uk, TrayQuit) => "Вийти",
        (Lang::En, TrayQuit) => "Quit",
        (Lang::Uk, MenuLanguage) => "Мова",
        (Lang::En, MenuLanguage) => "Language",
        (Lang::Uk, LangAuto) => "Авто (система)",
        (Lang::En, LangAuto) => "Auto (system)",
        (Lang::Uk, SettingsTitle) => "Налаштування",
        (Lang::En, SettingsTitle) => "Settings",
        (Lang::Uk, CloseSettings) => "Закрити налаштування",
        (Lang::En, CloseSettings) => "Close Settings",
        (Lang::Uk, MenuEdit) => "Редагувати",
        (Lang::En, MenuEdit) => "Edit",
        (Lang::Uk, MenuWindow) => "Вікно",
        (Lang::En, MenuWindow) => "Window",
    }
}

/// Language names shown in their own language (endonyms) for the selector. These
/// live here (not in `tray.rs`) so the hardcode guard stays green — a Cyrillic
/// endonym in `tray.rs` would trip it (P1-3). `Auto`'s label is `Key::LangAuto`.
pub fn endonym(choice: LocaleChoice) -> &'static str {
    match choice {
        LocaleChoice::Uk => "Українська",
        LocaleChoice::En => "English",
        LocaleChoice::Auto => "", // Auto uses t(lang, Key::LangAuto) instead
    }
}

const LOCALE_KEY: &str = "locale";

fn choice_to_str(c: LocaleChoice) -> &'static str {
    match c {
        LocaleChoice::Auto => "auto",
        LocaleChoice::Uk => "uk",
        LocaleChoice::En => "en",
    }
}

fn choice_from_str(s: &str) -> LocaleChoice {
    match s {
        "uk" => LocaleChoice::Uk,
        "en" => LocaleChoice::En,
        _ => LocaleChoice::Auto, // "auto" and anything unknown
    }
}

/// Reads the persisted locale choice. Any failure — missing file, unreadable
/// JSON, or an unrecognized `locale` value — falls back to `Auto` rather than
/// erroring, because this runs at start-up before there is anywhere to report
/// an error to.
pub fn read_choice(data_dir: &Path) -> LocaleChoice {
    let raw = match std::fs::read_to_string(paths::prefs_path(data_dir)) {
        Ok(s) => s,
        Err(_) => return LocaleChoice::Auto,
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return LocaleChoice::Auto,
    };
    choice_from_str(
        value
            .get(LOCALE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("auto"),
    )
}

/// Persists the locale choice, preserving whatever other keys are already in
/// the file — forward-safe, so a field a newer version wrote survives a write
/// from this one.
pub fn write_choice(data_dir: &Path, choice: LocaleChoice) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = paths::prefs_path(data_dir);
    // Preserve unknown fields a newer version may have written.
    let mut obj = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s).ok())
        .unwrap_or_default();
    obj.insert(
        LOCALE_KEY.into(),
        serde_json::Value::String(choice_to_str(choice).into()),
    );
    let body = serde_json::to_vec_pretty(&serde_json::Value::Object(obj))?;
    // Atomic: write a sibling temp file, then rename over the target. POSIX
    // rename() is atomic, so a reader never observes a partially-written file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &body)?;
    // TODO(win): std::fs::rename errors on Windows when `path` already exists,
    // instead of replacing it atomically. This project's CI does not run
    // Windows; the win-pve live pass must confirm. If it fails there, replace
    // this with remove-then-rename or the `ReplaceFileW` API.
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_subtag_handles_real_os_grammar() {
        assert_eq!(primary_subtag(Some("uk-UA")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("uk_UA.UTF-8")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("UK-ua")).as_deref(), Some("uk"));
        assert_eq!(primary_subtag(Some("en")).as_deref(), Some("en"));
        assert_eq!(primary_subtag(Some("C")), None);
        assert_eq!(primary_subtag(Some("POSIX")), None);
        assert_eq!(primary_subtag(Some("")), None);
        assert_eq!(primary_subtag(None), None);
    }

    #[test]
    fn resolve_auto_picks_supported_else_english() {
        assert_eq!(resolve(LocaleChoice::Auto, Some("uk-UA")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::Auto, Some("uk")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::Auto, Some("en-US")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, Some("de-DE")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, Some("zh-CN")), Lang::En);
        assert_eq!(resolve(LocaleChoice::Auto, None), Lang::En);
    }

    #[test]
    fn resolve_explicit_choice_ignores_os() {
        assert_eq!(resolve(LocaleChoice::Uk, Some("en-US")), Lang::Uk);
        assert_eq!(resolve(LocaleChoice::En, Some("uk-UA")), Lang::En);
    }

    #[test]
    fn choice_round_trips_through_prefs() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_choice(dir.path()), LocaleChoice::Auto); // no file → Auto
        write_choice(dir.path(), LocaleChoice::Uk).unwrap();
        assert_eq!(read_choice(dir.path()), LocaleChoice::Uk);
        write_choice(dir.path(), LocaleChoice::En).unwrap();
        assert_eq!(read_choice(dir.path()), LocaleChoice::En);
    }

    #[test]
    fn unreadable_or_unknown_prefs_fall_back_to_auto() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(paths::prefs_path(dir.path()), b"{ not json").unwrap();
        assert_eq!(read_choice(dir.path()), LocaleChoice::Auto);
        std::fs::write(paths::prefs_path(dir.path()), br#"{"locale":"martian"}"#).unwrap();
        assert_eq!(read_choice(dir.path()), LocaleChoice::Auto);
    }

    #[test]
    fn write_creates_the_data_dir_when_missing() {
        let base = tempfile::tempdir().unwrap();
        let data_dir = base.path().join("does-not-exist-yet"); // deliberately NOT created
        write_choice(&data_dir, LocaleChoice::Uk).unwrap(); // must create the dir itself
        assert!(paths::prefs_path(&data_dir).exists());
        assert_eq!(read_choice(&data_dir), LocaleChoice::Uk);
    }

    #[test]
    fn write_preserves_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            paths::prefs_path(dir.path()),
            br#"{"locale":"auto","theme":"dark"}"#,
        )
        .unwrap();
        write_choice(dir.path(), LocaleChoice::Uk).unwrap();
        let raw = std::fs::read_to_string(paths::prefs_path(dir.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["theme"], "dark", "unknown field dropped: {raw}");
        assert_eq!(parsed["locale"], "uk", "locale not updated: {raw}");
        assert_eq!(read_choice(dir.path()), LocaleChoice::Uk);
    }

    #[test]
    fn every_key_has_both_languages_and_is_non_empty() {
        for &key in ALL_KEYS {
            assert!(!t(Lang::Uk, key).is_empty(), "UK missing for {key:?}");
            assert!(!t(Lang::En, key).is_empty(), "EN missing for {key:?}");
        }
    }

    #[test]
    fn tray_labels_differ_by_language() {
        assert_eq!(t(Lang::Uk, Key::TrayQuit), "Вийти");
        assert_eq!(t(Lang::En, Key::TrayQuit), "Quit");
    }
}
