#!/usr/bin/env bash
#
# Break one thing, run one test, require it to go red.
#
# Why this is in the repository rather than in whoever's scratch directory:
# this project has already had two mutation runs that silently applied nothing
# and reported it as a result. Both were caught by luck. The guard that catches
# it — `git diff --quiet`, below — is the only part of the harness that matters,
# and a guard nobody else can run is a guard nobody else can check.
#
# The specific failure it exists to stop: a marker checked with `grep -F` and a
# multi-line pattern matches when ANY single line of the pattern is present. A
# mutation that changed nothing therefore passed its own "did it apply" check,
# and the test that stayed green was written up as "this test protects nothing".
# Comparing the file against git cannot be fooled that way.
#
# Usage:
#   scripts/mutation-check.sh <case-file>
#
# A case file is a series of `case` calls; see scripts/mutations/task-8.sh.
# Everything runs in a throwaway git worktree at HEAD with its own
# CARGO_TARGET_DIR, so an interrupted run cannot leave a mutation in the tree
# you are working in.
#
# ── Two runners, and why there had to be a second ─────────────────────────────
#
# For most of this file's life it ran `cargo test` and nothing else, which made
# it blind to every guard in `ui/`. That is not a gap in coverage, it is a
# **false statement in the gate**: the line PR 8a's task reports quote as their
# evidence, `651 cases, stale 0`, was true of `crates/` and `src-tauri/` and
# said nothing about the Svelte components, so that number would have stayed
# green on the day a UI guard was weakened. PR 8a's Tasks 5, 6 and 7 backed thirty-odd UI guards with hand
# reverts written into task reports — evidence in a place no later change can
# trip over. This is that evidence moved somewhere a script can re-run it.
#
# A case names its runner as an optional field straight after the test name:
#
#   case_ "…" "src-tauri/src/tree.rs" 's/…/…/' "…" \
#     mnema-desktop the_test_name --lib          # cargo, the default
#
#   case_ "…" "ui/src/settings/Folders.svelte" 's/…/…/' "…" \
#     src/settings/Folders.test.ts 'the whole test name' runner=vitest
#
# For `cargo` the fifth field is the package and the trailing arguments are
# cargo's own target selectors. For `vitest` the fifth field is the test FILE,
# relative to `ui/`, the sixth is the whole test name, and nothing else is
# accepted. Every case written before there was a second runner keeps meaning
# exactly what it said, which is why the field is optional and positioned
# where an absent one costs nothing.
#
# ⚠️ **The vitest half needs `ui/node_modules`, and it is gitignored** — so the
# worktree has none and it is linked in below, the same problem `vendor/` and
# the staged sidecar already have and for the same reason: a baseline that
# fails because a dependency is missing is a red that is not about the
# mutation.
#
# 🔴 **A crashed oracle is not a kill, and vitest has this failure where cargo
# does not.** A mutant that makes a Svelte render throw does not fail the test —
# it kills the run that was supposed to judge it. Vitest reports that as
#
#      Tests  62 passed (62)
#     Errors  1 error
#
# and exits **1**. Three readers on this branch took that summary for a passing
# run; read as an exit code alone it is indistinguishable from a kill, and
# counting it as one would record "this test protects that line" about a test
# that never saw the mutant. So the vitest branch below never reads the exit
# status. It reads the printed counts, and an `Errors` line is a BROKEN CASE —
# the same verdict a mutation that does not compile gets, because it is the same
# fact: the test did not run against the mutation. Measured, at c12fb9d:
# deleting `patch`'s early return (`ui/src/settings/Folders.svelte:126`) is
# exactly this shape, and `scripts/mutations/pr8-ui-folders.sh` mutates that
# guard a second way instead — a fresh panel rather than a deleted line — which
# renders, so the oracle survives to answer, and it answers red.
#
# **Its cheap sibling: `scripts/mutation-staleness.sh <case-file>`.** This script
# answers "does the test still go red" and takes 3:17 on an Apple M2 Max, cold
# `CARGO_TARGET_DIR`, 80 cases over 69 baseline tests — measured with `time`
# around the whole invocation. `236% cpu` in that same measurement means this
# harness is very nearly serial, about two and a half cores busy out of
# twelve, so core count is not the axis a CI runner loses on. What this does
# not transfer to: a 2-core `ubuntu-24.04` runner with a cold target directory
# of its own is not this machine. **Measured there now, on the first pull
# request this leg ever ran on: 6m17s, green.** So the runner costs roughly
# twice this machine and not the ten times a core count would suggest — which
# is what "nearly serial" predicted, and the prediction is now checked rather
# than argued. `ci.yml`'s `timeout-minutes: 30` is about five times that,
# which is the margin `check` justifies for itself in the same file. That one
# answers "do the cases still apply, and do they still produce what they
# claim" and takes about a second, so it is the one to run after a refactor.
# It is not a substitute — it compiles nothing and runs no test — but it
# catches the thing 3:17 here does not: a case quoting code that has moved.
# Measured on this branch, one four-column re-indentation broke three cases
# and this script reported them one run apart, because a `perl` pattern's
# leading spaces are not anchored and one of the three had been substituting
# into the middle of an indent, passing both guards for a reason unrelated to
# its meaning.
#
# ⚠️ **A third guard, for the same failure, that the two above cannot see.**
# `s///` without `/g` stops at the first match and returns 1 whether that match
# was the one the case meant or a different place with the same shape — it
# cannot tell "matched correctly" from "matched a wrong place first, which
# happened to be byte-identical". So this guard counts occurrences separately,
# on a copy nothing else reads, by forcing `/g` onto whatever the expression
# already has (a no-op if it already carries one): no `g` requires exactly one
# occurrence, `g` requires at least one. No exception list — the one
# legitimate many-match case (`linux-resource.sh`'s "no case arm sets a
# library any more") declares its own multiplicity in its own syntax. Applied
# for real against all 546 expressions this repository held then (a figure every
# run of `mutation-staleness.sh` re-derives and prints beside the files it read,
# so take it from there rather than from this sentence, which has already gone
# stale once), it found eight
# cases sharing a pattern with a sibling function or match arm — one of them
# the exact shape above, an unanchored 4-space pattern matching inside a
# 12-space-indented sibling as a substring — each narrowed to name its
# function or arm rather than loosened, and verified to still produce the
# byte-identical mutation it always had.
#
# ⚠️ **Read the exit code from the script, not from a pipeline.** `… | tail`
# reports `tail`'s status. Measured here: a run of this harness with two broken
# cases was written up as "exit code 0" for exactly that reason. Redirect and
# capture instead:
#
#   scripts/mutation-check.sh scripts/mutations/embedding.sh > out.txt 2>&1
#   echo "EXIT=$?"

