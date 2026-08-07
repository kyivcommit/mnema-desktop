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

restore() { git -C "$TREE" checkout -q -- . ; }

# Exact multi-line substring test. NOT `grep -F`: given a pattern containing a
# newline, grep splits the PATTERN and matches if ANY one of its lines occurs
# anywhere in the file. That is what let a mutation which changed nothing pass
# its own "did it apply" check, and `-z` does not help — it changes how the input
# is split, not the pattern. Measured before this was written.
contains() {
  MUTATION_MARKER="$1" perl -0777 -ne 'exit(index($_, $ENV{MUTATION_MARKER}) < 0)' "$2"
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
    case "$seen" in *"$key"*) return ;; esac
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
    return
  fi

  printf '%s\n' "-- $label"
  restore
  perl -0pi -e "$expr" "$file"

  # The authoritative guard. A mutation that left the file byte-for-byte
  # identical is a broken case, not a test result, and must never count as either.
  if git -C "$TREE" diff --quiet -- "$file"; then
    echo "   BROKEN CASE: the mutation left $file unchanged"
    broken=$((broken + 1)); restore; return
  fi
  # Second guard: it changed, but into what the case says? Redundant with the
  # first for detecting a no-op; kept because it also catches a perl expression
  # that matched somewhere unintended.
  if ! contains "$marker" "$file"; then
    echo "   BROKEN CASE: $file changed, but not into what the case describes"
    broken=$((broken + 1)); restore; return
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
}

# Pass one: every named test, unmutated, must be green.
mode=baseline
baseline_ok=0
baseline_bad=0
# shellcheck disable=SC1090
. "$CASES"
echo "baseline: $baseline_ok green"
if [ "$baseline_bad" -ne 0 ]; then
  echo "refusing to mutate against $baseline_bad test(s) that are not green to begin with"
  exit 1
fi
echo

# Pass two: the mutations.
mode=mutate
# shellcheck disable=SC1090
. "$CASES"

restore
echo
echo "red: $red   still green: $green   broken cases: $broken"
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
