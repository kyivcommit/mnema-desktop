//! The Tauri shell: a translation layer between the core crates and a webview.
//!
//! It is a library with a two-line binary in front of it rather than a binary
//! alone, because an integration test cannot reach into a bin-only crate, and
//! this is exactly the layer where a test has to call the commands the way the
//! webview calls them.

pub mod bridge;
pub mod embed_job;
pub mod error;
pub mod job;
pub mod locale;
pub mod models;
pub mod paths;
pub mod state;
pub mod tray;
mod tree;
pub mod walk_job;

use anyhow::Context as _;
use tauri::Manager as _;
use tauri_plugin_positioner::{Position, WindowExt as _};

/// Everything the webview is allowed to call, in one place.
///
/// Exposed rather than written inline in [`run`] so that a test drives the same
/// list the application registers. A test that builds its own handler proves the
/// commands work and nothing about whether they are reachable.
pub fn invoke_handler<R: tauri::Runtime>()
-> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        bridge::open_index,
        bridge::add_watched_folder,
        bridge::remove_watched_folder,
        bridge::search,
        bridge::ask,
        bridge::set_search_arms,
        bridge::skips,
        bridge::start_probe_job,
        bridge::cancel_job,
        bridge::job_status,
        tree::list_tree,
        models::provider_models,
        models::key_present,
        models::set_key,
        models::forget_key,
        models::set_embedding_model,
        models::set_rerank_model,
        models::set_chat_model,
        models::model_settings,
        walk_job::start_walk_job,
        embed_job::start_embed_job,
        locale::get_locale,
        locale::set_locale,
    ]
}

/// Decides where the index lives and puts the state into the application.
///
/// Exposed for the same reason as [`invoke_handler`], and the reason is the same
/// mistake: a test that constructs its own `AppState` proves the commands work
/// against a directory, and nothing about which directory the application picks.
/// This is the only place that choice is made.
///
/// Two of the four arguments are named here and nowhere else, for the same
/// reason: the real provider address and the production credential reference.
/// A test builds its own `AppState` pointed at a local server and at a
/// credential reference of its own, which is what keeps it out of the
/// developer's own keychain — see [`state::AppState`]'s fields.
///
/// LOCAL data, not roaming and not cache — see [`paths::index_path`] for why.
/// Getting it wrong is silent: it works on the machine that wrote it and loses a
/// user's index on theirs.
///
/// `paths::worker_path`'s own doc comment has what is and is not settled about
/// the second path this resolves. `?` rather than a fallback: a
/// `current_exe()` that fails is rare enough, and quiet enough if papered
/// over, that surfacing it at start-up beats discovering it the first time a
/// walk job's `Pool` cannot find its worker.
pub fn manage_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let dir = app.path().app_local_data_dir()?;
    let worker = paths::worker_path()?;
    app.manage(state::AppState::new(
        dir,
        worker,
        mnema_provider::OPENROUTER_BASE.to_string(),
        models::CREDENTIAL_REF.to_string(),
    ));
    Ok(())
}

/// Shows the launcher and focuses it, returning whether the launcher window was
/// there to act on. The single-instance callback and the tray's "show search"
/// item share this. §6: the launcher *hides*, so it is *shown* — not
/// unminimized, which never re-opens a hidden window. A test drives this against
/// the mock runtime, where the real window manager is absent, which is why the
/// return value is the found-ness of the window and not its resulting focus.
pub fn focus_launcher<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    match app.get_webview_window("launcher") {
        Some(window) => {
            let _ = window.show();
            // §6: put the launcher where the menu-bar item is, next to the tray,
            // before focusing it. `move_window` no-ops where the tray position is
            // unknown, so this is safe against the mock runtime and against
            // platforms that never record one; exact placement is tuned in PR 10.
            let _ = window.move_window(Position::TrayCenter);
            let _ = window.set_focus();
            true
        }
        None => false,
    }
}

/// The global shortcut's action: hide the launcher if it is up, otherwise show
/// and focus it. The visibility branch is exercised by the live run — the mock
/// runtime does not track a real window's visibility — so the CI seam is
/// `focus_launcher`, not this.
pub fn toggle_launcher<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("launcher") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            // Same as `focus_launcher`'s show path: position at the tray before
            // focusing (§6). No-ops where the tray position is unknown.
            let _ = window.move_window(Position::TrayCenter);
            let _ = window.set_focus();
        }
    }
}

/// Keeps the macOS activation policy in step with the settings window: the app
/// is an `Accessory` (no Dock icon, no menu bar — a menu-bar resident) while
/// only the launcher and tray are up, and becomes `Regular` (Dock icon + the
/// standard menu bar) while the settings window is visible. §6/§8: the standard
/// menu belongs to the settings window, not the launcher. A no-op off macOS,
/// where the method does not exist; OS-level, so it is verified by the live run,
/// not a headless test.
pub fn sync_activation_policy<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let settings_visible = app
            .get_webview_window("settings")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        let policy = if settings_visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The app menu's ⌘Q item id: it closes the settings window instead of quitting.
