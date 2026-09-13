//! The two operating-system services the Application section drives, behind
//! traits — the global-shortcut registrar and launch-at-login.
//!
//! 🔴 **Nothing under `cargo test` may construct a real wrapper, and the reason
//! is not tidiness.** Both plugins have effects on the machine the suite runs
//! on, and each has its own way of being unusable from a test:
//!
//! - [`PluginShortcuts`] would take a real global shortcut away from every other
//!   application, from a test process. It also *cannot* run there:
//!   `GlobalShortcut::register` and `unregister` post a closure with
//!   `run_on_main_thread` and then block on `rx.recv()`
//!   (`tauri-plugin-global-shortcut-2.3.2/src/lib.rs:75-86`, used at `:96` and
//!   `:187`), and under `mock_builder()` the plugin is never initialised at all,
//!   so `app.global_shortcut()` is an unmanaged state lookup that panics before
//!   any of that. The conclusion is the same either way: no test drives it.
//! - [`PluginAutolaunch`] writes a real LaunchAgent plist (or a registry entry,
//!   or a `.desktop` file) pointing at whatever binary is running — which under
//!   `cargo test` is the **test binary**, left behind after the run.
//!
//! So [`ShortcutRegistrar`] and [`Autolaunch`] are traits, [`NoOsServices`] is
//! the inert default `AppState::new` installs, and the real wrappers — or, for
//! the shortcut in a Wayland session, [`WaylandNoShortcuts`] (D153) — are put
//! in place by `.setup` and by nothing else. That makes "the suite never touches
//! the plugins" **structural** rather than a convention somebody has to keep:
//! the default answers `Err`, and the only constructor of a real wrapper is
//! called from a closure no test runs. It is the same argument
//! `tests/dependency_boundary.rs` makes about Pdfium, one layer up.

use tauri::{AppHandle, Runtime};

/// Registering and unregistering the application's one global shortcut.
///
/// `String` rather than a typed error: what a caller does with a failure here
/// is put the sentence in front of a person (`HotkeyStatus::Unavailable`'s
/// `reason`, or a rejected command), and both plugins already produce a
/// showable one through their own `Display`.
pub trait ShortcutRegistrar: Send + Sync {
    fn register(&self, shortcut: &str) -> Result<(), String>;
    fn unregister(&self, shortcut: &str) -> Result<(), String>;
}

/// Launch at login, and — the part that matters — **reading back what the
/// operating system now says**, rather than echoing what was asked for.
pub trait Autolaunch: Send + Sync {
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
    fn is_enabled(&self) -> Result<bool, String>;
}

/// What an application has before `.setup` installs anything: every method
/// answers `Err`, so a hotkey reads as `Unavailable` and autostart as
/// `Unknown`.
///
/// Deliberately not a silent success. A no-op that answered `Ok` would let
/// `set_autostart` report `Enabled` for a machine on which nothing had been
/// enabled, which is the one thing D-c exists to prevent.
pub struct NoOsServices;

/// The sentence [`NoOsServices`] answers with. English, like every other
/// sentence this crate hands to a window outside `locale.rs`.
///
/// It is not expected to reach a person: `.setup` installs the real services
/// before any window is drawn. It reaches `tests/commands.rs`, where `app_in`
/// never runs `.setup`.
const NOT_INSTALLED: &str = "the operating-system services have not been installed";

impl ShortcutRegistrar for NoOsServices {
    fn register(&self, _shortcut: &str) -> Result<(), String> {
        Err(NOT_INSTALLED.to_string())
    }

    fn unregister(&self, _shortcut: &str) -> Result<(), String> {
        Err(NOT_INSTALLED.to_string())
    }
}

impl Autolaunch for NoOsServices {
    fn enable(&self) -> Result<(), String> {
        Err(NOT_INSTALLED.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        Err(NOT_INSTALLED.to_string())
    }

    fn is_enabled(&self) -> Result<bool, String> {
        Err(NOT_INSTALLED.to_string())
    }
}

/// The real registrar: `tauri-plugin-global-shortcut`.
///
/// Generic over the runtime and holding an `AppHandle`, so no runtime parameter
/// leaks into `AppState` — which holds `Box<dyn ShortcutRegistrar>` and knows
/// nothing about `R`.
///
/// 🔴 **`register`, not `on_shortcut`.** `GlobalShortcut::register` attaches no
/// handler — it passes `None::<fn(&AppHandle<R>, &Shortcut, ShortcutEvent)>` to
/// `register_internal` (`tauri-plugin-global-shortcut-2.3.2/src/lib.rs:131-140`)
/// — and the plugin builder's `with_handler` (`:380-385`) is the other source.
/// Both are dispatched together at `:416-423`, so the **one** handler on the
/// builder in `lib.rs` serves every shortcut this ever takes. Attaching a
/// second one here would run `toggle_launcher` twice per press.
pub struct PluginShortcuts<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> PluginShortcuts<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> ShortcutRegistrar for PluginShortcuts<R> {
    fn register(&self, shortcut: &str) -> Result<(), String> {
        use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
        self.app
            .global_shortcut()
            .register(shortcut)
            .map_err(|e| e.to_string())
    }

