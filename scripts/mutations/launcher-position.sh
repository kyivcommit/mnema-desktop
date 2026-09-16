# Mutation cases for the launcher position (D155). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/launcher-position.sh
#
# Each case names the one test that must go red when the line it quotes is
# changed. Written from the tests, not the other way round.

case_ "launcher position: the monitor's right edge counts as inside" \
  src-tauri/src/launcher_position.rs \
  's~hx < area\.x \+ area\.width~hx <= area.x + area.width~' \
  'hx <= area.x + area.width' \
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

case_ "launcher position: placed runs even on Wayland" \
  src-tauri/src/launcher_position.rs \
  's~if !wayland \{\n        memory\.placed\(restored\);\n    \}~memory.placed(restored);\n    if !wayland {\n    }~' \
  'memory.placed(restored);
    if !wayland {' \
  mnema-desktop 'a_wayland_show_leaves_nothing_awaiting' --test shell

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
  's~let reference = slots\.left\.or\(slots\.applied\)\?;~let reference = slots.left.or(slots.applied).unwrap_or(Point { x: i32::MIN, y: i32::MIN });~' \
  'let reference = slots.left.or(slots.applied).unwrap_or(Point { x: i32::MIN, y: i32::MIN });' \
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

case_ "launcher position: macOS keeps the physical space" \
  src-tauri/src/launcher_position.rs \
  's~Platform::Mac \| Platform::Linux => Self::Logical,~Platform::Mac => Self::Physical, Platform::Linux => Self::Logical,~' \
  'Platform::Mac => Self::Physical, Platform::Linux => Self::Logical,' \
  mnema-desktop 'launcher_position::tests::only_windows_keeps_the_physical_space' --lib

case_ "launcher position: the reporter's scale is ignored" \
  src-tauri/src/launcher_position.rs \
  's~x: \(f64::from\(p\.x\) / scale\)\.round\(\) as i32,~x: p.x,~' \
  'x: p.x,
                y: (f64::from(p.y) / scale).round() as i32,' \
  mnema-desktop 'launcher_position::tests::a_point_read_on_a_2x_monitor_is_saved_in_logical_points' --lib

case_ "launcher position: a logical point is restored as physical" \
  src-tauri/src/launcher_position.rs \
  's~Self::Logical => \{\n                Position::Logical\(LogicalPosition::new\(f64::from\(p\.x\), f64::from\(p\.y\)\)\)\n            \}~Self::Logical => Position::Physical(PhysicalPosition::new(p.x, p.y)),~' \
  'Self::Logical => Position::Physical(PhysicalPosition::new(p.x, p.y)),' \
  mnema-desktop 'launcher_position::tests::a_logical_point_restores_as_logical_whatever_the_new_windows_scale' --lib

case_ "launcher position: a monitor keeps its physical size in the logical space" \
  src-tauri/src/launcher_position.rs \
  's~width: w / scale,\n                    height: h / scale,~width: w, height: h,~' \
  'width: w, height: h' \
  mnema-desktop 'launcher_position::tests::a_2x_monitors_area_is_measured_in_logical_points_with_no_handle_factor' --lib

case_ "launcher position: the reporter's scale is ignored for y" \
  src-tauri/src/launcher_position.rs \
  's~y: \(f64::from\(p\.y\) / scale\)\.round\(\) as i32,~y: p.y,~' \
  'x: (f64::from(p.x) / scale).round() as i32,
                y: p.y,' \
  mnema-desktop 'launcher_position::tests::a_point_read_on_a_2x_monitor_is_saved_in_logical_points' --lib

case_ "launcher position: a monitor keeps its physical origin in the logical space" \
  src-tauri/src/launcher_position.rs \
  's~x: x / scale,\n                    y: y / scale,~x, y,~' \
  'Area {
                    x, y,' \
  mnema-desktop 'launcher_position::tests::a_2x_monitors_area_is_measured_in_logical_points_with_no_handle_factor' --lib

case_ "launcher position: the handle offset is scaled in the logical space too" \
  src-tauri/src/launcher_position.rs \
  's~height: h / scale,\n                \},\n                1\.0,~height: h / scale,\n                },\n                scale,~' \
  'height: h / scale,
                },
                scale,' \
  mnema-desktop 'launcher_position::tests::a_2x_monitors_area_is_measured_in_logical_points_with_no_handle_factor' --lib

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

case_ "launcher: the drag's own blur hides the window" \
  ui/src/launcher/Launcher.svelte \
  's~if \(Date\.now\(\) - handlePressedAt < DRAG_GRAB_WINDOW_MS\) return;~if (false) return;~' \
  'if (false) return;' \
  src/launcher/Launcher.test.ts 'a blur right after a press on the drag handle is the drag, not a dismissal' runner=vitest

case_ "launcher: a press anywhere arms the drag window" \
  ui/src/launcher/Launcher.svelte \
  's~handlePressedAt = onHandle \? Date\.now\(\) : -Infinity;~handlePressedAt = Date.now(); void onHandle;~' \
  'handlePressedAt = Date.now(); void onHandle;' \
  src/launcher/Launcher.test.ts 'a press on the input or the pin does not arm the drag window' runner=vitest

case_ "launcher: a press off the handle leaves an earlier arming in place" \
  ui/src/launcher/Launcher.svelte \
  's~handlePressedAt = onHandle \? Date\.now\(\) : -Infinity;~if (onHandle) handlePressedAt = Date.now();~' \
  'if (onHandle) handlePressedAt = Date.now();' \
  src/launcher/Launcher.test.ts 'a press off the handle disarms an earlier arming even with no release' runner=vitest

case_ "launcher: a secondary-button press arms the drag window" \
  ui/src/launcher/Launcher.svelte \
  's~const onHandle = !event\.button && !!target~const onHandle = !!target~' \
  "const onHandle = !!target?.closest('.searchbar')" \
  src/launcher/Launcher.test.ts 'a right-button press on the handle does not arm the drag window' runner=vitest

case_ "launcher: a release does not disarm the drag window" \
  ui/src/launcher/Launcher.svelte \
  's~function onPointerUp\(\) \{ handlePressedAt = -Infinity; \}~function onPointerUp() { void handlePressedAt; }~' \
  'function onPointerUp() { void handlePressedAt; }' \
  src/launcher/Launcher.test.ts 'a release after the press disarms the drag window: a click on the handle, then a blur, hides' runner=vitest
