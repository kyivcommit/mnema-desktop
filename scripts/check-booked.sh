#!/usr/bin/env bash
#
# Fail while any obligation written into the code is still open.
#
# The marker is the word BOOKED in capitals, immediately followed by an opening
# parenthesis, and then what was promised and to whom:
#
#   // BOOKED@Task 11: give `reason` its own wording instead of globset's)
#
# — written here with `@` in place of the parenthesis, because this file is
# swept like every other and the example would otherwise be the one open
# obligation the sweep reports. The pattern itself is assembled at run time
# below for the same reason.
#
# Why this exists. Twice in one pull request an obligation was written into a
# comment — "booked to Task 11", with the remedy assigned to a later task — and
# never done; both came back as review findings, one of them blocking. A
# comment compiles to nothing and fails nothing, so the promise lived exactly
# as long as nobody re-read it. This script is the same fix `check-citations.sh`
# is for citations: the artefact that rots silently is turned into an exit
# code.
#
# What the exit code means: **an open obligation blocks the merge.** There is
# no "until" clause and no deadline, on purpose — a deadline named in a comment
# is a plan this repository cannot read, which puts it back where it started.
# The two ways to make this green are to do the thing and delete the marker, or
# to decide it belongs after the merge and write it in the ledger's §15.4,
# where booked work lives, and delete the marker. Ordinary prose that uses the
# word — "booked to Task 6 and now paid" — is history, not a promise, and is
# not matched: only the capitals-plus-parenthesis form is.
#
# Usage:
#   scripts/check-booked.sh              # every tracked file; exit 1 if any
#   scripts/check-booked.sh --self-test  # the sweep against a throwaway
#                                        # repository, in both directions
#
# `--self-test` builds a temporary git repository, commits a file carrying one
# marker and a file carrying the two near-misses (the prose form, and the word
# in capitals with no parenthesis), and runs
# the SAME function this script runs on the real tree — not a copy of the
# pattern — asserting red on the first and green on the second. A self-test
# that only checks the pattern against strings would say nothing about the
# exit code, which is the only thing CI reads.

set -u

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Assembled, not written: the literal must not appear in this file's source.
WORD="BOOKED"
MARK="${WORD}("

# The sweep. `git grep` reads tracked files only, so a scratch file or an
# untracked note cannot make this red, and `-I` skips binaries. Exit status
# 0 means grep found something; 1 means nothing; anything else is grep's own
# failure and must not read as "nothing found".
sweep() {
  local root="$1" hits status
  hits="$(cd "$root" && git grep -n -I -F -- "$MARK" 2>&1)"
  status=$?
  if [ "$status" -gt 1 ]; then
    echo "check-booked: git grep failed ($status) in $root" >&2
    echo "$hits" >&2
    return 2
  fi
  if [ "$status" -eq 0 ]; then
    local n
    n="$(printf '%s\n' "$hits" | wc -l | tr -d ' ')"
    echo "--- $n open obligation(s) written into the code ---"
    printf '%s\n' "$hits" | sed 's/^/  /'
    echo "--- an obligation written into the code blocks the merge: do it, or move it to the ledger's §15.4, and delete the marker ---"
    return 1
  fi
  echo "--- no open obligation is written into the code ---"
  return 0
}

if [ "${1:-}" = "--self-test" ]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT INT TERM
  bad=0

  mk() {
    # A fresh repository with one committed file whose content is $2.
    local dir="$tmp/$1"
    mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" config user.email self-test@example.invalid
    git -C "$dir" config user.name self-test
    printf '%s\n' "$2" > "$dir/note.rs"
    git -C "$dir" add note.rs
    git -C "$dir" commit -q -m probe
    echo "$dir"
  }

  # Red: the marker, as it would be written in a doc comment.
  red="$(mk red "/// ${MARK}Task 11: give reason its own wording)")"
  out="$(sweep "$red")"; status=$?
  if [ "$status" -ne 1 ] || ! printf '%s\n' "$out" | grep -q 'note.rs:1:'; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: a committed marker must exit 1 and name note.rs:1; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  # Green: the two near-misses, neither of which is a promise. The check is a
  # plain substring match and does not try to be clever about word boundaries:
  # a marker glued to a prefix is still a marker, so no row here tests that.
  green="$(mk green "// booked to Task 6 and now paid
// ${WORD} as a word, no parenthesis")"
  out="$(sweep "$green")"; status=$?
  if [ "$status" -ne 0 ]; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: prose and the bare word must exit 0; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  # Untracked: a marker in a file git does not track must not make it red.
  printf '%s\n' "/// ${MARK}never committed)" > "$green/scratch.rs"
  out="$(sweep "$green")"; status=$?
  if [ "$status" -ne 0 ]; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: an untracked file must not count; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  if [ "$bad" -ne 0 ]; then
    echo "--- self-test: $bad failure(s) ---"
    exit 1
  fi
  echo "--- self-test: red on a committed marker, green on prose, the bare word and an untracked file ---"
  exit 0
fi

if [ $# -ne 0 ]; then
  echo "unknown option: $1" >&2
  exit 2
fi

sweep "$REPO"
