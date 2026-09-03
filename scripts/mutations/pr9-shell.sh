# The preferences file — `src-tauri/src/prefs.rs`, the one place the app reads
# and writes `prefs.json` from PR 9 onward. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9-shell.sh
#
# What is here, and it is two different kinds of claim. Some cases pin
# behaviour that already existed while `locale.rs` owned the file and that the
# extraction had to carry across unchanged — the merge, and the temp-file-plus-
# rename. Those are the ones judged by a `locale::tests::` name below, and that
# is deliberate: their oracle is the locale's OWN tests, unedited, because a
# refactor whose evidence is a test the refactor also wrote has no evidence.
# The rest pin what the extraction ADDED and nothing before it had: the
# process-wide lock, the backup of a malformed file, the unique temp name and
# its removal on a failed write. Which case is which is re-derivable, so no
# list of them is kept here:
#
#   grep -oE "mnema-(desktop|walk) 'locale::[^']+'" scripts/mutations/pr9-shell.sh
#
# No count of the cases here, deliberately — `pr8-exclusions.sh` explains what
# that number costs when it goes stale. Re-derive:
#
#   grep -c '^case_ ' scripts/mutations/pr9-shell.sh                     # cases
#   grep -oE "mnema-desktop '[^']+'" scripts/mutations/pr9-shell.sh | sort -u
#
# ⚠️ **Two test shapes, and the selector is what tells them apart.** The `--lib`
# cases name unit tests: `prefs.rs` and `locale.rs` both keep theirs in their own
# `mod tests`, so those names carry the `prefs::tests::` / `locale::tests::`
# prefix, and a name without it selects nothing — the harness then reports a
# BASELINE FAILURE for the whole file rather than a result. The `--test commands`
# cases name integration tests in `src-tauri/tests/commands.rs`, which sit at the
# top level of that file and carry NO prefix. Getting the two the wrong way round
# fails the same way. Re-derive which is which:
#
#   grep -oE "mnema-desktop '''[^''']+''' --(lib|test [a-z_]+)" scripts/mutations/pr9-shell.sh
#
# ⚠️ At least one case here names a `#[cfg(unix)]` test. Both of this
# repository's CI legs are unix; a Windows leg would fail this file's baseline
# WHOLE — every case in it, before any mutation runs — the way
# `pr8-exclusions-macos.sh` had to be split off for. Which tests those are is
# taken from the code rather than from this sentence, which would go stale:
#
#   for t in $(grep -oE "mnema-desktop '[^']+'" scripts/mutations/pr9-shell.sh \
#               | sed "s/.*'\(.*\)'/\1/" | sed 's/.*:://' | sort -u); do
#     grep -rn -B3 "fn ${t}(" src-tauri | grep -q 'cfg(unix)' && echo "$t"
#   done

# The merge itself, removed: `write_key` builds a fresh object instead of the
# one already in the file. Every key anybody else wrote is then dropped by the
# next write of any single key — the hotkey erases the locale and the locale
# erases the hotkey — while the file stays valid JSON holding exactly what was
# last written, which is why nothing but a two-key fixture can see it.
case_ "a key already in the file must survive a write of a different key" \
  src-tauri/src/prefs.rs \
  's~        Some\(Ok\(object\)\) => object,~        Some(Ok(_)) => serde_json::Map::new(),~' \
  'Some(Ok(_)) => serde_json::Map::new(),' \
  mnema-desktop 'prefs::tests::a_new_key_joins_the_one_already_in_the_file' --lib

# The same mutation, judged by the oracle the refactor did not write. This is
# the case that says the extraction is behaviour-preserving rather than merely
# self-consistent: `write_preserves_unknown_fields` was written against the
# code that used to live in `locale.rs`, has not been edited, and still dies
# when the merge it was written for goes away.
case_ "the locale's own forward-safety test must still see the merge disappear" \
  src-tauri/src/prefs.rs \
  's~        Some\(Ok\(object\)\) => object,~        Some(Ok(_)) => serde_json::Map::new(),~' \
  'Some(Ok(_)) => serde_json::Map::new(),' \
  mnema-desktop 'locale::tests::write_preserves_unknown_fields' --lib