const CMD_Q_CLOSE_SETTINGS: &str = "cmd_q_close_settings";

/// The application menu on macOS — the standard items MINUS the native Quit.
/// `PredefinedMenuItem::quit` maps to AppKit `terminate:`, which cannot be vetoed
/// (tao has no `applicationShouldTerminate`), so it would quit the app past the
/// ExitRequested guard and past §6 — even from a hidden menu bar, since a menu
/// key-equivalent stays live. In its place ⌘Q is a custom item that hides the
/// settings window; the app is quit ONLY from the tray's «Вийти» (§6). The menu
/// bar is shown only while settings is visible (`sync_activation_policy`).
#[cfg(target_os = "macos")]
pub(crate) fn build_app_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    lang: crate::locale::Lang,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use crate::locale::{self, Key};
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

    // `lang` is passed in, never resolved here: the first build runs during
    // `build()`, before the path resolver exists, so calling `app.path()` /
    // `resolve_effective` inside this function panics ("state() called before
    // manage()"). Callers pass the language — `boot_lang()` (OS-only) at the
    // first build, the resolved effective in `.setup` and `apply_locale`.
    let pkg = app.package_info();
    let about = AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        ..Default::default()
    };

    // ⌘Q → close the settings window, never quit. Custom (not the predefined
    // Quit), so it runs our handler instead of `terminate:`.
    let close_settings = MenuItem::with_id(
        app,
        CMD_Q_CLOSE_SETTINGS,
        locale::t(lang, Key::CloseSettings),
        true,
        Some("CmdOrCtrl+Q"),
    )?;

    let app_menu = Submenu::with_items(
        app,
        pkg.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &close_settings,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        locale::t(lang, Key::MenuEdit),
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        locale::t(lang, Key::MenuWindow),
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])
}

/// Off macOS the ⌘Q → `terminate:` problem does not arise; keep the default menu
/// until the cross-platform pass (PR 10).
#[cfg(not(target_os = "macos"))]
pub(crate) fn build_app_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    _lang: crate::locale::Lang,
) -> tauri::Result<tauri::menu::Menu<R>> {
    tauri::menu::Menu::default(app)
}