# No `set -e`, deliberately: nearly every command in this script is expected to
# fail — that is what a red mutation is — and `-e` would abort the run on the
# first one. Failures are classified explicitly instead.
set -uo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <case-file>" >&2
  exit 2
fi

CASES=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
REPO=$(git rev-parse --show-toplevel)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/mnema-mutation.XXXXXX")
TREE="$WORK/tree"
export CARGO_TARGET_DIR="$WORK/target"

cleanup() {
  git -C "$REPO" worktree remove --force "$TREE" >/dev/null 2>&1
  rm -rf "$WORK"
  # The removal above can fail and the `rm -rf` then destroys the tree anyway,
  # leaving a stale entry under .git/worktrees that makes the next run's
  # `worktree add` fail on a name it cannot see. Prune, always.
  git -C "$REPO" worktree prune
}
trap cleanup EXIT

git -C "$REPO" worktree add -q --detach "$TREE" HEAD || exit 1
# Vendored native libraries are gitignored, so the worktree has none. Tests that
# need them would otherwise fail for a reason that has nothing to do with the
# mutation. `vendor/` also holds the Pdfium the bundle now ships, which
# `bundle.resources` makes a compile-time requirement of src-tauri and not only
# a run-time one.
[ -d "$REPO/vendor" ] && cp -R "$REPO/vendor" "$TREE/vendor"
# And the staged sidecar, for the same reason and with a sharper edge: it is
# gitignored too, and `tauri-build` refuses to compile src-tauri at all without
# it — `resource path binaries/mnema-extract-worker-<triple> doesn't exist`.
#
# Read what that cost carefully, because the obvious reading is too small. The
# baseline pass below counts DISTINCT TESTS, not cases, and 23 of task-8.sh's,
# 3 of task-9.sh's and 2 of branch-review.sh's live in `mnema-desktop`. But a
# non-zero `baseline_bad` exits 1 for the WHOLE FILE before pass two starts, so
# what could not run was not those tests' cases — it was every case in each of
# those files: 34 + 13 + 11 = **58 cases, three files, zero mutations executed**,
# from the day `externalBin` landed (fb3a924) until this line was written.
# `branch-review.sh` is the whole-branch file, the one run before a merge; it
# had never run at all. All of it was reported honestly as "refusing to mutate
# against N test(s) that are not green to begin with", and read by nobody.
#
# Copied rather than rebuilt — the tests that need it read the workflow and the
# profile, not the binary, and `tauri-build` only checks the file is there.
[ -d "$REPO/src-tauri/binaries" ] \
  && cp -R "$REPO/src-tauri/binaries" "$TREE/src-tauri/binaries"

