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
pub mod walk_job;

use anyhow::Context as _;
use tauri::Manager as _;

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

/// Builds and runs the application. Returns only when the last window closes or
/// start-up fails.
pub fn run() -> anyhow::Result<()> {
    // Process-global, and it must precede every connection: a connection opened
    // before registration never sees the extension and only fails much later, at
    // the first vector statement. Registering with no vector table costs
    // nothing, which is what makes it safe to do unconditionally. G7.0 §5.7.
    mnema_index::register_vector_extension().context("registering the sqlite-vec extension")?;

    tauri::Builder::default()
        // Registered before anything else, as the plugin requires. Two instances
        // over one SQLite file is a second writer that can only wait, an
        // indexing job running twice over the same folder, and the cloud spend
        // for it billed twice.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // The second process exits as soon as it has handed its arguments
            // over. All that is left to do here is show the window the user was
            // asking for.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Native folder picking, gated by `dialog:allow-open` in
        // `capabilities/default.json` rather than the text field
        // `ui/index.html` used before this: a path typed by hand was enough
        // to exercise `add_watched_folder`, but not something the interface
        // spec would keep.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            manage_state(app.handle())?;
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .context("running the application")
}