# Temp-file-plus-rename collapsed to a write straight at the target. A write
# that fails part-way then leaves a truncated file where the preferences were:
# in a read-only directory the sibling temp file cannot be created at all, but
# the existing file is still writable through its own mode, so the mutant
# truncates it and reports success. Again judged by the locale's own unedited
# test, which is what that test was written for.
case_ "a failed write must not be able to destroy what was already persisted" \
  src-tauri/src/prefs.rs \
  's~    let tmp = path\.with_extension\(temp_extension\(\)\);\n(?:    //[^\n]*\n)+    if let Err\(e\) = std::fs::write\(&tmp, &body\) \{\n(?:.*?\n)+?    Ok\(\(\)\)~    std::fs::write(\&path, \&body)~' \
  'std::fs::write(&path, &body)' \
  mnema-desktop 'locale::tests::failed_write_keeps_the_previous_choice' --lib

# `read_all`'s promise that nothing it meets is an error. A file that is not
# JSON, or is JSON of the wrong shape, then takes the process down — and the
# first caller of this function is start-up, before there is anywhere to report
# anything to. The named test's first assertion is what sees it.
case_ "a malformed file must read as no preferences, not as a panic" \
  src-tauri/src/prefs.rs \
  's~    serde_json::from_slice\(&bytes\)\.unwrap_or_default\(\)~    serde_json::from_slice(\&bytes).unwrap()~' \
  'serde_json::from_slice(&bytes).unwrap()' \
  mnema-desktop 'prefs::tests::a_malformed_file_is_kept_byte_for_byte_beside_the_one_that_replaces_it' --lib

# 🔴 The lock, gone. Everything still compiles, every other test in this file
# stays green, and two writers that interleave each write an object computed
# before the other's key was added — so the loser's key is simply not in the
# file, with nothing to say it ever was. This is the one fixture in the task
# that can see it: the named test parks the first writer INSIDE the critical
# section and watches the second one walk past. A "two threads, N writes each,
# then check nothing was lost" test would pass against this mutant whenever the
# interleaving happened not to occur.
case_ "the read-modify-write must be serialised, not merely usually uncontended" \
  src-tauri/src/prefs.rs \
  's~    let _guard = PREFS_LOCK\.lock\(\)\.unwrap_or_else\(\|e\| e\.into_inner\(\)\);~    let _guard = (); // mutant: the critical section is not serialised~' \
  'let _guard = (); // mutant: the critical section is not serialised' \
  mnema-desktop 'prefs::tests::a_second_writer_waits_for_the_first_to_leave_the_critical_section' --lib

# 🔴 The backup, skipped: a file that does not parse is simply overwritten. The
# malformed file may be the only copy of something a person hand-edited, or
# something a newer version wrote in a shape this build cannot read, and after
# this mutant it is gone with nothing recording that it existed. Nothing above
# can see this — the file it replaces it with is byte-identical either way; it
# is the `.corrupt` sibling's contents, asserted byte-for-byte, that dies.
case_ "a malformed file must be kept before it is replaced, not destroyed" \
  src-tauri/src/prefs.rs \
  's~        Some\(Err\(_\)\) => \{\n            replace_file\(&path, &path\.with_extension\("json\.corrupt"\)\)\?;\n            serde_json::Map::new\(\)\n        \}~        Some(Err(_)) => serde_json::Map::new(), // mutant: the malformed file is overwritten~' \
  'Some(Err(_)) => serde_json::Map::new(), // mutant: the malformed file is overwritten' \
  mnema-desktop 'prefs::tests::a_malformed_file_is_kept_byte_for_byte_beside_the_one_that_replaces_it' --lib

