//! The menu-bar tray: the resident's only always-present surface and, by §6,
//! the only way to quit. Built in `lib.rs::run`'s `setup` hook.
//!
//! Nothing here holds a user-facing sentence — the text comes from `locale.rs`
//! and the emoji are composed onto it in [`tray_label`]. The one value that is
//! neither is the shortcut hint on «Показати пошук», and from Task 11a it is
//! not a literal either: it is the `prefs::HotkeyState` the operating system
//! answered with, drawn by `shortcut::format_shortcut`, which mirrors the
//! settings window's own formatter against `ui/src/i18n/shortcut.fixtures.json`
//! — one fixture, two implementations, so neither can drift alone. Three
//! entry points read that state and so three menus can change with it:
//! [`build_tray`] at boot, [`swap_tray_menu`] on a language change, and
//! `prefs::set_hotkey` when the shortcut itself moves.

use tauri::Manager as _;
use tauri::{
    Runtime,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
};

use crate::locale::{self, Key, Lang, LocaleChoice};
use crate::prefs::{HotkeyState, HotkeyStatus};

/// The two tray ids that more than one module has to spell: the item's own
/// menu entry is built here and its press is dispatched in `lib.rs`, so the
/// string is written in two files that no compiler check ties together — a
/// dispatcher arm spelled `"resume_scan"` against a menu item built as
/// `"resume"` is an item that silently does nothing when pressed.
///
/// Only these two. The other ids (`show_search`, `open_settings`, `quit`, the
/// language items) are left as the bare literals they have always been:
/// widening this to all of them is a rename, not a fix, and the review that
/// asked for these two asked for exactly these two.
///
/// `TRAY_ITEM_IDS` is built from them, and so is [`tray_label`]'s match — but
/// `tray_item_ids_match_spec_order` deliberately keeps its literals, because a
/// list compared against the constants it is built from is a list compared
/// against itself.
pub const STOP_ID: &str = "stop_indexing";
pub const RESUME_ID: &str = "resume";

