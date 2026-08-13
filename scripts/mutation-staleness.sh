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
#   scripts/mutation-staleness.sh                 # every case file, the default
#   scripts/mutation-staleness.sh <case-file>…    # only these
#
# **Sweeping is the default because a green line about one file gets read as a
# green line about the directory.** The first honest run of this script reported
# `cases checked: 80  stale: 0` — true of `embedding.sh`, and true of one file
# out of twenty-four while three cases in two other files had stopped applying
# altogether. A count from a limited query, presented as a total, is the mistake
# this project has now paid for four times. So the summary names the files it
# read, and the single-file form is still there for when somebody wants it.
#
# **What is a case file, and why the seven scripts beside them are not.**
# `scripts/mutations/` holds two kinds of file: lists of `case_` calls, and
# stand-in worker binaries (`pdf-*.sh`) that tests execute as a fake extraction
# worker. A shebang is what tells them apart — a file that declares an
# interpreter is a program, and every one of the seven has one while none of the
# twenty-four case files does. It is checked before anything is sourced, which
# matters: sourcing a stand-in worker would *run* it, and one of them blocks
# reading its stdin.
#
# That test is deliberately not "does it contain any cases", because then a case
# file emptied by an edit would classify itself out of the sweep and be reported
# as nothing at all. A file with no shebang and no cases is a **failure** here.
#
# ⚠️ **The whole question this script has to keep asking of itself: is there an
# input for which it reports success by checking LESS?** Asked deliberately in
# review round 3, and the answer was two, both measured rather than reasoned:
#
#   1. A case file that gains a shebang was skipped — one file and thirty-four
#      cases quietly out of the sweep, `stale: 0`, exit 0. The mirror of the hole
#      this script exists to close, inside it. A skipped file holding a `case_`
#      call is now a failure, not a skip.
#   2. A case file with a syntax error part way through sourced as far as the
#      error and stopped. The cases before it were checked, the rest were never
#      seen, and the run exited 0. The status of `.` is now read.
#
# Not a hole, checked: a `case_` call with too few arguments dies on `set -u`
# and takes the run with it, loudly. And a glob that matches nothing exits 2
# naming the path rather than passing over an empty list.
#
# That list is what it is because somebody asked once. It is worth asking again
# of any new branch added below.
#
# Nothing is written outside a temporary directory: every case is applied to a
# copy, and the working tree is never touched.

set -uo pipefail

REPO=$(git rev-parse --show-toplevel) || exit 2

if [ $# -eq 0 ]; then
  FILES=("$REPO"/scripts/mutations/*.sh)
else
  FILES=("$@")
fi
WORK=$(mktemp -d "${TMPDIR:-/tmp}/mnema-staleness.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

# Exact multi-line substring test. NOT `grep -F` — see the header.
contains() {
  MUTATION_MARKER="$1" perl -0777 -ne 'exit(index($_, $ENV{MUTATION_MARKER}) < 0)' "$2"
}

checked=0
stale=0
files_read=0
empty=0
skipped=""
read_names=""
hidden=0
unreadable=0

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
  # Deterministic, so that the status of `. "$file"` below is only ever about
  # the sourcing and never about whichever branch the last case took.
  return 0
}

for file in "${FILES[@]}"; do
  if [ ! -f "$file" ]; then
    echo "no case file at $file" >&2
    exit 2
  fi
  name=$(basename "$file")

  # Before sourcing, not after: a stand-in worker that got sourced would run.
  case "$(head -1 "$file")" in
    '#!'*)
      # ⚠️ **A skip is a claim that there was nothing to check, and it has to be
      # checked too.** Measured: prepend a shebang to a case file and the sweep
      # reported one file and thirty-four cases fewer, `stale: 0`, exit 0 — the
      # mirror of the hole this script exists to close, in the script itself.
      # The file was named in the skipped list, so the only thing between a case
      # file and silent exclusion was somebody reading it.
      if grep -q '^case_ ' "$file"; then
        echo "SHEBANG OVER CASES: $name"
        echo "   it declares an interpreter, so it would be skipped as a stand-in worker — and it"
        echo "   holds cases, which would go unchecked. One or the other, not both."
        hidden=$((hidden + 1))
      else
        skipped="$skipped $name"
      fi
      continue
      ;;
  esac

  before=$checked
  # shellcheck disable=SC1090
  . "$file"
  sourced=$?
  files_read=$((files_read + 1))
  count=$((checked - before))
  read_names="$read_names $name($count)"
  # ⚠️ **The second door of the same shape, and it is not hypothetical.** A
  # syntax error part way through a case file stops the sourcing there; bash
  # prints it, and without this the sweep reported the cases it happened to
  # reach and exited 0 with the rest silently unchecked. Measured — and measured
  # twice, because `mutation-check.sh` did exactly this to a scoped file of mine
  # in this same round and reported `baseline: 1 green` for five cases.
  if [ "$sourced" -ne 0 ]; then
    echo "COULD NOT BE READ: $name"
    echo "   sourcing it failed after $count case(s); whatever follows was never checked"
    unreadable=$((unreadable + 1))
  fi
  if [ "$count" -eq 0 ]; then
    echo "NO CASES: $name declares no interpreter, so it is a case file, and it holds none"
    empty=$((empty + 1))
  fi
done

# **Say what was read, by name.** A bare "stale: 0" is the sentence that got
# this script's own first run believed about twenty-three files it never opened.
echo
echo "read $files_read case file(s), $checked cases:$read_names"
if [ -n "$skipped" ]; then
  echo "skipped, not case files (they declare an interpreter — stand-in workers):$skipped"
fi
echo "stale: $stale   holding no cases: $empty   hidden by a shebang: $hidden   unreadable: $unreadable"
echo "nothing was compiled and no test was run — that is scripts/mutation-check.sh"

# `checked > 0` is not decoration on `stale == 0`, it is the condition that one
# cannot express: a case file containing no cases reports zero stale, and would
# otherwise pass. That is the assertion-satisfied-by-zero failure this project
# has now found eleven times in the code and twice inside the tools built to find
# it.
if [ "$files_read" -eq 0 ] || [ "$checked" -eq 0 ]; then
  echo "no cases anywhere in what was asked for — a result derived from nothing is not a result"
  exit 1
fi
# Four conditions, and three of them are the same one: **a green line must not be
# reachable by checking less.** `stale` is the finding this script is for;
# `empty`, `hidden` and `unreadable` are the three ways it could otherwise report
# success over cases it never looked at.
[ "$stale" -eq 0 ] && [ "$empty" -eq 0 ] && [ "$hidden" -eq 0 ] && [ "$unreadable" -eq 0 ]