# ── Task review, concern 1 ────────────────────────────────────────────────────
#
# 🔴 The read reverted from bytes to text, which is exactly what the body moved
# out of `locale.rs` used to do. `read_to_string` fails on a file that is not
# valid UTF-8, so `existing` is `None` and the file lands on the same arm as a
# file that DOES NOT EXIST: no backup, and the only copy of whatever wrote it is
# gone. Nothing else in this file can see it — measured by running the whole
# `--lib prefs` filter against this exact reversion, where thirteen tests stay
# green and only the one named below dies:
#
#   a file that is not text was replaced with no backup beside it
#
# It is the arm, not the parse, that this case is about: both readings answer an
# empty map from `read_all` and both write the same replacement file.
case_ "a preferences file that is not valid UTF-8 must be malformed, not absent" \
  src-tauri/src/prefs.rs \
  's~    let existing = std::fs::read\(&path\)\.ok\(\);\n    let parsed = existing\n        \.as_deref\(\)\n        \.map\(serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>\);~    let existing = std::fs::read_to_string(\&path).ok(); // mutant: unreadable text, not malformed bytes\n    let parsed = existing\n        .as_deref()\n        .map(serde_json::from_str::<serde_json::Map<String, serde_json::Value>>);~' \
  'let existing = std::fs::read_to_string(&path).ok(); // mutant: unreadable text, not malformed bytes' \
  mnema-desktop 'prefs::tests::a_file_of_invalid_utf8_is_backed_up_rather_than_overwritten' --lib

# ── Task review, fix round 1 ──────────────────────────────────────────────────
#
# Minor 3. The unique temp suffix, collapsed back to the one fixed name the
# plan replaced. Two writers then pick the same sibling to write into, and the
# uniqueness the plan asked for was until now a requirement nothing could
# observe: this mutant survived all seven cases above.
case_ "each writer must name its own temp file, not one shared name" \
  src-tauri/src/prefs.rs \
  's~fn temp_extension\(\) -> String \{\n    static SEQ: AtomicU64 = AtomicU64::new\(0\);\n    let n = SEQ\.fetch_add\(1, Ordering::Relaxed\);\n    format!\("json\.\{\}\.\{n\}\.tmp", std::process::id\(\)\)\n\}~fn temp_extension() -> String \{\n    "json.tmp".to_string() // mutant: one fixed temp name for every writer\n\}~' \
  '"json.tmp".to_string() // mutant: one fixed temp name for every writer' \
  mnema-desktop 'prefs::tests::each_call_names_a_different_temp_file' --lib

# Important 2. The temp file's removal on the rename's error path, taken back
# out — which is exactly what this task shipped in its first commit. The write
# still fails and still reports the failure, so nothing above notices; what it
# leaves behind is a `prefs.json.<pid>.<n>.tmp` that no later write will ever
# reuse or overwrite, one per failure, in the user's data directory forever.
#
# ⚠️ The sibling removal on the PARTIAL-WRITE path (a full disk, a killed
# process mid-`write`) is deliberately NOT a case here: this harness has no way
# to fail a `std::fs::write` after it has created the file, so a case for it
# could only be written against a test that does not exist. It is guarded by
# the same reading of the same one-line idiom, and by nothing executable.
case_ "a write that fails at the rename must take its temp file with it" \
  src-tauri/src/prefs.rs \
  's~    if let Err\(e\) = replace_file\(&tmp, &path\) \{\n        let _ = std::fs::remove_file\(&tmp\);\n        return Err\(e\);\n    \}\n    Ok\(\(\)\)~    replace_file(\&tmp, \&path) // mutant: the temp file is left behind on failure~' \
  'replace_file(&tmp, &path) // mutant: the temp file is left behind on failure' \
  mnema-desktop 'prefs::tests::a_write_that_fails_at_the_rename_leaves_no_temp_file_behind' --lib

# ── PR 9, Task 2: the hotkey lifecycle, autostart, and the three commands ─────
#
# ⚠️ Every case below names a test in `tests/commands.rs`, so the selector is
# `--test commands` and the name carries NO module prefix — unlike the
# `--lib` cases above, whose names are `prefs::tests::…`. The two shapes live in
# one file because they mutate one file; the selector is what tells them apart.
#
# ⚠️ One of them (`a_persist_failure_leaves_the_new_shortcut_registered_and_the_
# old_one_on_disk`) is `#[cfg(unix)]`, which the file header's warning already
# covers: a Windows leg would fail this file's baseline WHOLE.

