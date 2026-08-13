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
# **Its cheap sibling: `scripts/mutation-staleness.sh <case-file>`.** This script
# answers "does the test still go red" and takes 3:17 on an Apple M2 Max, cold
# `CARGO_TARGET_DIR`, 80 cases over 69 baseline tests — measured with `time`
# around the whole invocation. `236% cpu` in that same measurement means this
# harness is very nearly serial, about two and a half cores busy out of
# twelve, so core count is not the axis a CI runner loses on. What this does
# not transfer to: a 2-core `ubuntu-24.04` runner with a cold target directory
# of its own is not this machine, nobody has measured it, and `ci.yml`'s
# `timeout-minutes: 30` was set from nothing at all — the first pull request
# is what measures that, and the number belongs there once it runs. That one
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
# for real against all 546 expressions in this repository, it found eight
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
expr_wants_every_match() {
  printf '%s' "$1" | perl -ne 'exit(/([a-zA-Z]*)$/ && $1 =~ /g/ ? 0 : 1)'
}

seen=""

# case_ <label> <file> <perl-expr> <marker> <package> <test-name> <cargo target args...>
#
# Runs in two passes over the same case file. `$mode` selects which.
case_() {
  local label="$1" file="$2" expr="$3" marker="$4" pkg="$5" test="$6"
  shift 6

  if [ "$mode" = baseline ]; then
    # A case naming a test that is already failing, or misspelled, would read as
    # a pass in the mutation pass below — the run would go red for a reason that
    # has nothing to do with the mutation. So every named test is run once
    # unmutated first and must be green. Deduplicated: several cases usually
    # target one test.
    local key="|$pkg $* $test|"
    case "$seen" in *"$key"*) return 0 ;; esac
    seen="$seen$key"

    local out
    out=$(cargo test -p "$pkg" "$@" -- --exact "$test" 2>&1)

    # `1 passed`, not merely exit zero. `--exact` with a name that matches
    # nothing runs no tests and exits 0, so a misspelled case would sail through
    # here and then be reported one step later as "the test does not protect
    # what it names" — a true-sounding verdict about a test that does not exist.
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
  out=$(cargo test -p "$pkg" "$@" -- --exact "$test" 2>&1)
  status=$?

  # Checked before the status, because a mutation that does not compile also
  # exits non-zero. Counting that as red is the no-op defect wearing a different
  # hat: the test never ran, so it proved nothing.
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
  else
    echo "   *** STILL GREEN: $test does not protect what it names ***"
    green=$((green + 1))
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
