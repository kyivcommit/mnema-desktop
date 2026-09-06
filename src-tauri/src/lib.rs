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
pub mod os_services;
pub mod paths;
pub mod prefs;
pub mod scan_job;
pub mod scan_state;
pub mod shortcut;
pub mod state;
pub mod tray;
mod tree;
pub mod walk_job;

use anyhow::Context as _;
use tauri::Emitter as _;
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
        bridge::list_exclusions,
        bridge::exclude_subfolder,
        bridge::include_subfolder,
        bridge::list_masks,
        bridge::add_mask,
        bridge::remove_mask,
        bridge::search,
        bridge::ask,
        bridge::set_search_arms,
        bridge::skips,
        bridge::start_probe_job,
        bridge::cancel_job,
        bridge::job_status,
        tree::list_subfolders,
        tree::list_tree,
        tree::mask_preview,
        tree::source_around,
        models::provider_models,
        models::key_present,
        models::set_key,
        models::forget_key,
        models::set_embedding_model,
        models::set_rerank_model,
        models::set_chat_model,
        models::model_settings,
        scan_job::start_scan_job,
        locale::get_locale,
        locale::set_locale,
        prefs::app_prefs,
        prefs::set_hotkey,
        prefs::set_autostart,
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

/// Opens the index once, at start-up — because until this existed, nothing in
/// the product ever did.
///
/// `open_index` had no caller outside this crate's tests: a command, its
/// registration in the invoke handler, and the state method behind it were the
/// whole of it, and no line under `ui/src/` so much as names the command — so
/// nothing ever invoked it. `AppState::db` stays `None` until it runs and
/// `with_index` refuses while it is, so the shipped application could not
/// answer a question and could not list a tree, and the settings screen read
/// `Unreadable` for as long as it stayed open. The suite was green because
/// every test opens the index for itself — the same shape as the start-up panic
/// `launch_smoke.rs` was written for, and the reason that file's guard now also
/// has to prove `.setup` calls this function, not only that the function works.
///
/// **It returns nothing and cannot fail the boot.** A `?` here would turn an
/// index this build cannot open — a database written by a newer Mnema, a
/// corrupt file — into an application that does not start, and a person who
/// cannot start it cannot be told why. `AppState::open_index`'s own doc records
/// the other half of that trade: calling it again re-opens, so a failed open is
/// recoverable inside the running process rather than only across a restart.
///
/// **The error is stored as well as logged**, which is the part that is easy to
/// leave out. Logged and dropped, a failed boot open is indistinguishable from
/// a boot that never ran: both leave `db` at `None`, both reach the window as
/// `IndexNotOpen`, and `UnreadableCause` has to call that "the ordinary state
/// at start-up". The state carries the answer instead — see
/// [`state::AppState::set_boot_open_error`] and what `models::index_settings`
/// then does with it.
///
/// **This is where "ask what disappears" (CLAUDE.md) binds this task,
/// contrary to what the plan assumed when it scoped that pass out.** Opening at
/// boot means start-up now *writes* — `open_index` creates the directory,
/// creates the file, and runs `apply(&mut conn)`
/// (`crates/mnema-index/src/open.rs:118`) before anyone has touched a folder.
/// Four things considered; nothing found that a person can lose.
///
/// 1. **A migration against an index a newer build wrote.** `to_latest` wraps
///    the whole migration set in one transaction
///    (`crates/mnema-index/src/migrations.rs:85-88`), so "migration number too
///    high" fails `open` atomically — the file is left exactly as it was, and
///    the boot reports it rather than starting from it. `AppState::open_index`'s
///    own doc already names this state; this function is what makes reaching
///    it silent-but-safe rather than silent-but-lost.
/// 2. **The boot racing the job's second connection**
///    (`AppState::open_job_index`, `state.rs:222`). Structurally it cannot,
///    today: this call runs inside the `Ready`-event handler that Tauri itself
///    uses to create every configured window and then call this closure, in
///    that order, before the event loop advances to deliver anything a webview
///    could send (`tauri-2.11.5/src/app.rs:2524-2533` builds the windows;
///    `:1414-1416` runs both inside one `RuntimeRunEvent::Ready` arm) — so no
///    command reaches `with_index` or `open_job_index` before this line has
///    already run. Were that ever not true, the two connections still could not
///    corrupt each other: WAL with a five-second busy timeout serialises them
///    (`crates/mnema-index/src/open.rs:114-115`), and `to_latest` is a no-op
///    once `user_version` is current, so the loser of the race would wait, not
///    fail.
/// 3. **A `data_dir` that cannot be created.** `open_index` reports
///    `Error::DataDir` (`state.rs:156-157`); nothing existed yet to lose, and
///    the window is told `ReadFailed` instead of being shown a healthy state it
///    does not have.
/// 4. **A second call.** `AppState::open_index`'s own doc says calling it again
///    drops the previous connection and reopens — recoverable, not lossy — but
///    this line is the setter's only caller (also cited from
///    [`state::AppState::set_boot_open_error`]'s doc), so nothing calls it
///    twice today.
///
/// So opening at boot is safe to keep as a write, and does not, on its own,
/// owe this task a new index-writing command or a wider pass than this one.
///
/// **It also applies the default models, and on a thread of its own.** An index
/// that opens here is the second half of
/// [`models::choose_the_default_models_for_a_stored_key`]'s rule — a key stored
/// while no index was open leaves an installation with a key and no models, and
/// nothing else ever comes back to fix it. Two reasons it is not run inline:
/// this closure is Tauri's `Ready` handler, so blocking it is the frozen window
/// [`bridge::open_index`]'s own doc is about, and the work reads the credential
/// store (on macOS, possibly a dialog) and then asks the provider a question
/// with a thirty-second timeout behind it. Neither belongs on the boot path.
///
/// **The handle is returned rather than dropped**, and only for that reason: a
/// test that asserts about what the thread did has to be able to wait for it,
/// and a test that polls for a background write is a test that reports timing.
/// The caller in `.setup` drops it, which detaches the thread.
pub fn boot_index<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> std::thread::JoinHandle<()> {
    let state = app.state::<state::AppState>();
    let outcome = state.open_index();
    if let Err(e) = &outcome {
        // The log line is for the terminal a developer launched from; the stored
        // sentence is for the person who has no terminal.
        eprintln!("mnema: the index could not be opened at start-up: {e}");
    }
    state.set_boot_open_error(outcome.err().map(|e| e.to_string()));

    // Spawned unconditionally, including after a failed open: every step of what
    // it runs is already silent against an index that will not answer, so the
    // thread costs one spawn and does nothing. A condition here would be a
    // second spelling of a rule that is written once, over there.
    let app = app.clone();
    std::thread::spawn(move || {
        models::choose_the_default_models_for_a_stored_key(&app.state::<state::AppState>());
    })
}