    fn unregister(&self, shortcut: &str) -> Result<(), String> {
        use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
        self.app
            .global_shortcut()
            .unregister(shortcut)
            .map_err(|e| e.to_string())
    }
}

/// The real launch-at-login: `tauri-plugin-autostart` 2.5.1.
///
/// `ManagerExt::autolaunch()` is a `State<AutoLaunchManager>` lookup, so the
/// plugin has to be initialised before any of these run. It is: Tauri
/// initialises plugins inside `build()` (`tauri-2.11.5/src/app.rs:2440`) and
/// runs the `.setup` closure afterwards (`:2531`), and `.setup` is the only
/// place this type is constructed.
pub struct PluginAutolaunch<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> PluginAutolaunch<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> Autolaunch for PluginAutolaunch<R> {
    fn enable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt as _;
        self.app.autolaunch().enable().map_err(|e| e.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt as _;
        self.app.autolaunch().disable().map_err(|e| e.to_string())
    }

    fn is_enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt as _;
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|e| e.to_string())
    }
}

/// The sentence a Wayland session's registrar answers with. English, like
/// every other sentence this module hands to a window (see [`NOT_INSTALLED`]'s
/// doc): it lands in `HotkeyStatus::Unavailable { reason }` and the settings
/// window shows it under «not registered with the system», followed by its
/// own «the search can still be opened from the tray» — so the reason names
/// only the cause.
pub const WAYLAND_REASON: &str =
    "Wayland session: this application cannot register a global shortcut.";

/// The registrar `.setup` installs on Linux when the session is Wayland
/// (D153). `PluginShortcuts` there takes an X11 grab through XWayland that
/// *succeeds* and never fires — 2026-09-13 stand smoke, F1 — so the section
/// said «registered with the system» about a shortcut that did nothing.
/// Refusing up front is the honest state. `unregister` answers `Ok` because
/// this registrar never registers anything, so there is never anything to
/// release — the trait's contract for an unregistered shortcut. (Today no
/// caller reaches it under this registrar: `change_hotkey` only unregisters a
/// `Registered` current shortcut, and this registrar never produces one —
/// `a_refused_registration_from_an_unavailable_start_takes_nothing_back` in
/// `tests/commands.rs` pins exactly that.)
pub struct WaylandNoShortcuts;

impl ShortcutRegistrar for WaylandNoShortcuts {
    fn register(&self, _shortcut: &str) -> Result<(), String> {
        Err(WAYLAND_REASON.to_string())
    }

    fn unregister(&self, _shortcut: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Whether `WAYLAND_DISPLAY` names a Wayland session — the one signal the
/// compositor leaves in every process it starts. Pure so the rule is testable;
/// [`wayland_session`] reads the real environment.
pub fn wayland_session_from(wayland_display: Option<std::ffi::OsString>) -> bool {
    wayland_display.is_some_and(|d| !d.is_empty())
}

/// [`wayland_session_from`] on this process's environment. Only Linux has a
/// Wayland; elsewhere the variable is meaningless and this is `false`.
pub fn wayland_session() -> bool {
    cfg!(target_os = "linux") && wayland_session_from(std::env::var_os("WAYLAND_DISPLAY"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wayland_session_registers_nothing_and_says_why() {
        // Under Wayland the X11 grab `PluginShortcuts` makes through XWayland
        // "succeeds" and never fires (2026-09-13 stand smoke, F1). The honest
        // registrar for that session refuses up front. The sentence names the
        // session, nothing more: the window already adds «the search can still
        // be opened from the application icon in the tray» under any refusal.
        let reason = WaylandNoShortcuts.register("Alt+Space").unwrap_err();
        assert_eq!(reason, WAYLAND_REASON);
    }

    #[test]
    fn a_wayland_session_has_nothing_to_unregister() {
        // The trait's contract, not a path any caller takes today: nothing
        // this registrar ever registered, so nothing is there to release, and
        // an `Err` here would be a second refusal with no registration behind
        // it. `change_hotkey` never gets here under it — see the type's doc.
        assert_eq!(WaylandNoShortcuts.unregister("Alt+Space"), Ok(()));
    }

    #[test]
    fn wayland_is_recognised_from_a_set_display_and_nothing_else() {
        use std::ffi::OsString;
        assert!(wayland_session_from(Some(OsString::from("wayland-0"))));
        assert!(!wayland_session_from(Some(OsString::new())));
        assert!(!wayland_session_from(None));
    }
}
