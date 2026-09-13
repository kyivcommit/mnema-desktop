# Mutation cases for the launcher position (D155). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/launcher-position.sh
#
# Each case names the one test that must go red when the line it quotes is
# changed. Written from the tests, not the other way round.

case_ "launcher position: the monitor's right edge counts as inside" \
  src-tauri/src/launcher_position.rs \
  's~&& hx < x0 \+ f64::from\(area\.size\.width\)~\&\& hx <= x0 + f64::from(area.size.width)~' \
  '&& hx <= x0 + f64::from(area.size.width)' \
  mnema-desktop 'launcher_position::tests::the_work_area_edge_is_outside' --lib

case_ "launcher position: the handle offset ignores the monitor's scale" \
  src-tauri/src/launcher_position.rs \
  's~let hx = f64::from\(p\.x\) \+ HANDLE_CENTRE\.0 \* scale;~let hx = f64::from(p.x) + HANDLE_CENTRE.0;~' \
  'let hx = f64::from(p.x) + HANDLE_CENTRE.0;' \
  mnema-desktop 'launcher_position::tests::the_handle_offset_scales_with_the_monitor' --lib

case_ "launcher position: the corner is checked instead of the handle" \
  src-tauri/src/launcher_position.rs \
  's~let hx = f64::from\(p\.x\) \+ HANDLE_CENTRE\.0 \* scale;~let hx = f64::from(p.x);~' \
  'let hx = f64::from(p.x);' \
  mnema-desktop 'launcher_position::tests::a_window_whose_corner_is_on_screen_but_whose_handle_is_not_is_dropped' --lib

case_ "launcher position: a hide without a drag is written anyway" \
  src-tauri/src/launcher_position.rs \
  's~if reference == now \{\n            return None;~if false {\n            return None;~' \
  'if false {
            return None;' \
  mnema-desktop 'launcher_position::tests::a_hide_without_a_drag_writes_nothing' --lib

case_ "launcher position: a failed write forgets the drag" \
  src-tauri/src/launcher_position.rs \
  's~slots\.left = Some\(now\);\n        Some\(now\)~slots.left = slots.left;\n        Some(now)~' \
  'slots.left = slots.left;' \
  mnema-desktop 'launcher_position::tests::a_failed_write_still_updates_memory' --lib

case_ "launcher position: y is not read" \
  src-tauri/src/launcher_position.rs \
  's~let y = i32::try_from\(value\.get\("y"\)\?\.as_i64\(\)\?\)\.ok\(\)\?;~let y = 0;~' \
  'let y = 0;' \
  mnema-desktop 'launcher_position::tests::the_position_survives_a_write_and_a_read' --lib

case_ "launcher position: the show does not record where it put the window" \
  src-tauri/src/launcher_position.rs \
  's~memory\.placed\(restored\);~let _ = restored;~' \
  'let _ = restored;' \
  mnema-desktop 'place_leaves_the_position_to_the_focus_in' --test shell

case_ "launcher position: a focus-in never settles the show" \
  src-tauri/src/launcher_position.rs \
  's~if slots\.awaiting \{~if false {~' \
  'if false {' \
  mnema-desktop 'launcher_position::tests::a_show_leaves_applied_open_until_the_focus_in_settles_it' --lib

case_ "launcher position: a fallback show keeps the stale drag in memory" \
  src-tauri/src/launcher_position.rs \
  's~if restored\.is_none\(\) \{~if false {~' \
  'if false {' \
  mnema-desktop 'launcher_position::tests::a_show_that_could_not_restore_forgets_where_the_person_left_it' --lib

case_ "launcher: a visible launcher is re-placed by a second show" \
  src-tauri/src/lib.rs \
  's~if window\.is_visible\(\)\.unwrap_or\(false\) \{\n                let _ = window\.set_focus\(\);\n                return true;\n            \}~if false {\n                let _ = window.set_focus();\n                return true;\n            }~' \
  'if false {
                let _ = window.set_focus();
                return true;
            }' \
  mnema-desktop 'focus_launcher_leaves_a_visible_launcher_where_it_is' --test shell

case_ "launcher position: a position with nothing applied counts as a drag" \
  src-tauri/src/launcher_position.rs \
  's~let reference = slots\.left\.or\(slots\.applied\)\?;~let reference = slots.left.or(slots.applied).unwrap_or(PhysicalPosition::new(i32::MIN, i32::MIN));~' \
  'let reference = slots.left.or(slots.applied).unwrap_or(PhysicalPosition::new(i32::MIN, i32::MIN));' \
  mnema-desktop 'launcher_position::tests::a_quit_before_the_first_show_keeps_the_saved_position' --lib

case_ "launcher position: a drag back to the applied place is not a move" \
  src-tauri/src/launcher_position.rs \
  's~let reference = slots\.left\.or\(slots\.applied\)\?;~let reference = slots.applied?;~' \
  'let reference = slots.applied?;' \
  mnema-desktop 'launcher_position::tests::dragging_back_to_where_the_app_put_it_is_still_a_move' --lib

case_ "launcher position: the handle's y offset drifts" \
  src-tauri/src/launcher_position.rs \
  's~24\.0 \+ 11\.0 \+ 26\.0 / 2\.0\);~24.0 + 111.0 + 26.0 / 2.0);~' \
  '24.0 + 111.0 + 26.0 / 2.0);' \
  mnema-desktop 'launcher_position::tests::the_work_area_bottom_edge_is_outside' --lib

case_ "launcher position: the handle's y offset is ignored" \
  src-tauri/src/launcher_position.rs \
  's~let hy = f64::from\(p\.y\) \+ HANDLE_CENTRE\.1 \* scale;~let hy = f64::from(p.y);~' \
  'let hy = f64::from(p.y);' \
  mnema-desktop 'launcher_position::tests::a_window_above_the_top_edge_with_its_handle_below_it_is_kept' --lib

case_ "launcher position: remember writes nothing" \
  src-tauri/src/launcher_position.rs \
  's~if let Err\(e\) = remember_position\(now, memory, data_dir\) \{~if let Err(e) = Ok::<(), std::io::Error>(()) {~' \
  'if let Err(e) = Ok::<(), std::io::Error>(()) {' \
  mnema-desktop 'remember_writes_the_launcher_position_it_finds' --test shell

case_ "launcher: the search panel is not a drag region" \
  ui/src/launcher/Launcher.svelte \
  's~<div class="searchbar" data-tauri-drag-region="deep">~<div class="searchbar">~' \
  '<div class="searchbar">' \
  src/launcher/Launcher.test.ts 'the search panel is the drag handle and nothing else is' runner=vitest

case_ "launcher: the start-dragging permission is gone" \
  src-tauri/capabilities/launcher.json \
  's~, "core:window:allow-start-dragging"\]~]~' \
  '"core:event:allow-listen"]' \
  src/launcher/Launcher.test.ts 'the search panel is the drag handle and nothing else is' runner=vitest
