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
        "stop_indexing" => format!("⏹ {}", locale::t(lang, Key::TrayStopIndexing)),
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
    "stop_indexing",
    "quit",
];

/// The «Мова» submenu's three items as pure `(id, label, checked)` data —
/// the only part of the submenu that carries a decision (which language is
/// currently selected). Kept separate from `CheckMenuItem` construction so a
/// headless test can catch a wrong-variant mapping (e.g. `lang_en` compared
/// against `LocaleChoice::Uk`) or an all-checked/all-unchecked slip — neither
/// of which any other test here would catch, and the built `Menu` itself is
/// macOS main-thread-only (see `TRAY_ITEM_IDS`), so this is the only headless
/// path to it.
fn lang_menu_items(lang: Lang, choice: LocaleChoice) -> [(&'static str, String, bool); 3] {
    [
        (
            "lang_auto",
            locale::t(lang, Key::LangAuto).to_string(),
            choice == LocaleChoice::Auto,
        ),
        (
            "lang_uk",
            locale::endonym(LocaleChoice::Uk).to_string(),
            choice == LocaleChoice::Uk,
        ),
        (
            "lang_en",
            locale::endonym(LocaleChoice::En).to_string(),
            choice == LocaleChoice::En,
        ),
    ]
}

/// Assembles the tray menu for a resolved language and the persisted choice
/// behind it — §8, plus the «Мова» submenu (§D129) that lets the user pin a
/// language or return to Auto (`lang_auto`/`lang_uk`/`lang_en`, checked to
/// match `choice`). Like `build_tray`, this needs the main thread on macOS
/// (see `TRAY_ITEM_IDS`) and so is exercised only by the live run, not a
/// headless test; from Task 6, it is also what a language change calls to
/// relabel the live menu via `set_menu`.
///
/// Hands back the Stop item alongside the menu, because whoever swaps this
/// menu in has to keep [`StopItem`]'s slot pointing at the item that is
/// actually on screen — see [`swap_tray_menu`], which is the only caller that
/// should be doing either. The item is built **disabled**: whether there is a
/// job to stop is a fact about `AppState`, not about the menu, and reading it
/// here would give this function a second, hidden input. The caller seeds it.
pub fn build_tray_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
    lang: Lang,
    choice: LocaleChoice,
) -> tauri::Result<(Menu<R>, MenuItem<R>)> {
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

    // «Мова»: Auto plus the two supported languages, in their own endonyms.
    // The (id, label, checked) triples come from `lang_menu_items` — pure
    // data, headlessly tested — rather than being computed inline here.
    let [
        (auto_id, auto_label, auto_checked),
        (uk_id, uk_label, uk_checked),
        (en_id, en_label, en_checked),
    ] = lang_menu_items(lang, choice);
    let lang_auto =
        CheckMenuItem::with_id(app, auto_id, auto_label, true, auto_checked, None::<&str>)?;
    let lang_uk = CheckMenuItem::with_id(app, uk_id, uk_label, true, uk_checked, None::<&str>)?;
    let lang_en = CheckMenuItem::with_id(app, en_id, en_label, true, en_checked, None::<&str>)?;
    let language_menu = Submenu::with_id_and_items(
        app,
        "lang_menu",
        locale::t(lang, Key::MenuLanguage),
        true,
        &[&lang_auto, &lang_uk, &lang_en],
    )?;

    let stop = MenuItem::with_id(
        app,
        "stop_indexing",
        tray_label(lang, "stop_indexing"),
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", tray_label(lang, "quit"), true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &show_search,
            &open_settings,
            &language_menu,
            &PredefinedMenuItem::separator(app)?,
            &stop,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    Ok((menu, stop))
}

/// The handle to the tray's «Зупинити індексацію» item, as managed state.
///
/// It is here, and read out of here on every use, because the item on screen is
/// replaced whenever the language changes: [`build_tray_menu`] builds a whole
/// new menu and [`swap_tray_menu`] swaps it in. A caller that had captured one
/// `MenuItem` would go on addressing an item that is in no menu, and the one a
/// person can see would keep whatever state it was built with. `MenuItem<R>` is
/// `Send + Sync` (Tauri unsafe-impls both on the inner type,
/// `tauri-2.11.5/src/menu/mod.rs:90-91`), so holding it here is sound.
pub struct StopItem<R: Runtime>(pub std::sync::Mutex<Option<MenuItem<R>>>);

impl<R: Runtime> StopItem<R> {
    /// Points the slot at a new item and seeds it from the job that is running
    /// **now** — never from a boolean remembered across the rebuild, which is
    /// the shape D136's repair deleted for surviving a remount as a lie.
    fn replace(&self, app: &tauri::AppHandle<R>, item: MenuItem<R>) {
        let _ = item.set_enabled(app.state::<crate::state::AppState>().job_is_running());
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(item);
    }
}

/// Enables or disables whichever Stop item the slot holds **right now**.
///
/// A function over the `AppHandle` rather than a method on a held item, which
/// is the whole point of [`StopItem`]: between one call and the next, a
/// language change may have put a different item there. Does nothing when the
/// slot is unmanaged, which is every headless test and every moment before
/// `.setup` reaches the tray.
pub fn set_stop_enabled<R: Runtime>(app: &tauri::AppHandle<R>, enabled: bool) {
    if let Some(slot) = app.try_state::<StopItem<R>>()
        && let Some(item) = slot
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
    {
        let _ = item.set_enabled(enabled);
    }
}

/// Rebuilds the tray menu in `lang` and puts it on the live tray, keeping
/// [`StopItem`]'s slot in step.
///
/// Both callers that relabel the tray go through here — `locale::apply_locale`
/// on a language change, and `lib.rs`'s handler when a change failed to persist
/// and the checkmark has to be put back. One predicate rather than two, because
/// the half that is easy to forget is not the `set_menu`: it is that the old
/// Stop item has just left the menu, and anything still holding it is now
/// talking to nothing.
///
/// Best-effort throughout (`let _ =`), like everything else on the language
/// path: this runs from a tray callback that has no error channel of its own
/// (§6). Does nothing when there is no tray, which is every headless test.
pub fn swap_tray_menu<R: Runtime>(app: &tauri::AppHandle<R>, lang: Lang, choice: LocaleChoice) {
    let Some(tray) = app.tray_by_id("mnema-tray") else {
        return;
    };
    let Ok((menu, stop)) = build_tray_menu(app, lang, choice) else {
        return;
    };
    if let Some(slot) = app.try_state::<StopItem<R>>() {
        slot.replace(app, stop);
    }
    let _ = tray.set_menu(Some(menu));
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
///
/// Hands the Stop item back to `.setup`, which is the one place that can put it
/// into [`StopItem`]'s slot: the slot has to be managed before anything can be
/// read out of it, and nothing before this call has an item to put there.
pub fn build_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<MenuItem<R>> {
    let locale_state = app.state::<crate::state::AppState>().locale();
    let (menu, stop) = build_tray_menu(app, locale_state.effective, locale_state.choice)?;

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
    Ok(stop)
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

    /// Both languages, because the glyph is composed once at this call site and
    /// the text comes from the catalog: a label that lost either half would
    /// still be non-empty, and `every_tray_id_has_a_non_empty_label...` would
    /// go on passing.
    #[test]
    fn the_stop_item_composes_its_glyph_with_both_translations() {
        assert_eq!(
            tray_label(crate::locale::Lang::Uk, "stop_indexing"),
            "⏹ Зупинити індексацію"
        );
        assert_eq!(
            tray_label(crate::locale::Lang::En, "stop_indexing"),
            "⏹ Stop indexing"
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
                "stop_indexing",
                "quit"
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

    /// The amended §8 dropped «Перевірити оновлення», and dropping an item is
    /// not the same as leaving its label behind: a `tray_label` that still
    /// answered for `check_updates` would let a rebuilt menu carry the item
    /// back with nothing complaining. Same shape as the unknown-id case above,
    /// because after the amendment that is exactly what this id is.
    #[test]
    #[should_panic(expected = "unknown tray id")]
    fn tray_label_rejects_the_deleted_update_check() {
        tray_label(Lang::En, "check_updates");
    }

    #[test]
    fn exactly_one_language_item_is_checked_and_it_matches_choice() {
        use LocaleChoice::*;
        for (choice, checked_id) in [(Auto, "lang_auto"), (Uk, "lang_uk"), (En, "lang_en")] {
            let items = lang_menu_items(Lang::En, choice);
            for (id, _label, checked) in &items {
                // The item matching `choice` is checked, and — same assertion,
                // both directions at once — the other two are not.
                assert_eq!(
                    *checked,
                    *id == checked_id,
                    "wrong checked state: {id} @ {choice:?}"
                );
            }
            // Belt against an all-checked or all-unchecked slip, which the
            // per-item comparison above would not catch on its own.
            assert_eq!(items.iter().filter(|(_, _, c)| *c).count(), 1);
        }
    }

    #[test]
    fn language_items_are_wired_to_the_catalog() {
        let it = lang_menu_items(Lang::En, LocaleChoice::Auto);
        assert_eq!(
            [it[0].0, it[1].0, it[2].0],
            ["lang_auto", "lang_uk", "lang_en"]
        );
        assert_eq!(it[0].1, locale::t(Lang::En, Key::LangAuto));
        assert_eq!(it[1].1, locale::endonym(LocaleChoice::Uk));
        assert_eq!(it[2].1, locale::endonym(LocaleChoice::En));
    }
}
