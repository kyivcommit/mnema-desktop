#!/usr/bin/env bash
#
# Do the mutation cases still apply, and do they still produce what they claim?
#
# **This is not a mutation run and must never be substituted for one.**
# `mutation-check.sh` asks whether a test still goes red when the thing it names
# is broken, and takes twenty minutes to answer. This asks a narrower question —
# whether each case's expression still matches the code it was written against,
# and whether what it produces is still what its marker describes — and answers
# in about a second. A file that passes here can still be full of tests that
# protect nothing; a file that fails here is proving less than it says, whatever
# the last mutation run reported.
#
# Why it exists, from the run that paid for it. A four-column re-indentation —
# one function's body moved out of a closure — broke three cases that quote it.
# The harness reported two of them, and the third only on the run after that,
# because `perl` patterns are unanchored: twelve leading spaces matched the last
# twelve of sixteen, so that case had been substituting into the middle of an
# indent and passing both guards **for a reason unrelated to what it meant**. It
# was green, and meaningless, and no amount of running the harness would have
# said so. What finds that is checking every case at once, cheaply enough to do
# after every refactor.
#
# The two guards below are `mutation-check.sh`'s own, and deliberately the same:
#
#   1. the expression changed the file at all, and
#   2. it changed it into what the marker describes.
#
# `contains` is the multi-line-safe test that file argues for at length, copied
# rather than approximated: `grep -F` given a pattern with a newline splits the
# PATTERN and matches if ANY one of its lines appears anywhere, which is what let
# a mutation that changed nothing pass its own check once already.
#
# What it does **not** check: that the case names a test that exists (the
# harness's baseline pass does that), that the mutation compiles, or that
# anything goes red. Those need a compiler and a test run.
#
# ⚠️ **Read the exit code from this script, not from a pipeline.**
# `scripts/mutation-staleness.sh cases | tail` reports `tail`'s status, not this
# one's — measured on this project, where a harness run with two broken cases was
# quoted as "exit code 0" because of exactly that. Redirect and capture:
#
#   scripts/mutation-staleness.sh scripts/mutations/embedding.sh > out.txt 2>&1
#   echo "EXIT=$?"
#
# Usage:
#   scripts/mutation-staleness.sh <case-file>
#
# Nothing is written outside a temporary directory: every case is applied to a
# copy, and the working tree is never touched.

set -uo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <case-file>" >&2
  exit 2
fi

CASES=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
if [ ! -f "$CASES" ]; then
  echo "no case file at $CASES" >&2
  exit 2
fi
REPO=$(git rev-parse --show-toplevel) || exit 2
WORK=$(mktemp -d "${TMPDIR:-/tmp}/mnema-staleness.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

# Exact multi-line substring test. NOT `grep -F` — see the header.
contains() {
  MUTATION_MARKER="$1" perl -0777 -ne 'exit(index($_, $ENV{MUTATION_MARKER}) < 0)' "$2"
}

checked=0
stale=0

# case_ <label> <file> <perl-expr> <marker> <package> <test-name> <cargo args...>
#
# The same signature the case files are written against, so one file serves both
# tools. Everything from the package onwards belongs to the harness and is
# ignored here.
case_() {
  local label="$1" file="$2" expr="$3" marker="$4"
  checked=$((checked + 1))

  if [ ! -f "$REPO/$file" ]; then
    echo "MISSING FILE: $file — $label"
    stale=$((stale + 1))
    return
  fi

  local scratch="$WORK/scratch"
  cp "$REPO/$file" "$scratch"
  perl -0pi -e "$expr" "$scratch"

  if cmp -s "$REPO/$file" "$scratch"; then
    echo "NO LONGER APPLIES: $label"
    echo "   the expression changed nothing in $file — the code it was written against has moved"
    stale=$((stale + 1))
    return
  fi
  if ! contains "$marker" "$scratch"; then
    echo "MARKER NO LONGER DESCRIBES IT: $label"
    echo "   $file changed, but not into what the case says it becomes"
    stale=$((stale + 1))
  fi
}

# shellcheck disable=SC1090
. "$CASES"

echo
echo "cases checked: $checked   stale: $stale"

# `checked > 0` is not decoration on `stale == 0`, it is the condition that one
# cannot express: a case file containing no cases reports zero stale, and would
# otherwise pass. That is the assertion-satisfied-by-zero failure this project
# has now found eleven times in the code and twice inside the tools built to find
# it.
if [ "$checked" -eq 0 ]; then
  echo "no cases found in $CASES — a result derived from nothing is not a result"
  exit 1
fi
[ "$stale" -eq 0 ]
