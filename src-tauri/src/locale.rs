//! Interface localization (§D129). English is the fallback for any locale the
//! app does not support (§D80, amended). This module owns the effective locale;
//! `mnema-core::Coordinate::render` is prompt-only and deliberately untouched.

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
/// only: no emoji, no shortcut hints, no endonyms — those are composed at the
/// call site or, for endonyms, live in `endonym` below.
///
/// The shortcut hint is the one worth naming, because it is no longer a
/// literal anywhere: `tray.rs`'s `tray_label` derives `(⌥Space)` and every
/// other form of it from the `HotkeyState` the operating system reports,
/// through `shortcut::format_shortcut`. It was a fixed string in this
/// catalogue's neighbour until Task 11a, and a person who changed the shortcut
/// read the old one off the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    // The five that replaced `TrayStatus` (Task 5): `tray::status_label` is
    // the ONE place that picks between them and appends the number and, for
    // the count-carrying two, the plural word `files_word` below computes —
    // this catalog holds only the fixed words, never a number.
    TrayIndexingPercent, // "Індексація" / "Indexing" (+ " NN %")
    TrayIndexingCount,   // "Індексація:" / "Indexing:" (+ " N <word>")
    TrayEmbedding,       // "Вбудовування" / "Embedding" (+ " NN %")
    TrayRemoving,        // "Видаляємо теку…" / "Removing folder…" (whole sentence)
    TrayScanned,         // "Проскановано:" / "Scanned:" (+ " N <word>")
    TrayShowSearch,      // "Показати пошук" / "Show search"
    TrayOpenSettings,    // "Відкрити налаштування" / "Open settings"
    TrayStopIndexing,    // "Зупинити сканування" / "Stop scanning"
    TrayQuit,            // "Вийти" / "Quit"
    MenuLanguage,        // submenu title "Мова" / "Language"
    LangAuto,            // "Авто (система)" / "Auto (system)"
    SettingsTitle,       // "Налаштування" / "Settings" (window title after "Mnema — ")
    CloseSettings,       // "Закрити налаштування" / "Close Settings"
    MenuEdit,            // "Редагувати" / "Edit"
    MenuWindow,          // "Вікно" / "Window"
    // The two hotkey refusals that are OURS rather than the parser's. They are
    // here, and not in `error.rs` with every other rejection sentence, because
    // each answers a PRESS a person made and there is a better sentence for it
    // than the library's: `global-hotkey` accepts a bare `Space` outright
    // (`hotkey.rs:174-178`) and refuses a modifier-only press by asking the
    // reader to open an issue against `github.com/tauri-apps/muda`
    // (`hotkey.rs:40`). `set_hotkey` renders these in the active language and
    // hands them back through `Error::HotkeyRefused`.
    HotkeyNeedsAKey,      // modifiers with no key, or nothing at all
    HotkeyNeedsAModifier, // a key with no modifier
}

pub const ALL_KEYS: &[Key] = &[
    Key::TrayIndexingPercent,
    Key::TrayIndexingCount,
    Key::TrayEmbedding,
    Key::TrayRemoving,
    Key::TrayScanned,
    Key::TrayShowSearch,
    Key::TrayOpenSettings,
    Key::TrayStopIndexing,
    Key::TrayQuit,
    Key::MenuLanguage,
    Key::LangAuto,
    Key::SettingsTitle,
    Key::CloseSettings,
    Key::MenuEdit,
    Key::MenuWindow,
    Key::HotkeyNeedsAKey,
    Key::HotkeyNeedsAModifier,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    use Key::*;
    match (lang, key) {
        (Lang::Uk, TrayIndexingPercent) => "Індексація",
        (Lang::En, TrayIndexingPercent) => "Indexing",
        (Lang::Uk, TrayIndexingCount) => "Індексація:",
        (Lang::En, TrayIndexingCount) => "Indexing:",
        (Lang::Uk, TrayEmbedding) => "Вбудовування",
        (Lang::En, TrayEmbedding) => "Embedding",
        (Lang::Uk, TrayRemoving) => "Видаляємо теку…",
        (Lang::En, TrayRemoving) => "Removing folder…",
        (Lang::Uk, TrayScanned) => "Проскановано:",
        (Lang::En, TrayScanned) => "Scanned:",
        (Lang::Uk, TrayShowSearch) => "Показати пошук",
        (Lang::En, TrayShowSearch) => "Show search",
        (Lang::Uk, TrayOpenSettings) => "Відкрити налаштування",
        (Lang::En, TrayOpenSettings) => "Open settings",
        (Lang::Uk, TrayStopIndexing) => "Зупинити сканування",
        (Lang::En, TrayStopIndexing) => "Stop scanning",
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
        (Lang::Uk, HotkeyNeedsAKey) => {
            "Комбінація має закінчуватися клавішею, а не самими модифікаторами"
        }
        (Lang::En, HotkeyNeedsAKey) => "a shortcut has to end in a key, not in modifiers alone",
        (Lang::Uk, HotkeyNeedsAModifier) => {
            "Комбінація має містити щонайменше один модифікатор: Ctrl, Alt, Shift або Cmd"
        }
        (Lang::En, HotkeyNeedsAModifier) => {
            "a shortcut needs at least one modifier: Ctrl, Alt, Shift or Cmd"
        }
    }
}

