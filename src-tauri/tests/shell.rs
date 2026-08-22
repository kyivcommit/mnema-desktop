//! The shell's pure seams — the parts of the tray/launcher wiring that do not
//! need a real window manager, so they run headlessly on both CI legs. The
//! tray itself, the global shortcut, transparency, and positioning are OS-level
//! and are verified by the live `cargo tauri dev` run, not here.

use mnema_desktop::tray::MENU_ITEMS;

#[test]
fn the_tray_menu_is_the_spec_menu_in_order() {
    // §8: статус · Показати пошук · Відкрити налаштування · Пауза індексації ·
    // Перевірити оновлення · Вийти.
    let ids: Vec<&str> = MENU_ITEMS.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [
            "status",
            "show_search",
            "open_settings",
            "pause_indexing",
            "check_updates",
            "quit"
        ],
        "the tray menu drifted from spec §8"
    );
    // Every id is unique — a duplicate id would make `on_menu_event` ambiguous.
    let mut seen = std::collections::HashSet::new();
    for (id, label) in MENU_ITEMS {
        assert!(seen.insert(*id), "duplicate tray menu id: {id}");
        assert!(!label.is_empty(), "empty label for tray id {id}");
    }
}
