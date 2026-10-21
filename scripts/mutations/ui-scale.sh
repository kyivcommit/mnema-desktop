# The interface scale — `src-tauri/src/ui_scale.rs`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/ui-scale.sh
#
# What is here, by name:
#
#   the fallback        — an unknown or missing `uiScale` reads as Medium, the
#                         step that changes nothing, not as whichever arm a
#                         refactor puts first
#   the layout ceiling  — Large stays at 1.15; the launcher needs 1240 CSS px
#                         for both side panels and the configured window is
#                         1440 device px, so 1.2 would hide the conversations
#                         panel on a fresh install (1.15 leaves 1252)
#   the stored spelling — the persisted string is the wire contract, not a
#                         synonym: `"large"` is what a later build reads back
#
# Not here, and why: the per-window `set_zoom` loop is unobservable under the
# mock runtime — its `set_zoom` is `Ok(())` and records nothing — and the
# persist→apply lock has no test yet (recorded in its own comment). The live
# run is the check for both.

case_ "ui_scale: an unknown stored value must fall back to Medium, not to Large" \
  src-tauri/src/ui_scale.rs \
  's~        _ => UiScale::Medium, // "medium" and anything unknown~        _ => UiScale::Large, // mutant: unknown becomes large~' \
  '_ => UiScale::Large, // mutant: unknown becomes large' \
  mnema-desktop 'ui_scale::tests::every_stored_string_is_understood_and_anything_else_is_medium' --lib

case_ "ui_scale: Large must stay under the launcher's own layout threshold" \
  src-tauri/src/ui_scale.rs \
  's~            UiScale::Large => 1.15,~            UiScale::Large => 1.2, // mutant: hides the panel~' \
  'UiScale::Large => 1.2, // mutant: hides the panel' \
  mnema-desktop 'ui_scale::tests::the_three_factors_are_the_contract' --lib

case_ "ui_scale: the stored spelling is the wire contract, not a synonym" \
  src-tauri/src/ui_scale.rs \
  's~        UiScale::Large => "large",~        UiScale::Large => "big", // mutant: a silent synonym~' \
  'UiScale::Large => "big", // mutant: a silent synonym' \
  mnema-desktop 'ui_scale::tests::every_choice_round_trips_through_the_file' --lib