# And `ui/node_modules`, for the third time and the same reason: gitignored, so
# absent from the worktree, and a `runner=vitest` case would fail its baseline
# because the runner is not installed rather than because its test is not green.
#
# Linked per entry rather than as one symlink over the whole directory, and the
# exception is the point: vitest writes its transform cache to
# `node_modules/.vite`, so a single symlink would have this harness writing
# into the checkout it promises not to touch — and sharing that cache with
# whatever `npm test` is running next door. A real directory holding links to
# each package leaves `.vite` inside the worktree, where the trap deletes it.
# 88 MB linked, not copied; measured cost of a `vitest run` of one named test
# in a fresh worktree this way: 1.2s.
if [ -d "$REPO/ui/node_modules" ]; then
  mkdir -p "$TREE/ui/node_modules"
  shopt -s dotglob nullglob
  for dep in "$REPO"/ui/node_modules/*; do
    depname=$(basename "$dep")
    [ "$depname" = ".vite" ] && continue
    ln -s "$dep" "$TREE/ui/node_modules/$depname"
  done
  shopt -u dotglob nullglob
fi
VITEST="$TREE/ui/node_modules/.bin/vitest"

cd "$TREE" || exit 1

red=0
green=0
broken=0
# N6: counted the same way `mutation-staleness.sh` counts it, and printed
# beside `red`/`green`/`broken` for the same reason — `/g` is a self-declaring
# opt-out from guard 3, and an opt-out nobody counts is one nobody would
# notice growing.
every_match_count=0

restore() { git -C "$TREE" checkout -q -- . ; }

# Exact multi-line substring test. NOT `grep -F`: given a pattern containing a
# newline, grep splits the PATTERN and matches if ANY one of its lines occurs
# anywhere in the file. That is what let a mutation which changed nothing pass
# its own "did it apply" check, and `-z` does not help — it changes how the input
# is split, not the pattern. Measured before this was written.
contains() {
  MUTATION_MARKER="$1" perl -0777 -ne 'exit(index($_, $ENV{MUTATION_MARKER}) < 0)' "$2"
}

# Whether `expr`'s own trailing flags carry a `g` — "every arm" rather than
# "this one place". In `perl`, not bash's `[[ =~ ]]`: the obvious bash regex
# for "trailing run of letters", `([a-zA-Z]*)$`, matched empty on this
# platform's bash even against a string that plainly ends in letters —
# measured, not assumed.
#
# 🔴 **Judged from the expression's FIRST LINE (fix round 2, B8, disclosure).**
# `perl -ne` runs this block once per line and `exit`s on the first, so a
# `case_` whose perl expression spans several physical lines is classified by
# the trailing characters of line one — which are usually mid-pattern, not the
# flags. Not a defect today, and re-derivable rather than remembered: sourcing
# every case file with a recording `case_` gives 667 expressions, 8 of them
# multi-line, and for each of those 8 the first line and the last agree (all
# eight: no `/g`). It becomes a defect the day a multi-line expression carries
# `/g` on its closing line: guard 3 would then demand exactly one occurrence of
# a pattern written to match several, and report a BROKEN CASE about a case
# that is fine.
#
# To re-derive, write a two-line recorder and source every case file with it:
#
#   case_() { printf '%s\\0' "$3"; }
#   for f in scripts/mutations/*.sh; do . "$f"; done
#
# then split the output on NUL and compare, for each expression containing a
# newline, the trailing letters of its first line against those of its last.
# ⚠️ Run it with `< /dev/null`: one case file's expression otherwise consumes
# the recorder's stdin and the sweep never finishes — measured, twice.
expr_wants_every_match() {
  printf '%s' "$1" | perl -ne 'exit(/([a-zA-Z]*)$/ && $1 =~ /g/ ? 0 : 1)'
}

seen=""

# ── The vitest runner's reading of its own output ─────────────────────────────
#
# Vitest says how a run went in two summary lines and NOT in its exit status,
# which is 1 for a failed assertion and 1 for a mutant that threw before any
# assertion could run. Both are parsed, and every verdict below is drawn from
# the counts rather than from `$?`.
#
#      Tests  1 failed | 61 skipped (62)
#     Errors  1 error
#
# `tail -1` because the last such line is the summary; a test that printed the
# word itself would be picked up otherwise. The `Tests` pattern deliberately
# does NOT require a digit — `Tests  no tests` is a real state (a file that
# failed to collect) and has to reach the "nothing ran" verdict rather than the
# "vitest printed nothing" one, which means something different.
vitest_summary=""
vitest_errline=""
vitest_passed=0
vitest_failed=0
vitest_errors=0

# The count standing immediately before <word> in <line>, or 0 when there is
# none. `2 errors` answers a query for `error`, which is what is wanted.
vitest_count() {
  local n
  n=$(printf '%s' "$1" | grep -oE "[0-9]+ $2" | head -1 | grep -oE '^[0-9]+')
  printf '%s' "${n:-0}"
}

# 🔴 **Colour is stripped before anything is matched, and this is the whole
# reason this function was blind for a day.** Vitest writes its summary as
# `  \e[2m      Tests \e[22m \e[1m\e[32m1 passed\e[39m…`, so every pattern
# below anchored with `^[[:space:]]*Tests` fails: an escape sequence sits
# between the indent and the word. Locally the output is not a terminal, the
# colour is off, and every pattern matches — in GitHub Actions the colour is
# ON, and this harness reported eleven green tests as "vitest printed no
# summary" and refused to mutate against any of them.
#
# The failure direction was the safe one — it refused rather than scoring a
# mutant it had not measured — but a parser that reads the product's output
# only in the configuration the author happened to run it in is not a parser.
#
# ⚠️ `NO_COLOR=1` is also set where vitest is invoked, and that is hygiene, not
# the guarantee: it is a request to a third-party library that any of its
# dependencies may stop honouring. This line is what this script can promise.
#
# 🔴 **Both were measured ALONE, and each is enough on its own** — which is
# exactly why the pair is a trap for the next reader. With the colour forced on
# (`FORCE_COLOR=1 bash scripts/mutation-check.sh <case file>`, which reproduces
# GitHub Actions in one command), removing `NO_COLOR=1` leaves the run green
# through this strip, and removing this strip leaves it green through
# `NO_COLOR=1`. So deleting either one changes nothing anybody would notice,
# and deleting both brings the whole vitest half back to "printed no summary".
# **Neither is pinned by a case**: a case would have to set `FORCE_COLOR` for
# the run it measures, and this harness deliberately does not reach into the
# environment of the runner it drives. Re-check by hand with the command above.
vitest_read() {
  local plain
  plain=$(printf '%s' "$1" | sed $'s/\033\[[0-9;]*m//g')
  vitest_summary=$(printf '%s' "$plain" | grep -E '^[[:space:]]*Tests[[:space:]]' | tail -1 | sed 's/^[[:space:]]*//')
  vitest_errline=$(printf '%s' "$plain" | grep -E '^[[:space:]]*Errors[[:space:]]+[0-9]+ error' | tail -1 | sed 's/^[[:space:]]*//')
  vitest_passed=$(vitest_count "$vitest_summary" passed)
  vitest_failed=$(vitest_count "$vitest_summary" failed)
  vitest_errors=$(vitest_count "$vitest_errline" error)
}