/// Builds and runs the application. Returns only when the tray's «Вийти» calls
/// `app.exit(0)`, or start-up fails (§6: window closes and ⌘Q hide, never quit —
/// the tray is the only exit).
pub fn run() -> anyhow::Result<()> {
    // Process-global, and it must precede every connection: a connection opened
    // before registration never sees the extension and only fails much later, at
    // the first vector statement. Registering with no vector table costs
    // nothing, which is what makes it safe to do unconditionally. G7.0 §5.7.
    mnema_index::register_vector_extension().context("registering the sqlite-vec extension")?;

    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

    let alt_space = Shortcut::new(Some(Modifiers::ALT), Code::Space);
    let global_shortcut = tauri_plugin_global_shortcut::Builder::new()
        .with_shortcut(alt_space)
        .context("registering the ⌥Space shortcut")?
        .with_handler(|app, _shortcut, event| {
            // Only one shortcut is registered, so no need to match it; act on
            // the press edge, not the release.
            if event.state() == ShortcutState::Pressed {
                toggle_launcher(app);
            }
        })
        .build();

    tauri::Builder::default()
        // Registered before anything else, as the plugin requires. Two instances
        // over one SQLite file is a second writer that can only wait, an
        // indexing job running twice over the same folder, and the cloud spend
        // for it billed twice.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // The second process has handed its arguments over and will exit;
            // show the launcher the user asked for. §6: show, not unminimize —
            // the launcher is hidden, and unminimize does not re-open a hidden
            // window.
            focus_launcher(app);
        }))
        // Native folder picking, gated by `dialog:allow-open` in
        // `capabilities/default.json` rather than the text field
        // `ui/index.html` used before this: a path typed by hand was enough
        // to exercise `add_watched_folder`, but not something the interface
        // spec would keep.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(global_shortcut)
        .menu(|app| build_app_menu(app, crate::locale::boot_lang()))
        // All menu events — the app menu's ⌘Q AND every tray item — dispatch to
        // this one app-level handler. `muda` registers `Builder::on_menu_event`
        // and the tray's menu into the same app-level listeners, so events fire
        // here regardless of `set_menu`; a closure bound to the tray instead
        // would need re-attaching on every language change's `set_menu` (Task
        // 6). The tray builder therefore keeps only `on_tray_icon_event`.
        .on_menu_event(|app, event| match event.id().as_ref() {
            // §6: ⌘Q closes the settings window (hide, keep state) and never
            // quits the app; the tray's «Вийти» is the only quit.
            id if id == CMD_Q_CLOSE_SETTINGS => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.hide();
                }
                sync_activation_policy(app);
            }
            // §6: show, not unminimize — the launcher is hidden. The bool it
            // returns (window found) has no meaning off a live window manager.
            "show_search" => {
                focus_launcher(app);
            }
            // Moved here from the tray builder (Task 5): reveal and focus the
            // settings window, then let the resident become Regular (Dock icon
            // + menu bar) while it is up (§6/§8).
            "open_settings" => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                sync_activation_policy(app);
            }
            // §6: the tray's «Вийти» is the only real exit. `Some(0)` is what
            // the ExitRequested guard lets through.
            "quit" => app.exit(0),
            // §D129: pin a language or return to Auto. Both this callback and
            // the `set_locale` command go through `apply_choice` (persist →
            // update state → `apply_locale`), the one path. A tray callback has
            // no UI channel of its own (§6), so on a persist failure we log the
            // error and rebuild the tray menu from the UNCHANGED LocaleState:
            // macOS has already flipped the clicked CheckMenuItem, and because
            // `apply_choice` fails at the persist step before it touches state
            // (locale.rs `write_choice(...)?`), this returns the checkmark to the
            // still-current choice ("старий вибір лишається", spec §5.8). The
            // `set_locale` command returns the same error to its caller for PR 9's
            // in-UI channel.
            "lang_auto" | "lang_uk" | "lang_en" => {
                use crate::locale::LocaleChoice;
                let choice = match event.id().as_ref() {
                    "lang_uk" => LocaleChoice::Uk,
                    "lang_en" => LocaleChoice::En,
                    _ => LocaleChoice::Auto,
                };
                let state = app.state::<state::AppState>();
                if let Err(e) = crate::locale::apply_choice(app, &state, choice) {
                    eprintln!("mnema: language change failed to persist: {e}");
                    // Restore the checkmark: the OS toggled it on click, but the
                    // choice never changed, so rebuild from the current state.
                    let current = state.locale();
                    if let Some(tray) = app.tray_by_id("mnema-tray")
                        && let Ok(menu) =
                            crate::tray::build_tray_menu(app, current.effective, current.choice)
                    {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
            }
            _ => {}
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // §6: the tray is the only way to quit. A window close hides the
                // window and keeps it alive, so a hidden webview keeps its DOM —
                // an unsaved query or a result set survives dismissal (§7.3,
                // "what disappears"). Real exit is `app.exit(0)` from the tray's
                // Quit, which is not a window close and so is not prevented here.
                let _ = window.hide();
                // Hiding the settings window drops the resident back to
                // Accessory (no Dock icon / menu bar); hiding the launcher while
                // settings is still up leaves the policy unchanged. §6/§8.
                sync_activation_policy(window.app_handle());
                api.prevent_close();
            }
        })
        .setup(|app| {
            manage_state(app.handle())?;
            // §D129: resolve the interface language once at start-up (prefs → OS
            // → EN) and seed it into `AppState` BEFORE the tray is built, which
            // reads it back to label its menu (`tray::build_tray`).
            let st = locale::resolve_effective(app.handle());
            app.state::<state::AppState>().set_locale_state(st);
            // The first app menu was built during `build()` from the OS locale
            // alone (`boot_lang` — no path resolver yet to read prefs). Rebuild
            // it now from the resolved language so an explicit saved choice that
            // differs from the OS shows the moment the menu bar first appears.
            if let Ok(menu) = build_app_menu(app.handle(), st.effective) {
                let _ = app.handle().set_menu(menu);
            }
            tray::build_tray(app.handle())?;
            // The settings window's native title in the resolved language. It is
            // hidden at start-up, so this is what it shows the first time it is
            // opened; a later language change re-titles it via `apply_locale`.
            if let Some(w) = app.get_webview_window("settings") {
                let _ = w.set_title(&format!(
                    "Mnema — {}",
                    locale::t(st.effective, locale::Key::SettingsTitle)
                ));
            }
            // §6/§8: start as a menu-bar resident — no Dock icon, no menu bar
            // (settings is hidden at startup). The standard menu returns only
            // while the settings window is visible.
            sync_activation_policy(app.handle());
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())
        .context("building the application")?
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                // §6: the tray's Quit is the only exit. A window close or macOS
                // Cmd+Q fires ExitRequested with `code: None`; the tray's
                // `app.exit(0)` carries `Some(0)`. Prevent the former, allow the
                // latter — together with the CloseRequested→hide handler, the
                // resident can be quit only from the tray.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
    Ok(())
}
