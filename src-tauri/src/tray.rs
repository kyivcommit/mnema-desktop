//! The menu-bar tray: the resident's only always-present surface and, by §6,
//! the only way to quit. Built in `lib.rs::run`'s `setup` hook.

use tauri::Manager as _;
use tauri::{
    Runtime,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

/// The tray menu in display order, as `(id, label)` — §8. Kept as pure data so
/// a headless test can assert the item set; the OS tray cannot be built without
/// a window manager, so `build_tray` is exercised only by the live run.
/// `status` is a disabled label (the count arrives with PR 9); `pause_indexing`
/// and `check_updates` are touch-points wired in PR 9.
pub const MENU_ITEMS: &[(&str, &str)] = &[
    ("status", "Проіндексовано —"),
    ("show_search", "🔍 Показати пошук (⌥Space)"),
    ("open_settings", "⚙ Відкрити налаштування"),
    ("pause_indexing", "⏸ Пауза індексації"),
    ("check_updates", "↻ Перевірити оновлення"),
    ("quit", "⏻ Вийти"),
];

fn label(id: &str) -> &'static str {
    MENU_ITEMS
        .iter()
        .find(|(item_id, _)| *item_id == id)
        .map(|(_, l)| *l)
        .expect("unknown tray menu id")
}

/// Builds the tray icon and its menu, and wires the menu events. Returns an
/// error the `setup` hook propagates. `show_search` shares `crate::focus_launcher`
/// with the single-instance callback and the ⌥Space handler.
pub fn build_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", label("status"), false, None::<&str>)?;
    let show_search =
        MenuItem::with_id(app, "show_search", label("show_search"), true, None::<&str>)?;
    let open_settings = MenuItem::with_id(
        app,
        "open_settings",
        label("open_settings"),
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(
        app,
        "pause_indexing",
        label("pause_indexing"),
        true,
        None::<&str>,
    )?;
    let updates = MenuItem::with_id(
        app,
        "check_updates",
        label("check_updates"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", label("quit"), true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &show_search,
            &open_settings,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &updates,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("mnema-tray")
        .icon(
            app.default_window_icon()
                .expect("a default window icon")
                .clone(),
        )
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_search" => {
                crate::focus_launcher(app);
            }
            "open_settings" => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                // Settings is now up → become a Regular app so it carries the
                // standard macOS menu bar (§8). No-op off macOS.
                crate::sync_activation_policy(app);
            }
            "quit" => app.exit(0),
            // status is disabled; pause_indexing / check_updates are PR 9.
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
        })
        .build(app)?;
    Ok(())
}