# Runs the case's named test, unmutated or mutated according to the tree it is
# called against, and prints everything the runner said.
#
# `--reporter=default` is pinned rather than left to the configuration: every
# verdict above is a parse of that reporter's two summary lines, and a
# `reporters` key added to `ui/vite.config.ts` would otherwise change what this
# function is reading without changing a line of this script.
#
# ⚠️ `-t` is a REGULAR EXPRESSION, not a literal — a test name carrying `(`,
# `[` or `?` will select something other than itself. Nothing here escapes it,
# because the baseline pass already refuses anything that does not select
# exactly one passing test, in either direction: a name that matches nothing,
# and a name that matches two.
run_named_test() {
  case "$runner" in
    cargo)
      cargo test -p "$pkg" "$@" -- --exact "$test" 2>&1
      ;;
    vitest)
      if [ ! -x "$VITEST" ]; then
        echo "no vitest in the worktree — $REPO/ui/node_modules has no .bin/vitest."
        echo "Run \`npm install\` in ui/; this harness links that directory, it does not install."
        return 127
      fi
      # `NO_COLOR=1`: see the note above `vitest_read`. Without it the summary
      # arrives wrapped in escape sequences on any runner that forces colour,
      # which GitHub Actions does and a local shell does not.
      ( cd "$TREE/ui" && NO_COLOR=1 "$VITEST" run "$pkg" --reporter=default -t "$test" ) 2>&1
      ;;
  esac
}