/// Composes one tray item's label from the catalog (§D129). The emoji is a
/// literal here and not in the catalog: the same glyph in both languages, not
/// translatable text.
///
/// 🔴 **The shortcut hint is DERIVED, and that is the whole of Task 11a.** It
/// used to be the literal `(⌥Space)`, written when the shortcut could not be
/// changed. PR 9 made it changeable and the label did not follow: measured on
/// 2026-09-04, a shortcut moved to ⌃⌥Space in the Application section left the
/// tray reading «Показати пошук (⌥Space)» after a restart — a sentence
/// contradicting the data beside it. The hint now comes from the same
/// [`HotkeyState`] the settings window is drawn from, through
/// [`crate::shortcut::format_shortcut`], which mirrors the window's own
/// formatter against a fixture both read.
///
/// And it is drawn ONLY when the operating system says `Registered`. From an
/// `Unavailable` start there is no shortcut to press, so the label names none:
/// parentheses around a combination that does nothing is the same class of
/// defect one step smaller.
pub fn tray_label(lang: Lang, id: &str, hotkey: &HotkeyState) -> String {
    match id {
        // "status" is deliberately NOT a `tray_label` id from Task 5: the
        // sentence it shows carries a number and a phase, which `tray_label`
        // has no `ScanState` to draw from. `status_label` is the one place
        // that composes it now, and this id falls through to the `other` arm
        // below like any id `tray_label` never knew — pinned by
        // `tray_label_rejects_the_status_item_too`.
        "show_search" => match hotkey.status {
            // `Platform::of_this_build` and NOT a `cfg!` written out here: the
            // same constant already answers this question for the settings
            // window, chosen at compile time and sent over the wire, and a
            // second derivation of one fact is how the two halves of a label
            // start disagreeing. See `models::Platform`.
            HotkeyStatus::Registered => format!(
                "🔍 {} ({})",
                locale::t(lang, Key::TrayShowSearch),
                crate::shortcut::format_shortcut(
                    &hotkey.shortcut,
                    crate::models::Platform::of_this_build()
                )
            ),
            HotkeyStatus::Unavailable { .. } => {
                format!("🔍 {}", locale::t(lang, Key::TrayShowSearch))
            }
        },
        "open_settings" => format!("⚙ {}", locale::t(lang, Key::TrayOpenSettings)),
        STOP_ID => format!("⏹ {}", locale::t(lang, Key::TrayStopIndexing)),
        RESUME_ID => format!("▶ {}", locale::t(lang, Key::TrayResumeScanning)),
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
    STOP_ID,
    RESUME_ID,
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

/// Composes the tray's status line from the current [`ScanState`] — the ONE
/// place that picks which of the five phase sentences to draw. Pure: nothing
/// here reads `AppState`, a clock or the index, so it can be pinned against a
/// literal `ScanState` without a runtime.
///
/// The number itself is computed here rather than carried by [`locale::t`]
/// (which is `&'static str`) — a percentage or a file count is not
/// translatable text — and the plural word for a file count comes from
/// [`locale::files_word`], which is the only place the Ukrainian three-arm
/// rule is allowed to live (`locale_guard.rs`).
///
/// `Reading`/`Embedding` divide by `counts.total` and the `total == 0` branch
/// exists precisely so that division never happens on it — there is no reachable
/// path here that computes `done / 0`, which is what the fixture pair
/// (`total: 0` against `total: 200, done: 48`) is asserting.
///
/// [`ScanState`]: crate::scan_state::ScanState
pub fn status_label(lang: Lang, state: &crate::scan_state::ScanState) -> String {
    use crate::scan_state::{Phase, ScanSnapshot};

    match &state.snapshot {
        ScanSnapshot::Running { phase, .. } => match phase {
            Phase::Reading { counts, .. } => {
                // `checked_div` rather than `if counts.total > 0 { .. } else
                // { .. }` (clippy's `manual_checked_ops`) — `None` on
                // `total == 0` is exactly the fixture pair this match
                // separates: a known total draws a percentage, an unknown one
                // draws a count instead of attempting `done / 0`.
                match counts.done.saturating_mul(100).checked_div(counts.total) {
                    Some(percent) => {
                        format!(
                            "{} {} %",
                            locale::t(lang, Key::TrayIndexingPercent),
                            percent
                        )
                    }
                    None => {
                        // Review round 1, Minor 4: `as i64` wraps negative
                        // above `i64::MAX` — unreachable for a real file
                        // count, but `try_from`/`unwrap_or` says the same
                        // thing without a cast that can misrepresent one.
                        let n = i64::try_from(counts.done).unwrap_or(i64::MAX);
                        format!(
                            "{} {} {}",
                            locale::t(lang, Key::TrayIndexingCount),
                            n,
                            locale::files_word(lang, n)
                        )
                    }
                }
            }
            Phase::Embedding { counts } => {
                // Unlike Reading, an unknown total draws `0 %` rather than a
                // count of its own — `unwrap_or(0)` is that fallback, over
                // the same `checked_div` that keeps this off a manual
                // `total > 0` check clippy flags.
                let percent = counts
                    .done
                    .saturating_mul(100)
                    .checked_div(counts.total)
                    .unwrap_or(0);
                format!("{} {} %", locale::t(lang, Key::TrayEmbedding), percent)
            }
            Phase::Removing { .. } => locale::t(lang, Key::TrayRemoving).to_string(),
            // Neither the person's own job (Reading/Embedding/Removing draw
            // their own sentence above) — the slot is taken by a probe or a
            // model adoption, neither of which is shown, so this reads the
            // same as Idle/Ended: what the index held as of the last count.
            Phase::Other { .. } => scanned_label(lang, state.files),
        },
        ScanSnapshot::Idle | ScanSnapshot::Ended { .. } => scanned_label(lang, state.files),
    }
}

/// «Проскановано: N файлів» / "Scanned: N files" — the sentence three of
/// [`status_label`]'s five branches share (Idle, Ended, and the running-but-
/// unshown `Other` phase), factored out once rather than written three times
/// so the three cannot drift apart from each other.
fn scanned_label(lang: Lang, files: i64) -> String {
    format!(
        "{} {} {}",
        locale::t(lang, Key::TrayScanned),
        files,
        locale::files_word(lang, files)
    )
}

/// Whether the tray's «Зупинити сканування» should be clickable, read straight
/// off the snapshot rather than from a boolean remembered across an
/// announcement — [`crate::state::JobObserver`]'s own doc has the defect a
/// remembered edge caused. `true` in exactly one shape: a job is running AND
/// that job said, when it claimed the slot, that it can be interrupted.
pub fn stop_enabled(state: &crate::scan_state::ScanState) -> bool {
    matches!(
        state.snapshot,
        crate::scan_state::ScanSnapshot::Running {
            cancellable: true,
            ..
        }
    )
}

/// Which entry point the tray's «Продовжити сканування» would start with, or
/// `None` when there is nothing to carry on from — F4 (Task 10 live run): a
/// person who pressed Stop had to open the settings window to find the button
/// that resumes, and the tray, the surface Stop was pressed on, offered
/// nothing.
///
/// The fact is the ENDED REPORT'S OWN `resume` and nothing else, which is the
/// same fact the settings strip's «Продовжити» is drawn from
/// ([`crate::scan_job::resume_for`] is where the table lives). Deriving it a
/// second time here — "cancelled, so offer Full" — is how the tray and the
/// window come to disagree about what a press would do: `resume_for` answers
/// `EmbedOnly` for a stop inside the embedding pass and `None` for the endings
/// that have nothing left (a `Skipped` embedding, a completed scan), and none
/// of that is recoverable from the snapshot's shape alone.
///
/// Pure, like [`status_label`] and [`stop_enabled`] beside it: no `AppState`,
/// no clock, no index, so it is pinned against a literal `ScanState`.
pub fn resume_entry(state: &crate::scan_state::ScanState) -> Option<crate::scan_state::Entry> {
    match &state.snapshot {
        crate::scan_state::ScanSnapshot::Ended { report } => report.resume,
        _ => None,
    }
}

/// Whether the tray's «Продовжити сканування» should be clickable — read off
/// the snapshot on every redraw, never remembered across an announcement, for
/// the reason [`stop_enabled`]'s doc gives.
///
/// It is [`resume_entry`] asked as a yes/no rather than a second predicate:
/// the item is enabled exactly when a press would have an entry to start, so
/// there is no state in which the item is clickable and the click does nothing
/// on purpose. (A click can still find the state moved on — the snapshot the
/// menu was drawn from is at most one tick old — and
/// [`crate::scan_job::resume_scan`] is where that is handled.)
pub fn resume_enabled(state: &crate::scan_state::ScanState) -> bool {
    resume_entry(state).is_some()
}

/// Assembles the tray menu for a resolved language, the persisted choice
/// behind it, and the scan the moment this is called — §8, plus the «Мова»
/// submenu (§D129) that lets the user pin a language or return to Auto
/// (`lang_auto`/`lang_uk`/`lang_en`, checked to match `choice`). Like
/// `build_tray`, this needs the main thread on macOS (see `TRAY_ITEM_IDS`) and
/// so is exercised only by the live run, not a headless test; from Task 6, it
/// is also what a language change calls to relabel the live menu via
/// `set_menu`.
///
/// Hands back both live items alongside the menu, because whoever swaps this
/// menu in has to keep [`TrayItems`]'s slot pointing at the items that are
/// actually on screen — see [`swap_tray_menu`], which is the only caller that
/// should be doing either.
///
/// `scan` seeds BOTH items at construction, unlike the `false` this function
/// used to hardcode for Stop before Task 5: the status line has a sentence to
/// draw the moment the menu appears (nobody polls it before then), and Stop's
/// initial state is a fact about `scan` rather than something the caller reads
/// back out of `AppState` a second time — `status_label`/`stop_enabled` are
/// the pure functions that decide both, so this function and `refresh_tray`
/// can never compute them differently.
pub fn build_tray_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
    lang: Lang,
    choice: LocaleChoice,
    hotkey: &HotkeyState,
    scan: &crate::scan_state::ScanState,
) -> tauri::Result<(Menu<R>, TrayItems<R>)> {
    let status = MenuItem::with_id(app, "status", status_label(lang, scan), false, None::<&str>)?;
    let show_search = MenuItem::with_id(
        app,
        "show_search",
        tray_label(lang, "show_search", hotkey),
        true,
        None::<&str>,
    )?;
    let open_settings = MenuItem::with_id(
        app,
        "open_settings",
        tray_label(lang, "open_settings", hotkey),
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
        STOP_ID,
        tray_label(lang, STOP_ID, hotkey),
        stop_enabled(scan),
        None::<&str>,
    )?;
    // F4: «Продовжити сканування», seeded from `scan` like Stop beside it and
    // for the same reason — the menu has to be right the first time it is
    // opened, and nobody polls it before then.
    let resume = MenuItem::with_id(
        app,
        RESUME_ID,
        tray_label(lang, RESUME_ID, hotkey),
        resume_enabled(scan),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        tray_label(lang, "quit", hotkey),
        true,
        None::<&str>,
    )?;

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
            &resume,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    Ok((
        menu,
        TrayItems {
            status,
            stop,
            resume,
        },
    ))
}

/// The handles to the tray's live status and Stop items, as managed state
/// (behind a `Mutex`, since `app.manage` hands out a shared reference and both
/// items are replaced together on a language change).
///
/// It is here, and read out of here on every use, because the items on screen
/// are replaced whenever the language changes: [`build_tray_menu`] builds a
/// whole new menu and [`swap_tray_menu`] swaps it in. A caller that had
/// captured one `MenuItem` would go on addressing an item that is in no menu,
/// and the one a person can see would keep whatever state it was built with.
/// `MenuItem<R>` is `Send + Sync` (Tauri unsafe-impls both on the inner type,
/// `tauri-2.11.5/src/menu/mod.rs:90-91`), so holding it here is sound.
///
/// Replaces `StopItem`, which held only the one item — this task gave the
/// status line a sentence of its own, so a language change has two live items
/// to keep in step rather than one.
pub struct TrayItems<R: Runtime> {
    pub status: MenuItem<R>,
    pub stop: MenuItem<R>,
    /// F4: «Продовжити сканування». Held here for [`refresh_tray`] to
    /// re-enable, exactly like `stop` — an item whose enabled state is a fact
    /// about the current scan is an item something has to redraw.
    pub resume: MenuItem<R>,
}

/// Re-reads the scan and the locale from `AppState` and redraws both tray
/// items to match — the observer's job, run on the main thread (`.setup`
/// wires `state::AppState`'s job observer to call this via
/// `run_on_main_thread`, because `set_text`/`set_enabled` hop there
/// themselves and a caller already on it saves the round trip).
///
/// 🔴 **Reads [`state::AppState::scan_state`], and NEVER `with_index` /
/// anything that opens the index.** This runs on the main thread — the same
/// thread every window redraw and every menu click waits on — and
/// `with_index` blocks for as long as a job holds the connection: a folder
/// removal alone can hold it for on the order of twenty seconds
/// (`state.rs`'s own `with_index` doc). A `with_index` call reachable from
/// here would freeze the whole application for that long every time a scan
/// ticks. `the_main_thread_closure_never_touches_the_index` in `lib.rs` is the
/// (admittedly brittle) guard against it reappearing.
///
/// Does nothing when the state or the items are unmanaged, which is every
/// headless test and every moment before `.setup` reaches the tray — the
/// same best-effort shape [`swap_tray_menu`] uses.
///
/// [`state::AppState::scan_state`]: crate::state::AppState::scan_state
pub fn refresh_tray<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let Some(items) = app.try_state::<std::sync::Mutex<TrayItems<R>>>() else {
        return;
    };
    let scan = state.scan_state();
    let lang = state.locale().effective;
    let guard = items
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = guard.status.set_text(status_label(lang, &scan));
    let _ = guard.stop.set_enabled(stop_enabled(&scan));
    let _ = guard.resume.set_enabled(resume_enabled(&scan));
}