/// How many files the index holds, read once so `.setup` can seed
/// [`state::AppState::set_files`] before [`tray::build_tray`] draws the first
/// menu — without this, a person reopening an already-indexed archive would
/// see the tray's status line claim `0` files until the next job happened to
/// run.
///
/// A free function over `&AppState` rather than inline in `.setup`, for the
/// same reason [`manage_state`]/[`boot_index`] are: `.setup` needs a live
/// Tauri `App` to reach at all (`tests/commands.rs`'s `app_in` never runs it —
/// see this crate's own `AppState::with_index` doc), so the only way to unit
/// test what it does is to give each step its own name and callable shape.
///
/// **`0` on every failure — an unopened index included — never a panic.**
/// `.setup` calls this after `boot_index`, whose own `open_index` call is
/// synchronous (only the default-model adoption it spawns runs on a thread of
/// its own, deliberately not waited on) — but a boot whose open failed
/// outright still has to draw a tray, and `with_index` refuses with
/// `IndexNotOpen` exactly then. `0` is honest either way: nothing has been
/// counted, which is also literally true of a fresh index.
pub fn boot_files(state: &state::AppState) -> i64 {
    state.with_index(|db| db.indexed_file_count()).unwrap_or(0)
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

    use tauri_plugin_global_shortcut::ShortcutState;

    // 🔴 **No `.with_shortcut(…)`, and `.with_handler(…)` stays.** The
    // shortcut is registered from `.setup`, through `prefs::install_hotkey`,
    // because a builder registration that fails is fatal — `with_shortcut(…)?`
    // is exactly the D128 defect this removes: a shortcut another application
    // already holds became a reason for this one not to start.
    //
    // The handler must NOT move with it. `GlobalShortcut::register` attaches
    // none at all — it passes `None::<fn(&AppHandle<R>, &Shortcut,
    // ShortcutEvent)>` to `register_internal`
    // (`tauri-plugin-global-shortcut-2.3.2/src/lib.rs:131-140`) — and this
    // builder handler (`:380-385`) is the other source; both are dispatched
    // together at `:416-423`, so this ONE closure serves every shortcut the
    // registrar ever takes, including one the person picks later. Delete it and
    // call `register`, and the operating system grabs the shortcut while
    // nothing in this application hears it — which no headless test can catch,
    // since D-d forbids driving the real registrar.
    let global_shortcut = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            // Whatever shortcut is bound, there is only ever one; act on the
            // press edge, not the release.
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
        // `capabilities/default.json`.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(global_shortcut)
        // Launch at login (D-c). The macOS launcher variant and no arguments:
        // the application starts the same way from a login item as from the
        // Dock. The webview never calls this plugin — it calls `set_autostart`,
        // which is this application's own command — so no `autostart:allow-*`
        // capability entry is expected.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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
            // §8: ask the running job to stop. There is no guard here and none
            // is owed — `cancel_job` on an idle application returns `()` after
            // storing the flag (`state.rs`'s `cancel_job`), and `claim_job`
            // clears that flag *after* it has won the slot, so a press with
            // nothing running cannot reach into the next job. The item is
            // enabled only while a cancellable job runs (`tray::stop_enabled`,
            // drawn by `tray::refresh_tray`) so as not to offer a control that
            // does nothing, which is a different concern from safety.
            tray::STOP_ID => {
                app.state::<state::AppState>().cancel_job();
            }
            // F4 (Task 10c): the other half of the pair above — the tray could
            // stop a scan and not carry one on, so a person who pressed Stop
            // here had to open the settings window to find «Продовжити».
            // `scan_job::start` is handed in rather than reached for inside:
            // it is the same function `start_scan_job` calls, so a tray press
            // and the window's button start the same scan. What entry that is,
            // and whether a press starts anything at all, is
            // `scan_job::resume_scan`'s decision and is tested there — the item
            // is enabled only when the ended report names a resume
            // (`tray::resume_enabled`, drawn by `tray::refresh_tray`), and a
            // press on a snapshot that has moved on since the draw is refused
            // and logged, not shown (§6).
            //
            // 🔴 **On a thread of its own, and that is not a nicety.** This
            // handler runs on the main thread. `scan_job::start` reaches
            // `start_inner`, which claims the slot, opens a job index and runs
            // `read_roots` — a `with_index` call that blocks for as long as
            // another job holds the connection (a folder removal alone, on the
            // order of twenty seconds). Inline, a press would freeze every
            // window redraw and every other menu click for that time. This is
            // the same thing `start_scan_job` buys with
            // `#[tauri::command(async)]`, which exists for exactly this reason
            // (`bridge::open_index`'s doc): a call that can wait on that mutex
            // must not be the one left running inline on the main thread.
            // `std::thread::spawn` rather than the async runtime because the
            // work is blocking and synchronous either way, and this is the
            // shape `refresh_tray`'s own hop already uses. Nothing is awaited:
            // the press's whole answer is the scan starting, which the tray
            // learns about through the job observer like every other surface.
            // `the_menu_handler_starts_a_scan_only_off_the_main_thread` is the
            // guard.
            tray::RESUME_ID => {
                let app = app.clone();
                std::thread::spawn(move || {
                    scan_job::resume_scan(&app.state::<state::AppState>(), scan_job::start);
                });
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
                    crate::tray::swap_tray_menu(app, current.effective, current.choice);
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
            // Immediately after the state exists and before anything else in
            // this closure, so every later step here meets an index that is
            // already open, and so does every command arriving after start-up —
            // which is as early as a boot can make it, not a promise about a
            // webview that is already invoking while `.setup` runs.
            // The handle is dropped, which detaches the thread it carries: the
            // boot does not wait on a credential store or a provider, and the
            // defaults it applies are wanted by the next command, not by the
            // next line of this closure. See `boot_index`'s own doc.
            drop(boot_index(app.handle()));
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
            // The operating-system services, and then the one boot the hotkey
            // has. Installed AFTER `manage_state` (there is no state to install
            // into before it) and after the locale is seeded, because a
            // registration failure's sentence is the plugin's own and the state
            // it lands in is read back by the settings window.
            //
            // 🔴 `install_hotkey` returns a `HotkeyState` and CANNOT fail the
            // boot. A `?` here would be the D128 defect moving house: a
            // shortcut another application already holds would once again stop
            // this application from starting, and a person who cannot start it
            // cannot be told why. Degraded, not broken — the tray's
            // «Показати пошук» still opens the launcher.
            {
                let state = app.state::<state::AppState>();
                state.install_os_services(
                    Box::new(os_services::PluginShortcuts::new(app.handle().clone())),
                    Box::new(os_services::PluginAutolaunch::new(app.handle().clone())),
                );
                let _ = prefs::install_hotkey(&state);
            }
            // §9.3/Task 5: seed how many files the index already holds BEFORE
            // the tray is built, so the very first menu this application draws
            // reads «Проскановано: N файлів» from a real count rather than
            // `ScanState::default()`'s `files: 0` — a person reopening an
            // already-indexed archive would otherwise see "0 files" flash
            // before the first job ever ran. Must run before `build_tray`,
            // which reads `scan_state()` to seed the status line's initial
            // text; `boot_files` is a free function precisely so this line is
            // unit-testable without a `.setup` to run it in.
            {
                let state = app.state::<state::AppState>();
                let files = boot_files(&state);
                state.set_files(files);
            }
            // §8: the tray's «Зупинити сканування» and its status line.
            // `build_tray` manages `TrayItems` itself now (Task 5) — nothing
            // here or below captures either item directly, since a language
            // change during a job rebuilds the whole tray menu and would leave
            // a captured handle addressing an item that is in no menu.
            tray::build_tray(app.handle())?;
            {
                let state = app.state::<state::AppState>();
                // 🔴 The closure captures the handle and NOTHING else — see
                // `tray::refresh_tray`'s own doc for why it re-reads
                // `AppState` itself rather than being handed a value: a
                // language change during a job rebuilds the whole tray menu,
                // and a value captured here could be the announcement that
                // arrived before that rebuild.
                //
                // It dispatches and returns rather than redrawing inline.
                // `set_text`/`set_enabled` hop to the main thread and wait,
                // and the announcement from `JobSlot::drop` fires on the
                // job's own thread — where that wait would hold the job
                // thread until the event loop got round to it.
                //
                // The emit happens OFF the main thread, on the job's own —
                // `state`/`job.rs` stay free of Tauri types, and `emit` does
                // not need the main thread the way a menu redraw does. It may
                // fire the same revision twice if two announcements race
                // (`state::JobObserver`'s own doc has why two announcements
                // can arrive in either order) — harmless: a window that
                // re-draws from an unchanged `ScanState` draws the same thing
                // it already had.
                let handle = app.handle().clone();
                state.set_job_observer(Box::new(move || {
                    let inner = handle.clone();
                    let scan = handle.state::<state::AppState>().scan_state();
                    let _ = handle.emit("scan-progress", &scan);
                    let _ = handle.run_on_main_thread(move || {
                        tray::refresh_tray(&inner);
                    });
                }));
                // Seeded AFTER the observer is installed, which is what makes
                // "nothing is missed between the two" a fact about the order
                // rather than a claim that nothing can have claimed the slot
                // this early. A claim arriving between these two statements
                // announces itself through the observer above, and this seed
                // then redraws from the same fact that announcement would
                // have read.
                tray::refresh_tray(app.handle());
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An `AppState` pointed at a fresh temp directory — enough for
    /// `open_index`/`with_index` and nothing more: no provider, no
    /// credential store, the same trade `state.rs`'s own observer-test
    /// helper makes, because `boot_files` touches neither.
    fn state_in(dir: &std::path::Path) -> state::AppState {
        state::AppState::new(
            dir.to_path_buf(),
            std::path::PathBuf::from("/nonexistent/mnema-boot-files-worker"),
            "http://127.0.0.1:0".to_string(),
            "mnema-desktop-boot-files-test".to_string(),
        )
    }

    /// The pair `boot_files` exists to tell apart: an index that already
    /// holds files (the seed a person reopening an already-indexed archive
    /// needs) against one that was never opened at all (a boot before
    /// `boot_index` ran, or one where the open itself failed) — both
    /// directions, so a `boot_files` that always answered `0`, or one that
    /// panicked instead of falling back, would go red on one row or the
    /// other rather than passing by accident.
    #[test]
    fn boot_files_counts_an_open_index_and_falls_back_to_zero_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path());

        assert_eq!(
            boot_files(&state),
            0,
            "no index has been opened yet — nothing to count, and not a panic"
        );

        state.open_index().expect("the index opens");
        state
            .with_index(|db| {
                let root = db.insert_watched_root("/tmp/mnema-boot-files-fixture")?;
                for (i, name) in ["a.txt", "b.txt", "c.txt"].iter().enumerate() {
                    let id = format!("{i:064x}");
                    db.insert_document(&id, "text/plain", 1, mnema_core::SourceKind::Document)?;
                    db.set_document_status(&id, mnema_index::DocumentStatus::Indexed)?;
                    db.insert_path(
                        root,
                        name,
                        &id,
                        mnema_core::OnDisk {
                            size_bytes: 1,
                            mtime: 1,
                        },
                        "text",
                        1,
                    )?;
                }
                Ok(())
            })
            .expect("the fixture writes");

        assert_eq!(
            boot_files(&state),
            3,
            "three indexed files were seeded into the now-open index"
        );
    }

    /// 🔴 **Brittle by design — a text-matching guard, not a type-level one.**
    /// It reads `lib.rs`'s own source and asserts that the ONE `with_index`
    /// substring never appears inside the `.setup` observer's
    /// `run_on_main_thread` closure — the invariant `tray::refresh_tray`'s own
    /// doc names: that closure runs on the main thread, and `with_index`
    /// blocks for as long as a job holds the index (a folder removal alone,
    /// on the order of twenty seconds), so a `with_index` call reachable from
    /// there would freeze every window redraw and every menu click for that
    /// long.
    ///
    /// 🔴 **Review round 1, Important 1 — a first-match `find` picks the wrong
    /// closure and is unfalsifiable against itself.** Two things were wrong
    /// with the original version, and both are fixed here rather than only
    /// documented: (1) `src.find(needle)` took the FIRST occurrence in the
    /// whole file, so a second `run_on_main_thread(move || {` added anywhere
    /// earlier in `lib.rs` (Tasks 6/10 both touch `.setup`) would silently
    /// steal the match and this test would go on passing while the real
    /// closure grew a `with_index`; (2) the needle was also this test's OWN
    /// string literal, so `find` could never return `None` and the "moved,
    /// renamed, or removed" branch was dead code. The fix: search only the
    /// PRODUCTION half of the file — everything above `#[cfg(test)]`, which
    /// this test's own source (including its needle and its `with_index`
    /// literal) never reaches — and require EXACTLY one match there. Zero
    /// matches (renamed/removed) and two-or-more matches (a second hop stole
    /// or shares the search) each fail with their own message instead of one
    /// swallowing the other. The needle is built with `concat!` on top of
    /// that even so: splitting `"run_on_main_thread"` from `"(move || {"`
    /// means no future refactor that widens the search region can make this
    /// test's own source satisfy its own search by accident.
    ///
    /// It still protects only the ONE call site this file writes today —
    /// splitting the closure into a named function, or a `with_index` reached
    /// indirectly through a function this test cannot see into
    /// (`tray::refresh_tray` itself, or anything it calls) would slip straight
    /// past it, and a genuine SECOND `run_on_main_thread` hop added above the
    /// observer needs a guard of its own (or this one taught to check both) —
    /// this test can only say "not exactly one," not which one is the real
    /// observer. A `#[test]` was chosen over nothing because nothing is a
    /// worse guard still; if a reviewer would rather have this as a
    /// mutation-harness case instead, that is Task 11's to make, not this
    /// one's.
    #[test]
    fn the_main_thread_closure_never_touches_the_index() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("lib.rs could not read its own source at {path:?}: {e}"));

        // Only the PRODUCTION half of the file is a valid haystack — this
        // test's own module (its needle literal, its `with_index` literal,
        // any decoy this test itself might one day contain) sits below
        // `#[cfg(test)]` and must never be searched, or a match against this
        // test's own source is indistinguishable from a match against the
        // real closure.
        let cfg_test_at = src
            .find("#[cfg(test)]")
            .expect("this file must carry its own #[cfg(test)] module marker");
        let production = &src[..cfg_test_at];

        // `concat!` rather than one string literal: the point of restricting
        // the search to `production` only holds as long as this needle
        // cannot appear as a contiguous substring of the test's OWN source
        // (which is excluded here, but a future reader who widens the region
        // should not get a false green for free) — splitting the call name
        // from its argument list means no single literal in this file spells
        // the whole needle out.
        let needle = concat!("run_on_main_thread", "(move || {");

        let occurrences: Vec<usize> = production.match_indices(needle).map(|(i, _)| i).collect();
        let call_at = match occurrences.as_slice() {
            [one] => *one,
            [] => panic!(
                "no `{needle}` found above #[cfg(test)] — the observer's main-thread hop moved, \
                 was renamed, or was removed"
            ),
            many => panic!(
                "found {} occurrences of `{needle}` above #[cfg(test)] — this guard only knows \
                 how to check ONE `run_on_main_thread` closure and cannot tell which is the \
                 observer's; a second call site needs a guard of its own or this one adapted to \
                 check all of them. Byte offsets: {many:?}",
                many.len()
            ),
        };
        let body_start = call_at + needle.len();

        // Balance braces from just after the closure's opening `{` to find
        // where the closure body ends, so this does not have to assume any
        // particular length or shape for what is inside.
        let mut depth: i32 = 1;
        let mut body_end = body_start;
        for (offset, ch) in production[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(
            depth == 0,
            "the closure's braces never balanced — this guard's own brace-matching broke, \
             not the invariant it protects"
        );

        let body = &production[body_start..body_end];
        assert!(
            !body.contains("with_index"),
            "a `with_index` call reached the main-thread closure — this would block the whole \
             application for as long as a job holds the index. Closure body:\n{body}"
        );
    }
    /// 🔴 **The second region of the same brittle guard above, and for a
    /// harder-won reason.** Review round 1, Important 1: the `"resume"` arm
    /// called `scan_job::resume_scan` inline on the main thread, and the start
    /// it leads to is not cheap — `scan_job::start_inner` claims the slot,
    /// opens a job index and runs `read_roots`, which is a `with_index` call.
    /// `with_index` blocks for as long as another job holds the connection (a
    /// folder removal alone, on the order of twenty seconds), so the press
    /// would have frozen every window redraw and every other menu click for
    /// that long. The window never had this defect: `start_scan_job` is
    /// `#[tauri::command(async)]` precisely so that a command which waits on
    /// that mutex is not left running inline on the main thread. The tray now
    /// buys the same thing with `std::thread::spawn`.
    ///
    /// The guard is the whole `on_menu_event` handler, not just the one arm:
    /// EVERY occurrence of the needles below anywhere in that handler must sit
    /// inside a spawned closure. A future arm that starts a scan of its own
    /// inline is the same defect and is caught by the same assertion, without
    /// this test having to know the arm exists.
    ///
    /// 🔴 **The needles are the CALL, not the function's name alone.** The
    /// review named `scan_job::start(` — that literal appears nowhere, because
    /// `start` is passed to `resume_scan` as a function REFERENCE and never
    /// called from this file at all, so a guard built on it would be a guard
    /// that cannot fail. `resume_scan(` is the call this handler actually
    /// makes, and the bare `scan_job::start` is kept beside it so that handing
    /// the starter to anything else in this handler is caught too.
    ///
    /// Its limits are the neighbouring guard's: it protects the call sites
    /// this file WRITES. A start reached indirectly through a function this
    /// test cannot see into slips past, and so does a spawn hidden behind a
    /// helper of another name.
    #[test]
    fn the_menu_handler_starts_a_scan_only_off_the_main_thread() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("lib.rs could not read its own source at {path:?}: {e}"));

        // The production half only, for `the_main_thread_closure_never_touches_
        // the_index`'s reason: this test's own needles are string literals in
        // the module below `#[cfg(test)]`, and a match against them is
        // indistinguishable from a match against the handler.
        let cfg_test_at = src
            .find("#[cfg(test)]")
            .expect("this file must carry its own #[cfg(test)] module marker");
        let production = &src[..cfg_test_at];

        // `concat!` for the same reason the neighbouring guard gives: no single
        // literal in this file spells a whole needle out.
        let handler_at = {
            let opener = concat!(".on_menu_event", "(|app, event| match");
            let found: Vec<usize> = production.match_indices(opener).map(|(i, _)| i).collect();
            match found.as_slice() {
                [one] => *one,
                [] => panic!(
                    "no `{opener}` found above #[cfg(test)] — the menu handler moved, was \
                     renamed, or was removed, and this guard is now protecting nothing"
                ),
                many => panic!(
                    "found {} occurrences of `{opener}` — this guard only knows how to check \
                     ONE menu handler. Byte offsets: {many:?}",
                    many.len()
                ),
            }
        };
        let handler = &production[handler_at..handler_at + balanced_len(&production[handler_at..])];
        // 🔴 **Comments are blanked before the search, byte for byte.** The
        // arm's own comment explains the fix in the words `scan_job::start`,
        // and a guard that reads prose as code fails on the sentence that
        // documents it — which is not a defect, it is a guard measuring the
        // wrong thing. Blanking with SPACES (one byte each, as many as the
        // comment held) keeps every offset in `code` equal to its offset in
        // `handler`, so the failure message can quote the real source.
        //
        // It is `//` to end of line, the same rule `tests/locale_guard.rs`'s
        // own sweep uses. A `//` inside a string literal would be blanked too;
        // there is none in this handler, and the day there is, the guard's
        // failure mode is a false GREEN — which is why the needles below are
        // asserted to be present at all.
        let code = blank_comments(handler);

        // Every `std::thread::spawn(move || {` body inside the handler, as
        // half-open byte ranges — the regions a start is allowed to happen in.
        let spawn_opener = concat!("std::thread::spawn", "(move || {");
        let spawned: Vec<std::ops::Range<usize>> = code
            .match_indices(spawn_opener)
            .map(|(at, _)| {
                let body = at + spawn_opener.len();
                body..body + balanced_len_from_inside(&code[body..])
            })
            .collect();

        for needle in [concat!("resume_scan", "("), concat!("scan_job::", "start")] {
            let hits: Vec<usize> = code.match_indices(needle).map(|(i, _)| i).collect();
            assert!(
                !hits.is_empty(),
                "no `{needle}` in the menu handler — the resume arm moved or was renamed, and \
                 this guard is now unfalsifiable"
            );
            for at in hits {
                assert!(
                    spawned.iter().any(|body| body.contains(&at)),
                    "`{needle}` at byte {at} of the menu handler is NOT inside a \
                     `{spawn_opener}` closure — it would run on the main thread, and the start \
                     it leads to opens the index (`scan_job::start_inner` → `read_roots` → \
                     `with_index`), freezing every window redraw and every other menu click \
                     for as long as a job holds the connection. Handler:\n{handler}"
                );
            }
        }
    }

    /// `src` with every `//`-to-end-of-line comment replaced by exactly as many
    /// SPACES as it held bytes, so that offsets into the answer are offsets
    /// into `src`.
    fn blank_comments(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        for line in src.split_inclusive('\n') {
            match line.find("//") {
                Some(at) => {
                    out.push_str(&line[..at]);
                    let commented = &line[at..];
                    let newline = commented.ends_with('\n');
                    let blanked = commented.len() - usize::from(newline);
                    out.push_str(&" ".repeat(blanked));
                    if newline {
                        out.push('\n');
                    }
                }
                None => out.push_str(line),
            }
        }
        debug_assert_eq!(out.len(), src.len(), "blanking moved the offsets");
        out
    }

    /// The byte length of `src` from its FIRST `{` through the `}` that closes
    /// it — the shape both source-reading guards need and neither should write
    /// twice.
    fn balanced_len(src: &str) -> usize {
        let open = src.find('{').expect("no `{` to balance from");
        open + 1 + balanced_len_from_inside(&src[open + 1..])
    }

    /// The byte length of the region from `src`'s start (already INSIDE one
    /// open brace) up to, but not including, the `}` that closes it.
    fn balanced_len_from_inside(src: &str) -> usize {
        let mut depth: i32 = 1;
        for (offset, ch) in src.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return offset;
                    }
                }
                _ => {}
            }
        }
        panic!(
            "braces never balanced — this guard's own brace-matching broke, not the invariant it protects"
        );
    }
}
