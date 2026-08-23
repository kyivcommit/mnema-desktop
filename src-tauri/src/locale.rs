//! Interface localization (§D129). English is the fallback for any locale the
//! app does not support (§D80, amended). This module owns the effective locale;
//! `mnema-core::Coordinate::render` is prompt-only and deliberately untouched.

use crate::paths;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, Runtime};

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

/// What the runtime seam carries: the persisted choice, and what it currently
/// resolves to. `Copy` so [`crate::state::AppState::locale`] can hand back a
/// value instead of a guard.
#[derive(Debug, Clone, Copy)]
pub struct LocaleState {
    pub choice: LocaleChoice,
    pub effective: Lang,
}

/// AppHandle-free core, so the resolution is unit-testable without a runtime.
pub fn effective_core(data_dir: &Path, os: Option<&str>) -> LocaleState {
    let choice = read_choice(data_dir);
    LocaleState {
        choice,
        effective: resolve(choice, os),
    }
}

/// Reads the persisted choice + the OS locale and resolves the effective
/// language. Calls `app.path()`, which PANICS if the path resolver is not yet
/// managed — so this must run in or after `.setup`, NEVER from the menu-build
/// closure (that path uses [`boot_lang`], which needs no resolver). If the data
/// dir cannot be located, fall back to Auto→OS→EN rather than a bogus path.
pub fn resolve_effective<R: Runtime>(app: &AppHandle<R>) -> LocaleState {
    let os = sys_locale::get_locale();
    match app.path().app_local_data_dir() {
        Ok(dir) => effective_core(&dir, os.as_deref()),
        Err(_) => LocaleState {
            choice: LocaleChoice::Auto,
            effective: resolve(LocaleChoice::Auto, os.as_deref()),
        },
    }
}

/// The language for the FIRST app-menu build, which happens during `build()`
/// before the path resolver (and `AppState`) exist — so it cannot read prefs
/// and must NOT call `app.path()` (that panics: "state() called before
/// manage()"). Resolves from the OS locale alone; `.setup` rebuilds the menu
/// with the persisted choice once the resolver is up, and `apply_locale`
/// rebuilds it on every change (the app menu stays hidden until settings opens,
/// after both later rebuilds have run).
pub fn boot_lang() -> Lang {
    resolve(LocaleChoice::Auto, sys_locale::get_locale().as_deref())
}

/// The IPC shape of [`LocaleState`]. A string rather than the enums
/// themselves: the enums have no `Serialize`, and the webview needs
/// `"auto"|"uk"|"en"` / `"uk"|"en"`, not a derived Rust-shaped encoding.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleReply {
    pub choice: String,
    pub effective: String,
}

fn lang_tag(l: Lang) -> &'static str {
    match l {
        Lang::Uk => "uk",
        Lang::En => "en",
    }
}

#[tauri::command]
pub fn get_locale(state: tauri::State<'_, crate::state::AppState>) -> LocaleReply {
    let s = state.locale();
    LocaleReply {
        choice: choice_to_str(s.choice).into(),
        effective: lang_tag(s.effective).into(),
    }
}

/// The shared path for a language change, used by BOTH the `set_locale`
/// command and the tray callback (Task 6): persist → update state → apply
/// natively. Writes to the data dir `AppState` resolved at startup
/// (`state.rs:16`).
pub fn apply_choice<R: Runtime>(
    app: &AppHandle<R>,
    state: &crate::state::AppState,
    choice: LocaleChoice,
) -> Result<(), crate::error::Error> {
    write_choice(state.data_dir(), choice)?; // a write failure surfaces (spec §6)
    let effective = resolve(choice, sys_locale::get_locale().as_deref());
    state.set_locale_state(LocaleState { choice, effective });
    apply_locale(app, effective); // Task 6 fills apply_locale
    Ok(())
}