# 🔴 The persist moved in front of the registration. On the success path nothing
# changes — the same value is written, twice — but a registration the operating
# system refuses is then already on disk, and the shortcut a person gets back
# tomorrow is one this application never managed to take. Only an assertion on
# the FILE can see it: the reply still reports the refusal either way.
case_ "a shortcut must not be persisted before the operating system has taken it" \
  src-tauri/src/prefs.rs \
  's~    let current = state\.hotkey\(\);\n    if current\.status == HotkeyStatus::Registered \{~    let current = state.hotkey();\n    // mutant: persisted before it is registered\n    write_key(state.data_dir(), HOTKEY_KEY, serde_json::Value::String(shortcut.clone()))?;\n    if current.status == HotkeyStatus::Registered \{~' \
  '// mutant: persisted before it is registered' \
  mnema-desktop 'a_failed_registration_restores_the_old_shortcut_and_leaves_prefs_untouched' --test commands

# 🔴 The best-effort restoration, deleted. The old shortcut was given up in step
# 4 and the new one was refused in step 5, so after this mutant the application
# holds NOTHING and never asks for anything back — the person's shortcut is gone
# and no message says so. The reply is byte-identical to the correct one, which
# is why only the recorded sequence can see it: its last entry is the
# re-registration.
case_ "a refused registration must hand the old shortcut back" \
  src-tauri/src/prefs.rs \
  's~        if let Err\(restoring\) = state\.with_shortcuts\(\|r\| r\.register\(&current\.shortcut\)\) \{\n            state\.set_hotkey_state\(HotkeyState \{\n                shortcut: current\.shortcut,\n                status: HotkeyStatus::Unavailable \{ reason: restoring \},\n            \}\);\n        \}~        // mutant: the old shortcut is never taken back~' \
  '// mutant: the old shortcut is never taken back' \
  mnema-desktop 'a_failed_registration_restores_the_old_shortcut_and_leaves_prefs_untouched' --test commands

# 🔴 The unregister made unconditional. D-b step 4 is "only if its status is
# `Registered`", and from an `Unavailable` start there is nothing the operating
# system holds — so this mutant asks it to give up a shortcut it never took.
# Every fixture that starts from `Registered` survives it; the one that starts
# where the application actually starts records two calls instead of one.
case_ "the old shortcut is given up only when the operating system holds it" \
  src-tauri/src/prefs.rs \
  's~    if current\.status == HotkeyStatus::Registered \{~    if true \{ // mutant: unregister whatever the status says~' \
  'if true { // mutant: unregister whatever the status says' \
  mnema-desktop 'set_hotkey_from_an_unavailable_start_registers_and_never_unregisters' --test commands

# 🔴 The no-modifier guard, deleted. The library ACCEPTS a bare `Space` — a
# single token parses to `Ok` with empty modifiers
# (`global-hotkey-0.8.0/src/hotkey.rs:174-178`) — so after this mutant a person
# can bind the space bar and lose it in every application on the machine. There
# is no error anywhere; the command answers `Ok`.
case_ "a shortcut with no modifier at all must be refused" \
  src-tauri/src/prefs.rs \
  's~    if parsed\.mods\.is_empty\(\) \{\n        return Err\(Error::HotkeyRefused\(\n            crate::locale::t\(lang, crate::locale::Key::HotkeyNeedsAModifier\)\.to_string\(\),\n        \)\);\n    \}~    let _ = parsed; // mutant: a shortcut with no modifier is accepted~' \
  'let _ = parsed; // mutant: a shortcut with no modifier is accepted' \
  mnema-desktop 'set_hotkey_refuses_a_key_with_no_modifier' --test commands

# 🔴 The modifier-only guard, deleted, judged by the single-token press. `"Alt"`
# is still refused — by the PARSER, as `UnsupportedKey`, whose sentence
# (`hotkey.rs:40`) asks the reader to report the string to
# `github.com/tauri-apps/muda`. So the mutant does not let a bad shortcut
# through; it hands somebody who held one modifier down a request to file a bug
# report against a library they have never heard of. The test asserts both
# halves: our sentence present, that URL absent.
case_ "one modifier with no key is refused in our own words, not the parser's" \
  src-tauri/src/prefs.rs \
  's~    if names_no_key\(&shortcut\) \{\n        return Err\(Error::HotkeyRefused\(\n            crate::locale::t\(lang, crate::locale::Key::HotkeyNeedsAKey\)\.to_string\(\),\n        \)\);\n    \}~    // mutant: an empty or modifier-only string falls through to the parser~' \
  '// mutant: an empty or modifier-only string falls through to the parser' \
  mnema-desktop 'set_hotkey_refuses_one_modifier_with_no_key' --test commands

