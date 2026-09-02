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
# not matched: only the capitals-plus-parenthesis form is. **The form is exact
# and nothing else is read as it**: `BOOKED (` with a space, the word split
# across two comment lines, and lower case are all prose to this script, and
# a submodule is not entered (`git grep` does not recurse into one; this
# repository has none). The check is only as good as the habit of writing the
# form, which is what the ledger's §15.4 asks for.
#
# Usage:
#   scripts/check-booked.sh              # every tracked file; exit 1 if any
#   scripts/check-booked.sh --self-test  # the sweep against a throwaway
#                                        # repository, in both directions
#
# `--self-test` builds two temporary git repositories — one committing two
# markers in files of two extensions, one committing the two near-misses (the
# prose form, and the word in capitals with no parenthesis) plus an untracked
# marker — and runs the SAME function this script runs on the real tree, not a
# copy of the pattern, asserting red on the first and green on the second.
# Then it copies this file into each repository's `scripts/` and runs it there
# as CI would, asserting the exit status of the whole script and not only of
# the function. A self-test that only checks the pattern against strings would
# say nothing about the exit code, which is the only thing CI reads.

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
  # `cd` on its own line: inside `$(cd … && git grep …)` a directory that does
  # not exist returns 1, which is also grep's "no match" — review of PR #28
  # showed `sweep /no/such/dir` answering "no open obligation".
  cd "$root" || return 2
  hits="$(git grep -n -I -F -- "$MARK" 2>&1)"
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
  # The self-test runs copies of this file, and a copy must never run the
  # self-test again: with the argument check below deleted, `--self-test bogus`
  # on the copy recursed without end and left 944 temporary repositories
  # behind before it was killed (fix round 2, measured). The copies are run
  # with this variable set, and a nested self-test stops here with 3 — after
  # the argument check, so that a bad argument is still 2 from inside. No
  # mutant of this guard is run on purpose: deleting it recurses.
  if [ $# -ne 1 ]; then
    echo "unknown option: $2" >&2
    exit 2
  fi
  if [ -n "${CHECK_BOOKED_INNER:-}" ]; then
    echo "check-booked: refusing to run the self-test from inside itself" >&2
    exit 3
  fi
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

  # Red: the marker, as it would be written in a doc comment — and a second
  # one in a file of another extension, so a pathspec narrowing the sweep to
  # `*.rs` is a mutant this catches (review of PR #28, Minor).
  red="$(mk red "/// ${MARK}Task 11: give reason its own wording)")"
  printf '%s\n' "// ${MARK}Task 12: the same, in the interface)" > "$red/note.ts"
  git -C "$red" add note.ts
  git -C "$red" commit -q -m probe-ts
  out="$(sweep "$red")"; status=$?
  if [ "$status" -ne 1 ] || ! printf '%s\n' "$out" | grep -q 'note.rs:1:' \
     || ! printf '%s\n' "$out" | grep -q 'note.ts:1:'; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: two committed markers must exit 1 and name note.rs:1 and note.ts:1; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  # Green: the two near-misses, neither of which is a promise. The check is a
  # plain substring match and does not try to be clever about word boundaries:
  # a marker glued to a prefix is still a marker, so no row here tests that.
  green="$(mk green "// booked to Task 6 and now paid
// ${WORD} as a word, no parenthesis
// ${WORD} (a space before the parenthesis)
// ${WORD}
// (split across two lines)")"
  out="$(sweep "$green")"; status=$?
  if [ "$status" -ne 0 ]; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: prose, the bare word, a space and a line break must exit 0; got $status:"
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

  # A root that does not exist must be an error (2), not "nothing found" (0):
  # inside `$(cd … && git grep …)` a failed `cd` returned grep's own "no
  # match" status, and the sweep answered green about a directory it never
  # entered (review of PR #28, Important 3).
  out="$(sweep "$tmp/absent" 2>/dev/null)"; status=$?
  if [ "$status" -ne 2 ]; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: a root that does not exist must exit 2; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  # An existing directory that is no repository is `git grep`'s own failure
  # (128), which must come back as 2 and not as "nothing found" — the guard
  # above the found branch had no test until this row (fix round 2).
  mkdir -p "$tmp/plain"
  out="$(sweep "$tmp/plain" 2>/dev/null)"; status=$?
  if [ "$status" -ne 2 ]; then
    bad=$((bad + 1))
    echo "SELF-TEST FAILURE: a directory that is not a repository must exit 2; got $status:"
    printf '%s\n' "$out" | sed 's/^/  /'
  fi

  # The script itself, not only the function: CI reads the exit status of
  # the last line of this file, and a self-test that stops at `sweep` leaves
  # that line unguarded — `sweep "$REPO" || exit 0` survived the first version
  # of this self-test (review of PR #28, Important 2). A copy of this file is
  # placed under `scripts/` of each throwaway repository, where `REPO` resolves
  # to that repository, and run as CI would run it.
  for pair in "red:1" "green:0"; do
    dir="$tmp/${pair%%:*}"; want="${pair##*:}"
    mkdir -p "$dir/scripts"
    cp "${BASH_SOURCE[0]}" "$dir/scripts/check-booked.sh"
    out="$(CHECK_BOOKED_INNER=1 bash "$dir/scripts/check-booked.sh")"; status=$?
    if [ "$status" -ne "$want" ]; then
      bad=$((bad + 1))
      echo "SELF-TEST FAILURE: the script run on the ${pair%%:*} repository must exit $want; got $status:"
      printf '%s\n' "$out" | sed 's/^/  /'
    fi
  done
  # An argument the script does not know is refused with 2, in both spellings;
  # without this row the refusal stood on `set -u` alone (fix round 2).
  for args in "bogus" "--self-test bogus"; do
    # shellcheck disable=SC2086
    CHECK_BOOKED_INNER=1 bash "$tmp/green/scripts/check-booked.sh" $args > /dev/null 2>&1; status=$?
    if [ "$status" -ne 2 ]; then
      bad=$((bad + 1))
      echo "SELF-TEST FAILURE: \`$args\` must be refused with 2; got $status"
    fi
  done

  if [ "$bad" -ne 0 ]; then
    echo "--- self-test: $bad failure(s) ---"
    exit 1
  fi
  echo "--- self-test: red on committed markers; green on every non-match the header names and an untracked file; 2 on a missing root, a non-repository and a bad argument; the script's own exit status checked both ways ---"
  exit 0
fi

if [ $# -ne 0 ]; then
  echo "unknown option: $1" >&2
  exit 2
fi

sweep "$REPO"