/// Applies a resolved language to everything already on screen: the tray menu
/// (labels + the «Мова» checkmarks), the settings window's native title, and
/// the macOS app menu — then broadcasts the change so the webview can follow.
///
/// Reads the persisted choice back from `AppState`, which `apply_choice` has
/// already updated before calling here, so the checkmarks land on the NEW
/// choice rather than the old one. Every step is best-effort (`let _ =`): a
/// language change relabels as much as it can even if one surface refuses, and
/// this runs from a tray callback with no error channel of its own (§6).
fn apply_locale<R: Runtime>(app: &AppHandle<R>, lang: Lang) {
    let choice = app.state::<crate::state::AppState>().locale().choice;
    // The tray menu is rebuilt whole and swapped in via `set_menu`; the tray
    // icon and its `on_tray_icon_event` (the positioner) are left in place.
    if let Some(tray) = app.tray_by_id("mnema-tray")
        && let Ok(menu) = crate::tray::build_tray_menu(app, lang, choice)
    {
        let _ = tray.set_menu(Some(menu));
    }
    // The settings window's native OS title, re-set whether or not it is
    // visible so an already-open or merely-hidden window is right next time.
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_title(&format!("Mnema — {}", t(lang, Key::SettingsTitle)));
    }
    // The macOS app menu, rebuilt always — not only while settings is visible
    // (§5.7) — so a change made from the tray with the menu bar hidden is
    // already applied when it next shows. Off macOS this is the default menu
    // and the rebuild is a harmless no-op.
    if let Ok(menu) = crate::build_app_menu(app, lang) {
        let _ = app.set_menu(menu);
    }
    // Broadcast the new language so any open webview can re-render its own
    // strings; the native chrome above is already relabelled.
    let _ = app.emit("locale-changed", lang_tag(lang));
}

#[tauri::command]
pub fn set_locale<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    choice: String,
) -> Result<(), crate::error::Error> {
    apply_choice(&app, &state, choice_from_str(&choice))
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

    #[cfg(unix)]
    #[test]
    fn failed_write_keeps_the_previous_choice() {
        // On a persist failure the tray callback logs and rebuilds its menu from
        // the UNCHANGED AppState ("старий вибір лишається", spec §5.8). That is
        // only correct because `write_choice` is atomic (temp + rename): a failed
        // write must both surface as an error AND leave the previously persisted
        // choice intact. This pins that invariant — an in-place write would go
        // red here (the file, writable inside a read-only dir, would be truncated
        // to the new value). The callback's own log + menu rebuild live in a muda
        // main-thread closure and are covered by the live run, not this test.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_choice(dir.path(), LocaleChoice::Uk).unwrap();

        // Read-only data dir: creating the sibling temp file must fail.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = write_choice(dir.path(), LocaleChoice::En);
        // Restore perms first, so the tempdir cleans up whatever the asserts do.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            failed.is_err(),
            "a write into a read-only dir must surface an error"
        );
        assert_eq!(
            read_choice(dir.path()),
            LocaleChoice::Uk,
            "a failed write must not change the persisted choice"
        );
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

    // resolve_effective is split so the AppHandle-free core is testable:
    #[test]
    fn effective_core_reads_choice_then_resolves() {
        let dir = tempfile::tempdir().unwrap();
        // No prefs → Auto → follows OS.
        assert_eq!(
            effective_core(dir.path(), Some("uk-UA")).effective,
            Lang::Uk
        );
        assert_eq!(
            effective_core(dir.path(), Some("de-DE")).effective,
            Lang::En
        );
        // Explicit pin ignores OS.
        write_choice(dir.path(), LocaleChoice::En).unwrap();
        let s = effective_core(dir.path(), Some("uk-UA"));
        assert_eq!(s.choice, LocaleChoice::En);
        assert_eq!(s.effective, Lang::En);
    }

    #[test]
    fn boot_lang_needs_no_app_handle_or_path_resolver() {
        // The first app-menu build calls this during `build()`, before the
        // Tauri runtime / path resolver exist. That it is callable with nothing
        // — no AppHandle, no `app.path()` — is the regression guard for the
        // start-up panic a menu-closure `resolve_effective` once caused.
        assert!(matches!(boot_lang(), Lang::Uk | Lang::En));
    }
}