# case_ <label> <file> <perl-expr> <marker> <target> <test-name> [runner=<name>] [runner args...]
#
# `<target>` and the trailing arguments belong to the runner: for `cargo` (the
# default) the package and cargo's own target selectors, for `vitest` the test
# file relative to `ui/` and nothing else. See the header.
#
# Runs in two passes over the same case file. `$mode` selects which.
case_() {
  local label="$1" file="$2" expr="$3" marker="$4" pkg="$5" test="$6"
  shift 6

  # Optional, and first if it is there at all — so every case written before
  # there was a second runner is unchanged and still means `cargo`.
  local runner=cargo
  case "${1-}" in
    runner=*) runner="${1#runner=}"; shift ;;
  esac

  # Both of these exit the whole run rather than counting a broken case: they
  # are errors in how the case file is WRITTEN, not results about the code, and
  # a miswritten case that merely increments a counter is one somebody reads
  # past. `mutation-staleness.sh` reads only the first four fields, so it can
  # say nothing about either — this is the only place they are checked.
  local arg
  for arg in "$@"; do
    case "$arg" in
      runner=*)
        echo "BROKEN CASE FILE: $label puts $arg after another argument. The runner has to come" >&2
        echo "straight after the test name, or it is passed to the runner as one of its own." >&2
        exit 2
        ;;
    esac
  done
  case "$runner" in
    cargo) ;;
    vitest)
      if [ $# -ne 0 ]; then
        echo "BROKEN CASE FILE: the vitest runner takes a test file and a test name and nothing" >&2
        echo "else; $label also passes: $*" >&2
        exit 2
      fi
      ;;
    *)
      echo "BROKEN CASE FILE: $label names an unknown runner '$runner'. Known: cargo, vitest." >&2
      exit 2
      ;;
  esac

  if [ "$mode" = baseline ]; then
    # A case naming a test that is already failing, or misspelled, would read as
    # a pass in the mutation pass below — the run would go red for a reason that
    # has nothing to do with the mutation. So every named test is run once
    # unmutated first and must be green. Deduplicated: several cases usually
    # target one test.
    local key="|$runner $pkg $* $test|"
    case "$seen" in *"$key"*) return 0 ;; esac
    seen="$seen$key"

    # `local out` on its own line for the reason given at the mutated pass
    # below: `local out=$(...)` makes `$?` the status of `local`, always 0.
    local out status
    out=$(run_named_test "$@")
    status=$?

    if [ "$runner" = cargo ]; then
      # `1 passed`, not merely exit zero. `--exact` with a name that matches
      # nothing runs no tests and exits 0, so a misspelled case would sail
      # through here and then be reported one step later as "the test does not
      # protect what it names" — a true-sounding verdict about a test that does
      # not exist.
      if printf '%s' "$out" | grep -q 'test result: ok\. 1 passed'; then
        baseline_ok=$((baseline_ok + 1))
      elif printf '%s' "$out" | grep -q 'test result: ok\. 0 passed'; then
        echo "BASELINE FAILURE: no test named $test — check the spelling in the case file"
        baseline_bad=$((baseline_bad + 1))
      else
        echo "BASELINE FAILURE: $test is not green before any mutation"
        printf '%s' "$out" | grep -E "panicked at|^error" | head -3 | sed 's/^/  /'
        baseline_bad=$((baseline_bad + 1))
      fi
      # Deterministic, for the reason given at the end of this function.
      return 0
    fi

    # The same requirement in vitest's own words, and it takes a count because
    # nothing else here distinguishes the three ways a run can be worthless.
    # `-t` with a name nobody wrote skips every test in the file and exits 0;
    # `-t` with a name that is a substring of two runs both, and a case that
    # cannot say which test protects the line is not evidence about either.
    vitest_read "$out"
    if [ -z "$vitest_summary" ]; then
      # 🔴 The exit status is printed HERE and nowhere else, and it is
      # diagnostic rather than a verdict — every verdict in this script is a
      # parse of the counts, for the reason written above `vitest_read`. But
      # "printed no summary" has three very different causes that the counts
      # cannot tell apart, and the status can: 1 is vitest reporting a problem,
      # 127 is a runner that is not there, and 137 is a process killed by the
      # kernel with nothing to say.
      #
      # 🔴 And the whole output, not `head -3`. It was `head -3` until a CI run
      # of this branch, where vitest printed its three-line banner and then
      # died: the banner IS three lines, so the diagnostic showed the banner
      # and hid whatever came after it. A truncated diagnostic is the same
      # defect this project keeps paying for — a count from a limited query
      # read as the whole answer — and it cost a day here.
      echo "BASELINE FAILURE: vitest printed no summary for $pkg — it never got as far as running"
      echo "  vitest exit status: $status"
      printf '%s\n' "$out" | sed 's/^/  /'
      baseline_bad=$((baseline_bad + 1))
    elif [ "$vitest_errors" -ne 0 ]; then
      # Before any mutation, so it is the test file's own problem and every
      # verdict this case could reach afterwards would be about that instead.
      echo "BASELINE FAILURE: $test throws outside its assertions before any mutation"
      echo "  $vitest_summary / $vitest_errline"
      baseline_bad=$((baseline_bad + 1))
    elif [ "$vitest_passed" -eq 1 ] && [ "$vitest_failed" -eq 0 ]; then
      baseline_ok=$((baseline_ok + 1))
    elif [ "$vitest_passed" -eq 0 ] && [ "$vitest_failed" -eq 0 ]; then
      echo "BASELINE FAILURE: no test named $test in $pkg — check the spelling in the case file"
      baseline_bad=$((baseline_bad + 1))
    elif [ "$vitest_passed" -gt 1 ]; then
      echo "BASELINE FAILURE: $test selects $vitest_passed tests in $pkg, not one — the name is"
      echo '  a regular expression matched as a substring, so name the test in full.'
      baseline_bad=$((baseline_bad + 1))
    else
      echo "BASELINE FAILURE: $test is not green before any mutation ($vitest_summary)"
      baseline_bad=$((baseline_bad + 1))
    fi
    # Deterministic, for the reason given at the end of this function.
    return 0
  fi

  printf '%s\n' "-- $label"
  restore
  cp "$file" "$WORK/count-copy"
  perl -0pi -e "$expr" "$file"

  # The authoritative guard. A mutation that left the file byte-for-byte
  # identical is a broken case, not a test result, and must never count as either.
  if git -C "$TREE" diff --quiet -- "$file"; then
    echo "   BROKEN CASE: the mutation left $file unchanged"
    broken=$((broken + 1)); restore; return 0
  fi
  # Second guard: it changed, but into what the case says? Redundant with the
  # first for detecting a no-op; kept because it also catches a perl expression
  # that matched somewhere unintended.
  if ! contains "$marker" "$file"; then
    echo "   BROKEN CASE: $file changed, but not into what the case describes"
    broken=$((broken + 1)); restore; return 0
  fi
  # Third guard — see the header for why counting occurrences needs `/g`
  # forced on a copy, rather than trusting what `$expr` itself returned.
  # `$WORK/count-copy` was saved above, before the real mutation touched
  # `$file`, so this counts against the same pre-mutation text the real
  # substitution just read.
  local forced="$expr"
  expr_wants_every_match "$expr" || forced="${expr}g"
  local occurrences
  occurrences=$(perl -0pi -e "my \$mnema_subs = do { $forced }; print STDERR ((\$mnema_subs) + 0);" "$WORK/count-copy" 2>&1 1>/dev/null)
  if expr_wants_every_match "$expr"; then
    every_match_count=$((every_match_count + 1))
    if [ "$occurrences" -lt 1 ]; then
      echo "   BROKEN CASE: the expression carries /g and should match at least once; it matched $occurrences times"
      broken=$((broken + 1)); restore; return 0
    fi
  elif [ "$occurrences" -ne 1 ]; then
    echo "   BROKEN CASE: the pattern matches $file $occurrences times, not exactly once — it may be"
    echo "   substituting into code it was not written against"
    broken=$((broken + 1)); restore; return 0
  fi

  # `local out` is deliberately on its own line: `local out=$(...)` would make
  # `$?` the exit status of `local`, which is always 0.
  local out status
  out=$(run_named_test "$@")
  status=$?

  if [ "$runner" = cargo ]; then
    # Checked before the status, because a mutation that does not compile also
    # exits non-zero. Counting that as red is the no-op defect wearing a
    # different hat: the test never ran, so it proved nothing.
    if printf '%s' "$out" | grep -qE 'error\[E[0-9]+\]|could not compile'; then
      echo "   BROKEN CASE: the mutation does not compile, so $test never ran"
      printf '%s' "$out" | grep -E 'error\[E[0-9]+\]|^error' | head -3 | sed 's/^/     /'
      broken=$((broken + 1))
    elif [ $status -ne 0 ]; then
      echo "   red"
      # `missing`/`unexpected` are here because an assertion that compares sets
      # puts its detail on continuation lines carrying none of the other words:
      # without them a corpus case prints its location and nothing about which
      # dimension diverged, which is most of what the case is for.
      printf '%s' "$out" | grep -E "panicked at|assertion|left:|right:|not found|missing|unexpected" | head -6 | sed 's/^/     /'
      red=$((red + 1))
    elif ! printf '%s' "$out" | grep -q 'test result: ok\. 1 passed'; then
      # 🔴 Fix round 2, B1. `1 passed`, not merely exit zero — and this is the
      # SAME check the baseline pass has made since it was written, twenty lines
      # above at `test result: ok. 0 passed`. This branch did not have it, so a
      # mutation that renamed the `#[test]` itself made `--exact` select
      # nothing, cargo print `running 0 tests` and exit 0, and this harness
      # announce that a test which had just been renamed out of existence "does
      # not protect what it names". Demonstrated by construction, not reasoned:
      # a throwaway case renaming `exclusions_come_back_sorted` printed exactly
      # that verdict before this line existed.
      #
      # 🔴 It is also the exact defect this file was extended to catch. The
      # vitest branch below has carried the same guard since round 7b (`after
      # the mutation nothing named $test ran`), so teaching the harness a second
      # runner gave the new half a defence the old half still lacked — the seam
      # covering half its sites, inside the instrument built against that class.
      # And the uncovered half is the one the plan's Global Constraints single
      # out by name: `--exact` with a name nobody wrote is how a guard in PR 7
      # passed while selecting nothing.
      #
      # Any binary saying `1 passed` is enough, for the baseline's reason: a
      # case with no `--test` selector runs every test binary in the package and
      # all but one of them report `0 passed`.
      echo "   BROKEN CASE: after the mutation nothing named $test ran"
      printf '%s' "$out" | grep -E 'running [0-9]+ test|test result:' | head -3 | sed 's/^/     /'
      broken=$((broken + 1))
    else
      echo "   *** STILL GREEN: $test does not protect what it names ***"
      green=$((green + 1))
    fi
  else
    # 🔴 `$status` is deliberately not read here, and that is the whole point of
    # this branch. Vitest exits 1 for a failed assertion AND for a mutant that
    # threw before any assertion could run, so the exit code cannot tell a kill
    # from a killed oracle — which is how three people on this branch read
    # `Tests 62 passed (62)` beside `Errors 1 error` as a passing run. The
    # counts can, and they are read in the order that makes a crash impossible
    # to score as a result.
    vitest_read "$out"
    if [ -z "$vitest_summary" ]; then
      echo "   BROKEN CASE: vitest printed no summary, so $test never ran — the mutation most"
      echo "   likely stopped the file being parsed at all"
      echo "   vitest exit status: $status"
      printf '%s' "$out" | grep -vE '^[[:space:]]*$' | sed 's/^/     /'
      broken=$((broken + 1))
    elif [ "$vitest_errors" -ne 0 ]; then
      echo "   BROKEN CASE: the mutation threw outside the test, so nothing judged it"
      echo "   $vitest_summary / $vitest_errline — a crashed oracle is not a kill"
      printf '%s' "$out" | grep -E 'Error:|TypeError|ReferenceError' | head -3 | sed 's/^/     /'
      broken=$((broken + 1))
    elif [ "$vitest_failed" -ge 1 ]; then
      echo "   red"
      printf '%s' "$out" | grep -E "AssertionError|Error:|expected|Unable to find" | head -6 | sed 's/^/     /'
      red=$((red + 1))
    elif [ "$vitest_passed" -eq 1 ]; then
      echo "   *** STILL GREEN: $test does not protect what it names ***"
      green=$((green + 1))
    else
      # Green at the baseline, and now selecting nothing: the mutation renamed
      # or removed the test itself, or — the shape this is here for — stopped
      # the file being collected at all, which vitest reports as `Tests no
      # tests` beside a failed test FILE. Either way no test met the mutant,
      # and cargo's "does not compile" verdict is the same fact.
      echo "   BROKEN CASE: after the mutation nothing named $test ran ($vitest_summary)"
      printf '%s' "$out" | grep -E 'Error:|error:|Failed to (parse|load)' | head -3 | sed 's/^/     /'
      broken=$((broken + 1))
    fi
  fi
  restore
  # Deterministic, so that the status of `. "$CASES"` is only ever about the
  # sourcing and never about whichever branch the file's last case took.
  return 0
}