/// Rebuilds the tray menu in `lang` and puts it on the live tray, keeping
/// [`TrayItems`]'s slot in step.
///
/// Every caller that relabels the tray goes through here — `locale::apply_locale`
/// on a language change, `lib.rs`'s handler when a change failed to persist and
/// the checkmark has to be put back, and from Task 11a `prefs::set_hotkey`,
/// whose label change is the shortcut hint rather than the language. One
/// predicate rather than three, because the half that is easy to forget is not
/// the `set_menu`: it is that the old items have just left the menu, and
/// anything still holding one of them is now talking to nothing.
///
/// The hotkey AND the scan are both read from `AppState` here rather than
/// passed in, for the same reason the locale is read inside `apply_locale`:
/// the caller that has just changed one of them and the caller that has not
/// must produce the same menu, and a parameter is one more thing a caller can
/// hand in stale. The read happens AFTER the tray lookup, so a headless test —
/// which has no tray — returns before touching state at all.
///
/// Best-effort throughout (`let _ =`), like everything else on the language
/// path: this runs from a tray callback that has no error channel of its own
/// (§6). Does nothing when there is no tray, which is every headless test.
pub fn swap_tray_menu<R: Runtime>(app: &tauri::AppHandle<R>, lang: Lang, choice: LocaleChoice) {
    let Some(tray) = app.tray_by_id("mnema-tray") else {
        return;
    };
    // `try_state`, like `refresh_tray` above and for its reason: `state`
    // panics on an unmanaged type, and the sentence above promises
    // best-effort throughout (review round 1, Minor 3). Nothing reachable gets
    // here without the state — `manage_state` runs at `lib.rs:524`, long before
    // any tray exists for `tray_by_id` to find — so this is the claim being
    // made true rather than a failure being handled.
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let hotkey = state.hotkey();
    let scan = state.scan_state();
    let Ok((menu, items)) = build_tray_menu(app, lang, choice, &hotkey, &scan) else {
        return;
    };
    if let Some(slot) = app.try_state::<std::sync::Mutex<TrayItems<R>>>() {
        *slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = items;
    }
    let _ = tray.set_menu(Some(menu));
}