# The same mutation, judged by the empty string — a different token shape
# through the same guard, and the one a window sends when a recorder is cleared.
case_ "the empty string is refused in our own words too" \
  src-tauri/src/prefs.rs \
  's~    if names_no_key\(&shortcut\) \{\n        return Err\(Error::HotkeyRefused\(\n            crate::locale::t\(lang, crate::locale::Key::HotkeyNeedsAKey\)\.to_string\(\),\n        \)\);\n    \}~    // mutant: an empty or modifier-only string falls through to the parser~' \
  '// mutant: an empty or modifier-only string falls through to the parser' \
  mnema-desktop 'set_hotkey_refuses_the_empty_string' --test commands

# 🔴 The guard narrowed to a SINGLE token, which is the shape an implementer
# reaches for first and the one that misses the commoner press. `"Ctrl+Alt"` —
# two modifiers held with no key — does not take the single-token path at all:
# it reaches `key.ok_or_else(…)` (`global-hotkey-0.8.0/src/hotkey.rs:229`) and
# comes back as `InvalidFormat`, a third sentence with a third shape. Every
# other refusal fixture in this task survives this mutant; measured by running
# the whole `--test commands` filter against it, where only the case below dies.
case_ "two modifiers with no key are refused by the same guard as one" \
  src-tauri/src/prefs.rs \
  's~    shortcut\.trim\(\)\.is_empty\(\) \|\| shortcut\.split\(.\+.\)\.all\(is_modifier_token\)~    shortcut.trim().is_empty() || is_modifier_token(shortcut) // mutant: only a single token is refused~' \
  'is_modifier_token(shortcut) // mutant: only a single token is refused' \
  mnema-desktop 'set_hotkey_refuses_two_modifiers_with_no_key' --test commands

# 🔴 The answer echoed from the request instead of read back from the operating
# system. Everything looks right: the switch moves, the reply agrees with it,
# and the machine does not launch the application at login. The fixture that
# sees it is the one whose fake DISAGREES with what it was told to do — without
# that state both implementations pass.
case_ "autostart is what the operating system says, not what was asked for" \
  src-tauri/src/prefs.rs \
  's~    Ok\(read_autostart\(&state\)\)~    Ok(if enabled \{ AutostartState::Enabled \} else \{ AutostartState::Disabled \}) // mutant: the request is echoed~' \
  '// mutant: the request is echoed' \
  mnema-desktop 'set_autostart_reports_what_the_operating_system_says_not_what_was_asked' --test commands

# 🔴 Step 6's two lines swapped: the persist first, the state only if it
# succeeded. The operating system is then holding the NEW shortcut while the
# window is told the old one is bound — the state stops reporting the only fact
# it is entitled to report. Only the read-only-directory fixture can see it,
# because it is the only one where the persist fails at all.
case_ "the state is written before the persist, not after it" \
  src-tauri/src/prefs.rs \
  's~    state\.set_hotkey_state\(registered\.clone\(\)\);\n    write_key\(\n        state\.data_dir\(\),\n        HOTKEY_KEY,\n        serde_json::Value::String\(shortcut\),\n    \)\?;~    write_key(\n        state.data_dir(),\n        HOTKEY_KEY,\n        serde_json::Value::String(shortcut),\n    )?;\n    state.set_hotkey_state(registered.clone()); // mutant: the state lands only after the persist~' \
  'state.set_hotkey_state(registered.clone()); // mutant: the state lands only after the persist' \
  mnema-desktop 'a_persist_failure_leaves_the_new_shortcut_registered_and_the_old_one_on_disk' --test commands

