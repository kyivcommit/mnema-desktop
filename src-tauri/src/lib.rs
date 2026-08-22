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
pub mod models;
pub mod paths;
pub mod state;
pub mod tray;
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

/// Builds and runs the application. Returns only when the last window closes or
/// start-up fails.
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // §6: the tray is the only way to quit. A window close hides the
                // window and keeps it alive, so a hidden webview keeps its DOM —
                // an unsaved query or a result set survives dismissal (§7.3,
                // "what disappears"). Real exit is `app.exit(0)` from the tray's
                // Quit, which is not a window close and so is not prevented here.
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(|app| {
            manage_state(app.handle())?;
            tray::build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .context("running the application")
}
