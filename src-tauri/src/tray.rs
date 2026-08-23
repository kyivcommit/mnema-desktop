//! The menu-bar tray: the resident's only always-present surface and, by §6,
//! the only way to quit. Built in `lib.rs::run`'s `setup` hook.

use tauri::Manager as _;
use tauri::{
    Runtime,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
};

use crate::locale::{self, Key, Lang, LocaleChoice};

/// Composes one tray item's label from the catalog (§D129). The emoji and the
/// `(⌥Space)` hint are literals here, not in the catalog: the same glyph and
/// the same shortcut in both languages, not translatable text.
pub fn tray_label(lang: Lang, id: &str) -> String {
    match id {
        "status" => locale::t(lang, Key::TrayStatus).to_string(),
        "show_search" => format!("🔍 {} (⌥Space)", locale::t(lang, Key::TrayShowSearch)),
        "open_settings" => format!("⚙ {}", locale::t(lang, Key::TrayOpenSettings)),
        "pause_indexing" => format!("⏸ {}", locale::t(lang, Key::TrayPauseIndexing)),
        "check_updates" => format!("↻ {}", locale::t(lang, Key::TrayCheckUpdates)),
        "quit" => format!("⏻ {}", locale::t(lang, Key::TrayQuit)),
        other => panic!("unknown tray id {other}"),
    }
}

/// The tray's action-item ids in display order — §8. Pure data, ids only (no
/// labels: those are locale-dependent, via `tray_label`), so a headless test
/// can guard against spec drift without constructing a native menu. On macOS,
/// `muda` requires the main thread to build even a plain `Menu` — not only
/// the tray *icon* — and `cfg!(test)` inside `muda` only bypasses that check
/// when `muda` itself is compiled for test, not when a dependent crate is;
/// see `muda-0.19.3/src/platform_impl/macos/mod.rs:132,328`. So
/// `build_tray_menu`, like `build_tray`, is exercised only by the live run —
/// this array is what stays headlessly testable.
pub const TRAY_ITEM_IDS: &[&str] = &[
    "status",
    "show_search",
    "open_settings",
    "pause_indexing",
    "check_updates",
    "quit",
];

/// Assembles the tray menu for a resolved language and the persisted choice
/// behind it — §8, plus the «Мова» submenu (§D129) that lets the user pin a
/// language or return to Auto (`lang_auto`/`lang_uk`/`lang_en`, checked to
/// match `choice`). Like `build_tray`, this needs the main thread on macOS
/// (see `TRAY_ITEM_IDS`) and so is exercised only by the live run, not a
/// headless test; from Task 6, it is also what a language change calls to
/// relabel the live menu via `set_menu`.
pub fn build_tray_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
    lang: Lang,
    choice: LocaleChoice,
) -> tauri::Result<Menu<R>> {
    let status = MenuItem::with_id(
        app,
        "status",
        tray_label(lang, "status"),
        false,
        None::<&str>,
    )?;
    let show_search = MenuItem::with_id(
        app,
        "show_search",
        tray_label(lang, "show_search"),
        true,
        None::<&str>,
    )?;
    let open_settings = MenuItem::with_id(
        app,
        "open_settings",
        tray_label(lang, "open_settings"),
        true,
        None::<&str>,
    )?;

    // «Мова»: Auto plus the two supported languages, in their own endonyms
    // (`locale::endonym` — never a Cyrillic literal here, or the hardcode
    // guard trips). Exactly one is checked, matching the persisted `choice`.
    let lang_auto = CheckMenuItem::with_id(
        app,
        "lang_auto",
        locale::t(lang, Key::LangAuto),
        true,
        choice == LocaleChoice::Auto,
        None::<&str>,
    )?;
    let lang_uk = CheckMenuItem::with_id(
        app,
        "lang_uk",
        locale::endonym(LocaleChoice::Uk),
        true,
        choice == LocaleChoice::Uk,
        None::<&str>,
    )?;
    let lang_en = CheckMenuItem::with_id(
        app,
        "lang_en",
        locale::endonym(LocaleChoice::En),
        true,
        choice == LocaleChoice::En,
        None::<&str>,
    )?;
    let language_menu = Submenu::with_id_and_items(
        app,
        "lang_menu",
        locale::t(lang, Key::MenuLanguage),
        true,
        &[&lang_auto, &lang_uk, &lang_en],
    )?;

    let pause = MenuItem::with_id(
        app,
        "pause_indexing",
        tray_label(lang, "pause_indexing"),
        true,
        None::<&str>,
    )?;
    let updates = MenuItem::with_id(
        app,
        "check_updates",
        tray_label(lang, "check_updates"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", tray_label(lang, "quit"), true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &show_search,
            &open_settings,
            &language_menu,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &updates,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// Builds the tray icon and its menu. Reads `AppState`'s locale once — set by
/// `manage_state`, which `lib.rs`'s `setup` hook runs first (`lib.rs:314-315`),
/// so the state is already managed here.
///
/// Menu-event handling (`show_search` → `crate::focus_launcher`,
/// `open_settings`, `quit`, and the language items) is wired at the app level
/// instead of here (`lib.rs`'s `on_menu_event`, Task 6): an event closure
/// bound to the tray itself would need re-attaching on every `set_menu`,
/// which is how a language change re-labels this same menu. Until Task 6
/// lands, the built menu is inert — clicking any item does nothing.
pub fn build_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let locale_state = app.state::<crate::state::AppState>().locale();
    let menu = build_tray_menu(app, locale_state.effective, locale_state.choice)?;

    TrayIconBuilder::with_id("mnema-tray")
        .icon(
            app.default_window_icon()
                .expect("a default window icon")
                .clone(),
        )
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_compose_emoji_and_translation() {
        assert_eq!(
            tray_label(crate::locale::Lang::En, "show_search"),
            "🔍 Show search (⌥Space)"
        );
        assert_eq!(tray_label(crate::locale::Lang::Uk, "quit"), "⏻ Вийти");
        assert_eq!(
            tray_label(crate::locale::Lang::En, "open_settings"),
            "⚙ Open settings"
        );
    }

    #[test]
    fn tray_item_ids_match_spec_order() {
        assert_eq!(
            TRAY_ITEM_IDS,
            [
                "status",
                "show_search",
                "open_settings",
                "pause_indexing",
                "check_updates",
                "quit",
            ],
            "the tray menu drifted from spec §8"
        );
    }

    #[test]
    fn every_tray_id_has_a_non_empty_label_in_both_languages() {
        // lang_auto/lang_uk/lang_en are not `tray_label` ids — they come from
        // `locale::t`/`locale::endonym` directly in `build_tray_menu` — and
        // are covered by locale.rs's own `every_key_has_both_languages...`.
        for &id in TRAY_ITEM_IDS {
            assert!(!tray_label(Lang::Uk, id).is_empty(), "UK missing for {id}");
            assert!(!tray_label(Lang::En, id).is_empty(), "EN missing for {id}");
        }
    }

    #[test]
    #[should_panic(expected = "unknown tray id")]
    fn tray_label_rejects_an_unknown_id() {
        tray_label(Lang::En, "not_a_real_id");
    }
}