/// Builds the tray icon and its menu. Reads `AppState`'s locale once — set by
/// `manage_state`, which `lib.rs`'s `setup` hook runs first (`lib.rs:524`),
/// so the state is already managed here.
///
/// It reads the hotkey the same way and in the same breath, and the boot order
/// is what makes that a fact rather than a hope: `.setup` runs
/// `prefs::install_hotkey` BEFORE this call, so the first menu this application
/// ever draws already names the shortcut the operating system answered about —
/// including an `Unavailable` one, which is named by no hint at all.
///
/// Menu-event handling (`show_search` → `crate::focus_launcher`,
/// `open_settings`, `quit`, and the language items) is wired at the app level
/// instead of here (`lib.rs`'s `on_menu_event`, Task 6): an event closure
/// bound to the tray itself would need re-attaching on every `set_menu`,
/// which is how a language change re-labels this same menu. Until Task 6
/// lands, the built menu is inert — clicking any item does nothing.
///
/// Manages [`TrayItems`] itself, behind a `Mutex`, rather than handing the
/// items back to `.setup` the way this function handed back the bare Stop
/// item before Task 5 — `.setup` has nothing left to do with either item once
/// they exist, since `boot_files`/`set_files` already ran (before this call)
/// to make `scan` a fact worth drawing rather than a fresh `ScanState::
/// default()`.
pub fn build_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let state = app.state::<crate::state::AppState>();
    let locale_state = state.locale();
    let hotkey = state.hotkey();
    let scan = state.scan_state();
    let (menu, items) = build_tray_menu(
        app,
        locale_state.effective,
        locale_state.choice,
        &hotkey,
        &scan,
    )?;

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
    app.manage(std::sync::Mutex::new(items));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Platform;
    use crate::shortcut::format_shortcut;

    /// The state the operating system is holding `shortcut`, which is the only
    /// state in which the label carries a hint at all.
    fn registered(shortcut: &str) -> HotkeyState {
        HotkeyState {
            shortcut: shortcut.to_string(),
            status: HotkeyStatus::Registered,
        }
    }

    /// The other state D128 made real: the shortcut is stored and the operating
    /// system refused it, so nothing on the keyboard opens the launcher.
    fn unavailable(shortcut: &str) -> HotkeyState {
        HotkeyState {
            shortcut: shortcut.to_string(),
            status: HotkeyStatus::Unavailable {
                reason: "taken by another application".into(),
            },
        }
    }

    #[test]
    fn tray_labels_compose_emoji_and_translation() {
        assert_eq!(
            tray_label(
                crate::locale::Lang::En,
                "show_search",
                &registered(crate::prefs::DEFAULT_HOTKEY)
            ),
            format!(
                "🔍 Show search ({})",
                format_shortcut("Alt+Space", Platform::of_this_build())
            )
        );
        assert_eq!(
            tray_label(crate::locale::Lang::Uk, "quit", &registered("Alt+Space")),
            "⏻ Вийти"
        );
        assert_eq!(
            tray_label(
                crate::locale::Lang::En,
                "open_settings",
                &registered("Alt+Space")
            ),
            "⚙ Open settings"
        );
    }

    /// 🔴 The defect this task exists for, as a pair of states rather than one
    /// value: the SAME language, two different registered shortcuts, two
    /// different labels. The literal `(⌥Space)` this replaced satisfies the
    /// first of these and nothing else — which is exactly what a person saw
    /// after changing the shortcut to ⌃⌥Space and restarting (measured
    /// 2026-09-04).
    ///
    /// Written through `format_shortcut(.., Platform::of_this_build())` rather
    /// than against a glyph, so it asserts the same thing on the CI's Linux and
    /// on a mac. The platform-pinned forms are `shortcut::tests`' own; the one
    /// mac literal this file still pins is the row below.
    #[test]
    fn the_search_hint_follows_the_registered_shortcut() {
        let default = tray_label(Lang::Uk, "show_search", &registered("Alt+Space"));
        let changed = tray_label(Lang::Uk, "show_search", &registered("Ctrl+Alt+Space"));
        assert_eq!(
            changed,
            format!(
                "🔍 {} ({})",
                locale::t(Lang::Uk, Key::TrayShowSearch),
                format_shortcut("Ctrl+Alt+Space", Platform::of_this_build())
            )
        );
        // Both directions, and this is the assertion the literal died on: the
        // label for one shortcut is not the label for another.
        assert_ne!(changed, default);
    }

    /// The form the literal used to be, now derived — pinned to mac because
    /// that is the platform the literal was written on and the one whose
    /// rendering a reader of this file recognises. On the other two builds the
    /// same call answers `Alt+Space`, which `shortcut::tests` pins.
    #[test]
    #[cfg(target_os = "macos")]
    fn on_a_mac_the_default_shortcut_still_reads_exactly_as_it_used_to() {
        assert_eq!(
            tray_label(
                Lang::Uk,
                "show_search",
                &registered(crate::prefs::DEFAULT_HOTKEY)
            ),
            "🔍 Показати пошук (⌥Space)"
        );
    }

    /// An `Unavailable` shortcut is not a shortcut a person can press, so the
    /// label promises none. Positive first — the item still says what it does
    /// and in the right language — then the absence, because a label that had
    /// lost its text entirely would also contain no parenthesis.
    #[test]
    fn an_unregistered_shortcut_is_named_by_no_hint_at_all() {
        let label = tray_label(Lang::Uk, "show_search", &unavailable("Ctrl+Alt+Space"));
        assert_eq!(
            label,
            format!("🔍 {}", locale::t(Lang::Uk, Key::TrayShowSearch))
        );
        assert!(
            !label.contains('('),
            "an unusable shortcut was drawn: {label}"
        );
        // Both directions: the very same shortcut, registered, does get one —
        // so the missing parenthesis is about the status and not about the
        // string beside it.
        assert!(
            tray_label(Lang::Uk, "show_search", &registered("Ctrl+Alt+Space")).contains('('),
            "a registered shortcut lost its hint"
        );
    }

    /// Both languages, because the glyph is composed once at this call site and
    /// the text comes from the catalog: a label that lost either half would
    /// still be non-empty, and `every_tray_id_has_a_non_empty_label...` would
    /// go on passing.
    #[test]
    fn the_stop_item_composes_its_glyph_with_both_translations() {
        assert_eq!(
            tray_label(crate::locale::Lang::Uk, STOP_ID, &registered("Alt+Space")),
            "⏹ Зупинити сканування"
        );
        assert_eq!(
            tray_label(crate::locale::Lang::En, STOP_ID, &registered("Alt+Space")),
            "⏹ Stop scanning"
        );
    }

    /// Both languages, for the reason the Stop item's twin above gives: the
    /// glyph is composed at the call site and the words come from the catalog,
    /// so a label that lost either half would still be non-empty and
    /// `every_tray_id_has_a_non_empty_label...` would go on passing.
    #[test]
    fn the_resume_item_composes_its_glyph_with_both_translations() {
        assert_eq!(
            tray_label(crate::locale::Lang::Uk, RESUME_ID, &registered("Alt+Space")),
            "▶ Продовжити сканування"
        );
        assert_eq!(
            tray_label(crate::locale::Lang::En, RESUME_ID, &registered("Alt+Space")),
            "▶ Continue scanning"
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
                // F4: «Продовжити сканування» sits directly under Stop — the
                // item a person reaches for after pressing the one above it.
                "resume",
                "quit"
            ],
            "the tray menu drifted from spec §8"
        );
    }

    /// Every id `tray_label` still answers for — every `TRAY_ITEM_IDS` entry
    /// EXCEPT `"status"`, which Task 5 moved to `status_label` and which
    /// `tray_label_rejects_the_status_item_too` below pins as a panic instead.
    /// Were this to iterate `TRAY_ITEM_IDS` unfiltered, as it did before Task
    /// 5, it would panic on `"status"` and never reach the other four ids.
    #[test]
    fn every_tray_id_has_a_non_empty_label_in_both_languages() {
        // lang_auto/lang_uk/lang_en are not `tray_label` ids — they come from
        // `locale::t`/`locale::endonym` directly in `build_tray_menu` — and
        // are covered by locale.rs's own `every_key_has_both_languages...`.
        for &id in TRAY_ITEM_IDS.iter().filter(|&&id| id != "status") {
            for state in [registered("Alt+Space"), unavailable("Alt+Space")] {
                assert!(
                    !tray_label(Lang::Uk, id, &state).is_empty(),
                    "UK missing for {id}"
                );
                assert!(
                    !tray_label(Lang::En, id, &state).is_empty(),
                    "EN missing for {id}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "unknown tray id")]
    fn tray_label_rejects_an_unknown_id() {
        tray_label(Lang::En, "not_a_real_id", &registered("Alt+Space"));
    }

    /// `"status"` stays in `TRAY_ITEM_IDS` — the item still exists in the
    /// menu — but Task 5 gave it a stateful sentence (`status_label`) that
    /// `tray_label` has no `ScanState` to compose, so it now falls to the same
    /// `other => panic!` arm as an id nobody ever defined. Same shape as
    /// `tray_label_rejects_the_deleted_update_check` just below, and for the
    /// same reason: a `tray_label` that quietly answered `"status"` again
    /// would let a caller draw a fixed sentence for it with nothing
    /// complaining, which is the exact defect this task exists to remove.
    #[test]
    #[should_panic(expected = "unknown tray id")]
    fn tray_label_rejects_the_status_item_too() {
        tray_label(Lang::En, "status", &registered("Alt+Space"));
    }

    /// The amended §8 dropped «Перевірити оновлення», and dropping an item is
    /// not the same as leaving its label behind: a `tray_label` that still
    /// answered for `check_updates` would let a rebuilt menu carry the item
    /// back with nothing complaining. Same shape as the unknown-id case above,
    /// because after the amendment that is exactly what this id is.
    #[test]
    #[should_panic(expected = "unknown tray id")]
    fn tray_label_rejects_the_deleted_update_check() {
        tray_label(Lang::En, "check_updates", &registered("Alt+Space"));
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

    // ── `status_label` / `stop_enabled` (Task 5) ──────────────────────────

    use crate::job::Progress;
    use crate::scan_state::{Entry, OtherJob, Phase, ScanReport, ScanSnapshot, ScanState};

    fn counts(done: u64, total: u64) -> Progress {
        Progress {
            done,
            total,
            ..Progress::default()
        }
    }

    fn idle(files: i64) -> ScanState {
        ScanState {
            files,
            ..ScanState::default()
        }
    }

    fn reading(done: u64, total: u64) -> ScanState {
        ScanState {
            snapshot: ScanSnapshot::Running {
                phase: Phase::Reading {
                    root_index: 0,
                    root_count: 1,
                    root_path: "/nonexistent/mnema-status-label-root".to_string(),
                    counts: counts(done, total),
                },
                cancellable: true,
            },
            ..ScanState::default()
        }
    }

    fn embedding(done: u64, total: u64) -> ScanState {
        ScanState {
            snapshot: ScanSnapshot::Running {
                phase: Phase::Embedding {
                    counts: counts(done, total),
                },
                cancellable: true,
            },
            ..ScanState::default()
        }
    }

    fn removing() -> ScanState {
        ScanState {
            snapshot: ScanSnapshot::Running {
                phase: Phase::Removing {
                    root_path: "/nonexistent/mnema-status-label-root".to_string(),
                },
                cancellable: false,
            },
            ..ScanState::default()
        }
    }

    fn other(job: OtherJob, cancellable: bool) -> ScanState {
        ScanState {
            snapshot: ScanSnapshot::Running {
                phase: Phase::Other { job },
                cancellable,
            },
            ..ScanState::default()
        }
    }

    fn ended(files: i64) -> ScanState {
        ScanState {
            files,
            snapshot: ScanSnapshot::Ended {
                report: ScanReport::default(),
            },
            ..ScanState::default()
        }
    }

    /// `Idle` — the state a fresh process starts in, and the state
    /// `boot_files`'s seed leaves it in until the first job claims the slot.
    /// Both languages and both directions: a Ukrainian sentence and an
    /// English one are different STRINGS, not the same value asserted twice,
    /// so `assert_ne!` is the second half of the pair rather than decoration.
    ///
    /// 🔴 **1234 takes «файли», not «файлів».** The brief names 1234 as this
    /// state's file count, and Ukrainian numeral agreement goes off the
    /// number's LAST TWO DIGITS, not its magnitude: 1234 ends in "34", the
    /// same last-two-digits shape as the brief's own 22 → «файли» — neither
    /// is in the 11–14 band, so both take the 2–4 plural.
    /// `ukrainian_file_count_takes_the_slavic_plural` in `locale.rs` is the
    /// same rule pinned on the brief's own 1/2/5/11/21/22/25/111 table, which
    /// this state's word has to agree with rather than restate a different
    /// rule for one more number — a `status_label` that special-cased 1234 to
    /// answer «файлів» would go red against that table's 22, not against this
    /// test.
    #[test]
    fn idle_draws_the_scanned_count_in_both_languages() {
        let uk = status_label(Lang::Uk, &idle(1234));
        let en = status_label(Lang::En, &idle(1234));
        assert_eq!(uk, "Проскановано: 1234 файли");
        assert_eq!(en, "Scanned: 1234 files");
        assert_ne!(uk, en);
    }

    /// The Ukrainian plural riding on the same sentence `idle_draws_...`
    /// pins, over the table `locale::ukrainian_file_count_takes_the_slavic_
    /// plural` already proves the RULE on — this test is that
    /// `status_label` actually calls `files_word` rather than hardcoding one
    /// form, which the single-value test above cannot tell apart from a
    /// `files_word` that always answered «файлів».
    #[test]
    fn idle_file_count_carries_the_plural_through_to_the_sentence() {
        for (files, word) in [
            (1, "файл"),
            (3, "файли"),
            (21, "файл"),
            (22, "файли"),
            (111, "файлів"),
        ] {
            assert_eq!(
                status_label(Lang::Uk, &idle(files)),
                format!("Проскановано: {files} {word}"),
                "files = {files}"
            );
        }
    }

    /// `Reading` with a known `total`: a percentage, not a count — the pair
    /// this separates from `reading_with_no_known_total_draws_a_count` below,
    /// which is the same phase with the one field that decides which
    /// sentence appears.
    #[test]
    fn reading_with_a_known_total_draws_a_percentage() {
        assert_eq!(status_label(Lang::Uk, &reading(48, 200)), "Індексація 24 %");
        assert_eq!(
            status_label(Lang::Uk, &reading(1, 3)),
            "Індексація 33 %",
            "integer division must floor, not round"
        );
        // Minor 4 (Task 10a review, round 1): the renamed variant's ENGLISH
        // arm, pinned here rather than only in Ukrainian — a rename that
        // fixed one language and left the other on the old literal would
        // still pass every test above.
        assert_eq!(status_label(Lang::En, &reading(48, 200)), "Indexing 24 %");
    }

    /// `done == total` reaches exactly 100, never more and never NaN — the
    /// division this guards is `done * 100 / total`, and this is the one
    /// input where a caller who wrote `(total - done)` or an off-by-one
    /// would visibly miss 100.
    #[test]
    fn reading_finished_reads_exactly_one_hundred_percent() {
        assert_eq!(status_label(Lang::Uk, &reading(7, 7)), "Індексація 100 %");
        assert_eq!(status_label(Lang::En, &reading(7, 7)), "Indexing 100 %");
    }

    /// `total == 0` — nothing has answered "how many folders" yet, which is
    /// the ordinary shape at the very start of a pass — draws a COUNT
    /// instead of attempting `done / 0`. Paired against
    /// `reading_with_a_known_total_draws_a_percentage`: same phase, the field
    /// that decides which sentence is `total`, and this is the value that
    /// would panic a naive `done * 100 / total`.
    #[test]
    fn reading_with_no_known_total_draws_a_count() {
        assert_eq!(
            status_label(Lang::Uk, &reading(120, 0)),
            "Індексація: 120 файлів"
        );
        assert_eq!(
            status_label(Lang::En, &reading(120, 0)),
            "Indexing: 120 files"
        );
    }

    /// `Embedding`, both branches of the same `total == 0` question the
    /// reading phase asks — a known total draws a percentage, and an unknown
    /// one draws `0 %` rather than a count (unlike Reading): embedding has no
    /// file-count sentence of its own, so `0 %` is what a phase that has not
    /// yet been told its total draws instead of dividing by zero.
    #[test]
    fn embedding_draws_a_percentage_or_zero_with_no_known_total() {
        assert_eq!(
            status_label(Lang::Uk, &embedding(30, 1000)),
            "Вбудовування 3 %"
        );
        assert_eq!(status_label(Lang::Uk, &embedding(0, 0)), "Вбудовування 0 %");
    }

    /// `Removing` — a fixed sentence with no number in it at all, unlike
    /// every other running phase, because there is nothing per-file to
    /// count during a folder's removal.
    #[test]
    fn removing_draws_a_fixed_sentence() {
        assert_eq!(status_label(Lang::Uk, &removing()), "Видаляємо теку…");
        assert_eq!(status_label(Lang::En, &removing()), "Removing folder…");
    }

    /// `Other` (a probe or a model adoption) and `Ended` both draw the same
    /// "Scanned: N" sentence `Idle` does — the person did not ask for either
    /// job and is shown nothing about it, so the tray reads exactly as it
    /// would if the slot were empty and the index just held `files` from
    /// whatever last counted them.
    #[test]
    fn other_jobs_and_ended_scans_draw_the_scanned_count_same_as_idle() {
        let want = "Проскановано: 7 файлів";
        let probe_with_files = ScanState {
            files: 7,
            ..other(OtherJob::Probe, true)
        };
        assert_eq!(status_label(Lang::Uk, &probe_with_files), want);
        assert_eq!(
            status_label(Lang::Uk, &other(OtherJob::ModelAdoption, false)),
            "Проскановано: 0 файлів",
            "Other carries no `files` field of its own — it draws ScanState::files, 0 here"
        );
        assert_eq!(status_label(Lang::Uk, &ended(7)), want);
    }

    /// An `Ended` snapshot whose report names `resume` — the one shape the
    /// tray's «Продовжити сканування» is allowed to be clickable in.
    fn ended_resuming(resume: Option<Entry>) -> ScanState {
        ScanState {
            snapshot: ScanSnapshot::Ended {
                report: ScanReport {
                    resume,
                    ..ScanReport::default()
                },
            },
            ..ScanState::default()
        }
    }

    /// `resume_enabled` over every shape the slot can be in, as one table for
    /// the reason `stop_is_enabled_only_while_a_cancellable_job_is_running`
    /// gives: a variant added to `ScanSnapshot` without a row here would
    /// silently narrow what this test claims to cover.
    ///
    /// The state pair this exists for is the last two rows — two `Ended`
    /// snapshots that differ in nothing but `report.resume`. `true` on the one
    /// that carries an entry, `false` on the one that does not, so an
    /// implementation that answered `matches!(snapshot, Ended { .. })` (the
    /// obvious wrong one: every ending would offer a button) goes red on the
    /// `None` row, and one that answered a constant `false` goes red on the
    /// `Some` rows.
    #[test]
    fn resume_is_enabled_only_when_the_ended_report_carries_its_own_resume() {
        let rows: [(ScanState, bool); 8] = [
            (idle(0), false),
            (reading(0, 1), false),
            (embedding(0, 1), false),
            (removing(), false),
            (other(OtherJob::ModelAdoption, false), false),
            (other(OtherJob::Probe, true), false),
            (ended_resuming(None), false),
            (ended_resuming(Some(Entry::EmbedOnly)), true),
        ];
        for (state, want) in &rows {
            assert_eq!(
                resume_enabled(state),
                *want,
                "state = {state:?}, want resume_enabled = {want}"
            );
        }
    }

    /// `resume_enabled` is a boolean and the click needs the ENTRY, so the
    /// entry is what the tray reads and the boolean is derived from it — this
    /// pins that the entry handed on is the report's own and not a fixed one.
    /// Both variants, because a `resume_entry` hardcoded to `Some(Entry::Full)`
    /// would make every `resume_enabled` row above pass while a person who
    /// stopped the embedding pass got their whole archive re-read.
    #[test]
    fn resume_entry_is_the_reports_own_entry_and_not_a_fixed_one() {
        assert_eq!(
            resume_entry(&ended_resuming(Some(Entry::Full))),
            Some(Entry::Full)
        );
        assert_eq!(
            resume_entry(&ended_resuming(Some(Entry::EmbedOnly))),
            Some(Entry::EmbedOnly)
        );
        assert_eq!(resume_entry(&ended_resuming(None)), None);
        assert_eq!(resume_entry(&reading(0, 1)), None);
    }

    /// `stop_enabled` over every shape the slot can be in, asserted as one
    /// table rather than six separate calls — a row added to `ScanSnapshot`
    /// without a row added here would silently narrow what this test claims
    /// to cover. `true` in exactly the one shape the doc comment names:
    /// running AND cancellable; every other row, including a cancellable
    /// `Ended`/`Idle` (there is no such thing — cancellable lives only inside
    /// `Running`), is `false`.
    #[test]
    fn stop_is_enabled_only_while_a_cancellable_job_is_running() {
        let rows: [(ScanState, bool); 6] = [
            (idle(0), false),
            (reading(0, 1), true),
            (removing(), false),
            (other(OtherJob::ModelAdoption, false), false),
            (other(OtherJob::Probe, true), true),
            (ended(0), false),
        ];
        for (state, want) in &rows {
            assert_eq!(
                stop_enabled(state),
                *want,
                "state = {state:?}, want stop_enabled = {want}"
            );
        }
    }
}