/// The word for "file(s)" that agrees with `n`, in `lang`.
///
/// The Ukrainian arm is the reason this exists at all: «файл» is a count noun
/// with three plural forms rather than English's two, and which form applies is
/// not "does `n` end in 1" — it is the Slavic rule where the LAST TWO digits
/// decide, so that 11–14 fall to the "many" form even though they end in
/// 1/2/4. Getting this wrong is not cosmetic: `tray::status_label` builds a
/// sentence with it, and a form that disagreed with the number beside it would
/// be exactly the kind of tray text this task was written to stop showing.
///
/// `n` is signed because [`crate::scan_state::ScanState::files`] is signed; the
/// magnitude is what decides the form, so `n.unsigned_abs()` is taken up front
/// and a negative count (a state the type allows but no writer here produces)
/// pluralizes the same as its positive twin rather than panicking or picking
/// arbitrarily.
pub fn files_word(lang: Lang, n: i64) -> &'static str {
    // Review round 1, Minor 3: this used to sit inside the Ukrainian arm
    // only, which made the doc comment above false for the English one —
    // `files_word(En, -1)` answered "files" while `files_word(Uk, -1)`
    // answered «файл». Taken here, before the `match`, so BOTH arms decide
    // off the magnitude the doc comment promises.
    let n = n.unsigned_abs();
    match lang {
        Lang::En => {
            if n == 1 {
                "file"
            } else {
                "files"
            }
        }
        Lang::Uk => {
            let last_two = n % 100;
            let last_one = n % 10;
            if (11..=14).contains(&last_two) {
                "файлів"
            } else if last_one == 1 {
                "файл"
            } else if (2..=4).contains(&last_one) {
                "файли"
            } else {
                "файлів"
            }
        }
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
    let all = crate::prefs::read_all(data_dir);
    choice_from_str(
        all.get(LOCALE_KEY)
            .and_then(|v| v.as_str())
            .unwrap_or("auto"),
    )
}

/// Persists the locale choice, preserving whatever other keys are already in
/// the file — forward-safe, so a field a newer version wrote survives a write
/// from this one. The file itself, including the atomicity of the write and
/// what happens to a malformed one, is [`crate::prefs`]'s concern: from PR 9
/// the locale is no longer the only key in it.
pub fn write_choice(data_dir: &Path, choice: LocaleChoice) -> std::io::Result<()> {
    crate::prefs::write_key(
        data_dir,
        LOCALE_KEY,
        serde_json::Value::String(choice_to_str(choice).into()),
    )
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
    // The rebuild also replaces the status/Stop items a job may be about to
    // redraw, which is why the swap is `tray::swap_tray_menu` and not a
    // `set_menu` here — see `tray::TrayItems`.
    crate::tray::swap_tray_menu(app, lang, choice);
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
    // The tests reach the file directly; the module itself no longer does.
    use crate::paths;

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

    /// The Ukrainian three-arm plural, on the table the brief pins:
    /// 1 → «файл» singular; 2, 22 → «файли» (ends in 2–4, not 12–14); 5, 25,
    /// 111 → «файлів» (ends in 5+ or falls in the 11–14 "teen" band by its last
    /// two digits); 11, 21 are the pair that separates "ends in 1" from
    /// "the Slavic rule": 21 ends in 1 and is NOT in 11–14, so it takes the
    /// singular form same as 1, while 11 ends in 1 and IS in 11–14, so it takes
    /// the plural — a naive "n % 10 == 1 → singular" rule would answer «файл»
    /// for both and go red only on this one row.
    ///
    /// Review round 1, Minor 5: `0`, `10`, `20`, `100` added — each ends in 0,
    /// which none of the rows above do, and each is the rule's own boundary
    /// rather than a value inside one of its bands. `0` in particular is the
    /// state a fresh install's tray is in, pinned only indirectly before this
    /// (through `Idle`'s «Проскановано: 0 файлів») — this puts it in the rule's
    /// own table instead of relying on that one caller to keep exercising it.
    #[test]
    fn ukrainian_file_count_takes_the_slavic_plural() {
        for (n, word) in [
            (0, "файлів"),
            (1, "файл"),
            (2, "файли"),
            (5, "файлів"),
            (10, "файлів"),
            (11, "файлів"),
            (20, "файлів"),
            (21, "файл"),
            (22, "файли"),
            (25, "файлів"),
            (100, "файлів"),
            (111, "файлів"),
        ] {
            assert_eq!(files_word(Lang::Uk, n), word, "n = {n}");
        }
    }

    /// Review round 1, Minor 3: `files_word`'s doc claims the magnitude alone
    /// decides, in both languages — the negative row is what makes that a
    /// tested claim rather than a sentence about the Ukrainian arm only.
    /// −1 takes the singular the SAME as 1 (not "files", which `n == 1`
    /// alone, without `unsigned_abs`, would have answered); −3 takes the
    /// plural the same as 3, in both languages at once so a fix that moved
    /// `unsigned_abs` into only one arm again would still go red here.
    #[test]
    fn a_negative_count_pluralizes_the_same_as_its_positive_twin() {
        for (n, uk, en) in [(-1, "файл", "file"), (-3, "файли", "files")] {
            assert_eq!(files_word(Lang::Uk, n), uk, "n = {n}");
            assert_eq!(files_word(Lang::En, n), en, "n = {n}");
        }
    }

    /// English has only the ordinary two forms, and 1 is the only singular one
    /// — asserted against a neighbour (0) so the rule pinned is "n == 1", not
    /// "n is odd" or some other coincidence that would also pass on 1 alone.
    #[test]
    fn english_file_count_is_singular_only_at_one() {
        assert_eq!(files_word(Lang::En, 1), "file");
        assert_eq!(files_word(Lang::En, 0), "files");
        assert_eq!(files_word(Lang::En, 2), "files");
        assert_eq!(files_word(Lang::En, 21), "files");
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