# 🔴 A failed `unregister(old)` ignored, and the new shortcut taken anyway. The
# operating system then holds BOTH, and one press fires the launcher twice —
# and the state records only the new one, so nothing will ever give the old one
# back. The reply is an `Ok` where the correct code refuses, but the assertion
# that discriminates is the sequence's length: exactly two entries, no third.
case_ "a failed unregister stops the change rather than adding a second shortcut" \
  src-tauri/src/prefs.rs \
  's~        state\n            \.with_shortcuts\(\|r\| r\.unregister\(&current\.shortcut\)\)\n            \.map_err\(Error::HotkeyUnavailable\)\?;~        let _ = state.with_shortcuts(\|r\| r.unregister(\&current.shortcut)); // mutant: a failed unregister is ignored~' \
  '// mutant: a failed unregister is ignored' \
  mnema-desktop 'a_failed_unregister_stops_before_the_new_shortcut_is_ever_attempted' --test commands

# 🔴 The double-failure row, and the state left claiming `Registered` while the
# operating system holds nothing at all. The reply is identical to the correct
# one — it carries the NEW shortcut's sentence either way — so the only fixture
# that can see this is the one that asks `app_prefs` afterwards. Every other
# refusal fixture in this task survives it.
case_ "a restoration that also failed must not leave the state claiming Registered" \
  src-tauri/src/prefs.rs \
  's~            state\.set_hotkey_state\(HotkeyState \{\n                shortcut: current\.shortcut,\n                status: HotkeyStatus::Unavailable \{ reason: restoring \},\n            \}\);~            let _ = restoring; // mutant: the status stays Registered with nothing behind it~' \
  'let _ = restoring; // mutant: the status stays Registered with nothing behind it' \
  mnema-desktop 'a_failed_restoration_leaves_the_state_unavailable_rather_than_registered' --test commands

# 🔴 The boot made fatal by a failed registration — the D128 defect moving house
# out of the plugin builder and into `.setup`. A shortcut another application
# already holds would once again be a reason this one does not start, and a
# person who cannot start it cannot be told why.
#
# ⚠️ **Not a literal `?`, and that is not a shortcut taken.** `install_hotkey`
# returns `HotkeyState`, not a `Result`, so `?` does not compile — and a mutant
# that will not build is a broken baseline this harness reports as a gate, not a
# guard that held. `.expect(…)` is the shape that compiles and is what "fatal"
# means at a boot with nowhere to report to. The named fixture calls
# `install_hotkey` DIRECTLY, which is the whole reason the boot is a free
# function: inline in `.setup` it would be reachable from no test here, and this
# case could not exist.
case_ "a registration the operating system refuses must not be fatal at start-up" \
  src-tauri/src/prefs.rs \
  's~    let status = match state\.with_shortcuts\(\|r\| r\.register\(&shortcut\)\) \{\n        Ok\(\(\)\) => HotkeyStatus::Registered,\n        Err\(reason\) => HotkeyStatus::Unavailable \{ reason \},\n    \};~    let status = state\n        .with_shortcuts(\|r\| r.register(\&shortcut))\n        .map(\|()\| HotkeyStatus::Registered)\n        .expect("mutant: a failed boot registration is fatal");~' \
  '.expect("mutant: a failed boot registration is fatal");' \
  mnema-desktop 'a_stored_hotkey_that_no_longer_parses_boots_without_a_registration' --test commands

# The other half of the stored-garbage pair: the fallback itself. A boot that
# handed the unparsable stored string to the operating system would ask for a
# shortcut nobody can press, and the recorded sequence is what says it never
# does.
case_ "an unparsable stored shortcut falls back and is never handed to the OS" \
  src-tauri/src/prefs.rs \
  's~        Some\(s\) if !names_no_key\(&s\) && s\.parse::<Shortcut>\(\)\.is_ok\(\) => s,~        Some(s) => s, // mutant: whatever is stored is registered as it stands~' \
  'Some(s) => s, // mutant: whatever is stored is registered as it stands' \
  mnema-desktop 'a_stored_hotkey_that_no_longer_parses_registers_the_default_and_never_the_garbage' --test commands