# Pass one: every named test, unmutated, must be green.
mode=baseline
baseline_ok=0
baseline_bad=0
# shellcheck disable=SC1090
. "$CASES"
sourced=$?
echo "baseline: $baseline_ok green"
# ⚠️ **`baseline: N green` is a claim about the cases that were READ, and until
# this line nothing checked that they were all of them.** A syntax error part way
# through a case file stops the sourcing there; bash prints it, this script
# carried on with whatever it had, and the numbers below described a subset while
# exiting 0 — measured, on a scoped file of five cases that reported
# `baseline: 1 green`. It is the same shape as everything else this harness
# guards against, sitting in the harness: a result over less than was asked for,
# presented as a result.
if [ "$sourced" -ne 0 ]; then
  echo "COULD NOT READ $CASES to the end (status $sourced): $baseline_ok case(s) were read and"
  echo "whatever follows the failure was never seen. The numbers above are about a subset."
  exit 1
fi
if [ "$baseline_bad" -ne 0 ]; then
  echo "refusing to mutate against $baseline_bad test(s) that are not green to begin with"
  exit 1
fi
echo

# Pass two: the mutations.
mode=mutate
# shellcheck disable=SC1090
. "$CASES"
sourced=$?
# Checked again rather than trusted from pass one. The two passes take different
# branches through `case_` — one runs `cargo test` against an unmutated tree, the
# other rewrites a file and restores it — so they can fail independently, and a
# file that sources cleanly for the baseline and not for the mutations is a
# stranger failure than either alone.
#
# ⚠️ **Nothing reaches this today, and it is here as the argument rather than as
# a defence.** The two passes read the same text, so a syntax error fails the
# first one and exits above; what would reach this is a failure in the mutate
# branch alone — an unbound variable under `set -u`, a `case_` shape only that
# branch dislikes. The half above IS exercised: seen red on a case file whose
# fifth case is an unterminated quote, which reported `baseline: 3 green` and
# then refused, where before it would have gone on to mutate against a subset
# and exited 0.
if [ "$sourced" -ne 0 ]; then
  restore
  echo
  echo "COULD NOT READ $CASES to the end on the mutation pass (status $sourced), although the"
  echo "baseline pass read it whole. The counts below cover only what was reached."
  echo "red: $red   still green: $green   broken cases: $broken   exempted by /g: $every_match_count"
  exit 1
fi

restore
echo
echo "red: $red   still green: $green   broken cases: $broken   exempted by /g: $every_match_count"
# `red > 0` is not decoration on the other two, it is the condition they cannot
# express: zero green and zero broken is exactly what a file containing NO CASES
# reports, and it reported it with exit 0. So `red: 0 / still green: 0` — a
# result derived from nothing — was a passing result, and a file emptied by an
# edit would have been reported as success.
#
# That is the assertion-satisfied-by-zero failure this branch found eleven times
# in the code under test, sitting inside the tool built to find it. Seven files
# in scripts/mutations/ are stand-in workers rather than case files and answer
# this way today; they now exit non-zero, which is the honest answer to "did
# this prove anything" and the reason they do not belong in that directory.
[ "$red" -gt 0 ] && [ "$green" -eq 0 ] && [ "$broken" -eq 0 ]
