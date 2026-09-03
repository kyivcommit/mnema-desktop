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
//! the inert default `AppState::new` installs, and the real wrappers are put in
//! place by `.setup` and by nothing else. That makes "the suite never touches
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
