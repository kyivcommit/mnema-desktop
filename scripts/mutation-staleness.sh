#!/usr/bin/env bash
#
# Do the mutation cases still apply, and do they still produce what they claim?
#
# **This is not a mutation run and must never be substituted for one.**
# `mutation-check.sh` asks whether a test still goes red when the thing it names
# is broken, and takes 3:17 on an Apple M2 Max, cold `CARGO_TARGET_DIR`, 80
# cases over 69 baseline tests — measured with `time` around the whole
# invocation. `236% cpu` in that same measurement means the harness is very
# nearly serial, about two and a half cores busy out of twelve, so core count
# is not the axis a CI runner loses on. What this does not transfer to: a
# 2-core `ubuntu-24.04` runner with a cold target directory of its own is not
# this machine — **measured there on the first pull request that leg ever ran
# on: 6m17s, green**, so about twice this machine rather than the ten times a
# core count would suggest, which is what "nearly serial" predicted. This asks
# a narrower question —
# whether each case's expression still matches the code it was written against,
# whether what it produces is still what its marker describes, and (guard 4,
# added later) whether the test it names still exists. **Measured, not "about
# a second" as this line used to claim**: `time scripts/mutation-staleness.sh
# > out.txt 2>&1` on an Apple M2 Max, cold, sweeping all 851 cases across 41
# case files, is 22.6-22.7s for guards 1-3 alone and 25.2-25.9s with guard 4
# — repeatable across runs, not a one-off. A file that passes here can still
# be full of tests that protect nothing; a file that fails here is proving
# less than it says, whatever the last mutation run reported.
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
# The three guards below are `mutation-check.sh`'s own, and deliberately the same:
#
#   1. the expression changed the file at all,
#   2. it changed it into what the marker describes, and
#   3. it changed it exactly once — unless the expression itself says otherwise.
#
# `contains` is the multi-line-safe test that file argues for at length, copied
# rather than approximated: `grep -F` given a pattern with a newline splits the
# PATTERN and matches if ANY one of its lines appears anywhere, which is what let
# a mutation that changed nothing pass its own check once already.
#
# Guard 3 is what the re-indentation above actually needed. One of its three
# broken cases substituted into the middle of a *different* function's
# matching indentation and reported still-green — it passed guards 1 and 2 for
# a reason unrelated to what it meant, because `perl` patterns are unanchored.
# `s///` without `/g` cannot tell that story from its own return value — it
# stops at the first match and reports 1 either way — so this guard counts
# occurrences separately, on a copy nothing else reads, by forcing `/g` onto
# whatever expression is given (a no-op for one that already has it). No `g`
# means exactly one occurrence is required; `g` means at least one — because
# the one legitimate many-match case, `linux-resource.sh`'s "no case arm sets
# a library any more", declares its own multiplicity in its own syntax, and
# needs no exception list to be told apart from the rest.
#
# Applying that, for real, against every one of this repository's 546
# expressions found eight cases sharing a pattern with a sibling function —
# boilerplate the pattern did not name specifically enough to tell apart —
# not the one exception a smaller check had assumed. All eight were real
# ambiguity (one, `journal_skipped_pages`'s, was the *exact* shape of the bug
# above: an unanchored 4-space pattern matching inside a 12-space-indented
# sibling as a substring), and all eight still mutated the correct line every
# time, by luck of definition order rather than by what the pattern named.
# Each was narrowed to name its enclosing function or arm rather than loosened
# or exempted, verified to produce the byte-identical mutation it always had,
# and this sweep is the record that all 546 matched exactly what their own
# flag said they should.
#
# ⚠️ 546 is what that run covered and is not the number today — every run of
# this script re-derives it, and prints it beside the files it read. Do not
# quote the figure above as a current total; run the sweep.
#
# A fourth guard, added after this script's own blind spot went unfixed for
# weeks: it now also checks that the named test EXISTS — a grep, not a run.
# `mutation-check.sh`'s baseline pass is the only place that requires the
# test green, and it only runs on the files a CI matrix actually lists; a case
# file outside that matrix could have its test renamed out from under it and
# this script would keep saying `stale: 0` about it, because a case's fifth
# and sixth fields (`<target>` and `<test-name>`) were, until now, read by
# nothing but `mutation-check.sh` itself. See guard 4 in `case_` below for what
# "exists" means for each runner, and the header note above the `case_`
# function for what is still not checked even now.
#
# ⚠️ The RED this guard was built against, precisely: `55b2bc6` left
# `pr8-ui-folders.sh` and `pr9-index.sh` naming seven tests that had been
# renamed or lived in the wrong file (fixed by `be2db76`) — not the thirty-
# four the "SHEBANG OVER CASES" note below once got confused with, which is a
# different hole (a case file skipped outright) that this guard does not
# touch.
#
# What it does **not** check: that the mutation compiles, or that anything
# goes red — those need a compiler and a test run. Nor, for the test-name
# guard just described, whether the test is green, or whether a `runner=`
# field was written somewhere other than straight after the test name — see
# the note above `case_` for why those two stay `mutation-check.sh`'s job
# alone. A misspelled runner name is no longer in this list: see the same
# note for what catches it instead.
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
  # ⚠️ **The glob above has no floor.** A case file renamed to something other
  # than `*.sh`, or moved into a subdirectory, would leave the sweep silently
  # — `files_read` and `checked` simply come back smaller, and nothing below
  # asserted the size, only `files_read == 0` did. `git ls-files` names every
  # tracked file under the directory regardless of name or nesting, so a file
  # it names that the glob did not match is exactly that hole — the same
  # shape as the shebang hole above, closed the same way: a skip has to be
  # checked too.
  while IFS= read -r tracked; do
    found=0
    for f in "${FILES[@]}"; do
      [ "$f" = "$REPO/$tracked" ] && found=1 && break
    done
    if [ "$found" -eq 0 ]; then
      echo "TRACKED BUT NOT SWEPT: $tracked — scripts/mutations/*.sh did not match it" >&2
      exit 2
    fi
  done < <(git -C "$REPO" ls-files scripts/mutations)
else
  FILES=("$@")
fi
WORK=$(mktemp -d "${TMPDIR:-/tmp}/mnema-staleness.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

# Exact multi-line substring test. NOT `grep -F` — see the header.
contains() {
  MUTATION_MARKER="$1" perl -0777 -ne 'exit(index($_, $ENV{MUTATION_MARKER}) < 0)' "$2"
}

# Whether `expr`'s own trailing flags carry a `g` — "every arm" rather than
# "this one place". Done in `perl`, not bash's `[[ =~ ]]`: the obvious bash
# regex for "trailing run of letters", `([a-zA-Z]*)$`, matched empty on this
# platform's bash even against a string that plainly ends in letters —
# measured, not assumed — so the one tool already relied on for exact text
# matching everywhere else in this script does this too.
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

checked=0
stale=0
files_read=0
empty=0
skipped=""
read_names=""
hidden=0
unreadable=0
# N6: `/g` is a self-declaring opt-out from guard 3's "exactly one" — a case
# earns "at least one" just by carrying the flag, no review required. An
# exemption nobody counts is an exemption nobody would notice growing, so this
# is printed in the summary the same way the skipped-file list already makes
# its own exclusions visible.
every_match_count=0
names_checked=0

# Resolves a cargo package name to the directory holding its Cargo.toml, by
# grepping for the FIRST `name = "…"` line in every workspace member's
# manifest — deliberately not a precise TOML parse. `[package]` always comes
# before `[lib]`/`[[bin]]` in every manifest this repository has today (a
# `[[bin]]` can carry a second, different `name = "…"` further down, e.g.
# `mnema-extract-worker` inside `mnema-extract`'s own Cargo.toml), so the
# first match is the package name a case's `<target>` field actually names.
# Prints the directory — ABSOLUTE, because it comes from `find "$REPO/…"` —
# and returns 0 on a match, or returns 1 having printed nothing. Callers must
# not prepend `$REPO/` to it again.
#
# Cached per target in `$WORK`, not in a bash associative array: `env bash`
# on this machine (and so, presumably, on any contributor's Mac without a
# newer bash on `PATH`) resolves to Apple's bundled 3.2.57, which does not
# have `declare -A` — verified with `env bash --version` rather than assumed,
# after a `declare -A` draft of this cache would have died outright on this
# very box. A `$WORK/pkgdir-<target>` file holds either the directory (a hit)
# or nothing at all (a confirmed miss, recorded so a target that names no
# package is not re-searched on every one of its cases); its mere existence
# is what tells the two apart.
#
# Sets `$PKGDIR_RESULT` and returns 0 or 1, rather than printing the
# directory for a caller to capture with `$(…)`: a command substitution
# forks a subshell every time regardless of what runs inside it, measured
# here to cost about 1.8s over 800 calls even when the body is nothing but a
# cache hit — most of what a first cut of this cache (fast internally, still
# wrapped in `pkgdir=$(find_pkg_dir "$target")` at the call site) left on the
# table. A cache hit below is `read` (a builtin) against a file redirection,
# which forks nothing at all; only a genuine miss forks `find`, `grep` and
# `dirname` (about a dozen times total, once per distinct package, not once
# per case).
find_pkg_dir() {
  local target="$1" toml
  # Sanitised the same way the vitest branch names its slurp: a `<target>`
  # that is a path (a `runner=` written in the wrong position leaves a test
  # file here) would otherwise make this cache path a directory that does not
  # exist, and the write would print a raw shell error on top of the verdict.
  local cache="$WORK/pkgdir-${target//\//_}"
  if [ -f "$cache" ]; then
    if [ -s "$cache" ]; then
      IFS= read -r PKGDIR_RESULT < "$cache"
      return 0
    fi
    PKGDIR_RESULT=""
    return 1
  fi
  while IFS= read -r toml; do
    if grep -qm1 -E '^name[[:space:]]*=[[:space:]]*"'"$target"'"' "$toml"; then
      PKGDIR_RESULT=$(dirname "$toml")
      printf '%s' "$PKGDIR_RESULT" > "$cache"
      return 0
    fi
  done < <(find "$REPO/crates" "$REPO/src-tauri" -maxdepth 2 -name Cargo.toml 2>/dev/null)
  PKGDIR_RESULT=""
  : > "$cache"
  return 1
}

# case_ <label> <file> <perl-expr> <marker> <target> <test-name> [runner=<name>] [args...]
#
# The same signature the case files are written against, so one file serves both
# tools. Guard 4, below, is the only thing that reads `<target>` and
# `<test-name>`; everything after the test name — the runner's own trailing
# arguments — still belongs to the harness and is still ignored here.
#
# ⚠️ **Two things guard 4 still cannot see, one loose thing it accepts on
# purpose, and one divergence from the tool it is standing in for.**
# `mutation-check.sh` grew an optional `runner=` field so a case can name a
# vitest test instead of a cargo one:
#
#   1. A `runner=` field written anywhere but straight after the test name is
#      never seen as a runner at all — guard 4 only inspects the seventh
#      field. Verified by hand: `case_ … target test extra-arg runner=vitest`
#      leaves guard 4 treating it as `cargo` (the default), which then asks
#      whether some workspace member is named `target` — the vitest test
#      file's path — finds none, and reports `TEST NOT FOUND` for the right
#      case but the wrong reason. `mutation-check.sh` refuses this shape
#      outright (`puts runner=… after another argument`, exit 2), which is
#      the only place the reason is stated correctly.
#   2. Whether the test is GREEN. Guard 4 is a grep for the name, not a
#      compile or a run — `mutation-check.sh`'s baseline pass is the only
#      place that requires it to pass.
#
# A misspelled runner name (`runner=vitets`) used to be a third — it falls
# through the `case` below to its default arm — but that arm now prints
# UNRECOGNISED RUNNER at the case and the final assertion requires
# `names_checked == checked`, so it is a checked failure, not a silent one.
# Guard 4 still does not know the set of valid runner names — it only notices
# that this one matched neither of the two it does know — which is why
# `mutation-check.sh`'s own exit-2 refusal remains the only place a bad
# runner name is named correctly.
#
# 🔴 **Guard 4's cargo check is deliberately loose, the same trade the fn-index
# below makes for speed.** `grep -qxF "fn $want"` (or, before the index
# existed, the equivalent single grep) is satisfied by ANY line reading
# `fn <name>` anywhere under the package's `src/` or `tests/` — a helper of
# the same name in an unrelated module, a doc comment quoting `fn foo(...)`,
# or the identifier inside a string literal all count. It does not require
# `#[test]`, does not resolve which module the case's `::`-qualified path
# actually names, and ignores every trailing cargo argument (`--lib`,
# `--test foo`) entirely. `mutation-check.sh`'s own `--exact` match against
# the full path is the fine check this cheap one stands in front of; guard 4
# only asks "does a function by this name exist somewhere in the package",
# which is enough to catch a rename or deletion and not enough to catch a
# case that now runs a different function of the same name than the one it
# was written against.
#
# ⚠️ **Guard 4's vitest check and `mutation-check.sh`'s own selection do not
# agree on what a title IS.** `run_named_test` passes `<test-name>` to
# vitest's `-t`, which treats it as a REGULAR EXPRESSION (see the warning
# above `run_named_test` in `mutation-check.sh`); guard 4 above instead
# `grep -qF`s it — always literal. The two agree exactly as long as no title
# contains a character that means something different to each: verified by
# sweeping every vitest case in `scripts/mutations/` (91 cases, 81 distinct
# titles) for `( ) [ ] { } ? + * ^ $ | \`, none of which appear; two titles
# contain a bare `.`, which both tools currently treat the same way only
# because nothing else happens to sit where it could match differently. A
# future title carrying one of the harder metacharacters would still pass
# guard 4 (a fixed string does not care what the bytes look like) and could
# silently change what `-t` selects in `mutation-check.sh` — that half is the
# one to re-check by hand if it ever happens, not this one.
case_() {
  local label="$1" file="$2" expr="$3" marker="$4" target="$5" test="$6"
  checked=$((checked + 1))

  # Guard 4: the named test exists. Independent of the file/marker guards
  # below — a case can quote its target file correctly and still name a test
  # that was renamed or deleted out from under it.
  local runner=cargo
  shift 6
  case "${1-}" in
    runner=*) runner="${1#runner=}" ;;
  esac
  case "$runner" in
    cargo)
      names_checked=$((names_checked + 1))
      # The LAST `::` segment only — `mutation-check.sh`'s own `--exact`
      # match is the fine check against the full path; this is the cheap one,
      # and deliberately does not try to resolve module nesting.
      local want="${test##*::}"
      if find_pkg_dir "$target"; then
        local pkgdir="$PKGDIR_RESULT"
        # One `fn`-index per package, built on its first case and reused by
        # every later one — measured against the alternative (a fresh
        # `grep -r` per case) at 851 cases over roughly a dozen packages: the
        # index turns "grep the package's sources 851 times" into "grep them
        # ~12 times and a file lookup 851 times". That change alone (before
        # `find_pkg_dir` below was also cached) still cost 45s over the full
        # sweep; caching `find_pkg_dir` too brought it to the 25.2-25.9s the
        # header's timing note quotes now — close to, not equal to, `main`'s
        # own 22.6-22.7s, and the remaining ~3s is the one `grep -qxF` per
        # case just below, which a bash-only alternative measured slower than
        # (see the note there). The index is a plain list, one `fn NAME` per
        # line, and nothing here reads it as anything other than the input
        # `grep -qxF` compares a whole line against — see the warning above
        # `case_` for exactly how loose that makes this check.
        local idx="$WORK/fns-$target"
        if [ ! -f "$idx" ]; then
          grep -rhoE '\bfn +[A-Za-z0-9_]+' "$pkgdir/src" "$pkgdir/tests" 2>/dev/null \
            | sed 's/  */ /g' | sort -u > "$idx"
        fi
        # 🔴 Tried and measured worse: replacing this with a bash-only
        # `read -d '' | case` slurp of `$idx`, to avoid the fork `grep` still
        # costs on every case. It cost MORE — 30.9s over the full sweep
        # against 25.7s for the `grep -qxF` below — because bash's own glob
        # matching against a whole-file string scales worse than an external
        # `grep` optimised for exactly this. Left as `grep -qxF`, and left
        # here so nobody re-tries the same idea assuming a fork is always
        # the expensive part.
        if ! grep -qxF "fn $want" "$idx"; then
          echo "TEST NOT FOUND: $label"
          echo "   cargo, package $target: no \"fn $want\" under $pkgdir/src or $pkgdir/tests — the case names $test"
          stale=$((stale + 1))
        fi
      else
        echo "TEST NOT FOUND: $label"
        echo "   cargo, package $target: no Cargo.toml under crates/*/ or src-tauri declares this package"
        stale=$((stale + 1))
      fi
      ;;
    vitest)
      names_checked=$((names_checked + 1))
      local vfile="$REPO/ui/$target"
      if [ ! -f "$vfile" ]; then
        echo "TEST NOT FOUND: $label"
        echo "   vitest: ui/$target does not exist — the case names test $test"
        stale=$((stale + 1))
      else
        # A title's apostrophe is written `\'` in the source when it sits
        # inside a single-quoted string literal (JS escaping a quote
        # character it is nested in) — bytes a case file's <test-name> never
        # carries, because that field holds the string vitest actually
        # reports, not its source spelling. Measured: `pr9-ui.sh`'s "the job
        # subscription must die with the window" case named a real, unrenamed
        # test and still failed a plain `grep -F` for exactly this reason —
        # `Settings.jobs-teardown.test.ts` writes `the window\'s own
        # subscription…`. Unescaping `\'`, `\"` and `` \` `` before the
        # search (never touching the needle, only the haystack) is what makes
        # the fixed-string match see the same text vitest does.
        #
        # ⚠️ **Through a file, never a pipe.** `perl … "$vfile" | grep -qF …`
        # measured wrong on this very file: `grep -q` closes its end of the
        # pipe the instant it finds a match, `perl` is still writing the rest
        # of a 190KB file when that happens, the kernel delivers it SIGPIPE,
        # and this script's own `set -uo pipefail` turns perl's SIGPIPE
        # death into the PIPELINE's exit status — 141, non-zero —
        # regardless of what grep found. Eighteen real, unrenamed tests were
        # reported `TEST NOT FOUND` by exactly this, every one of them a
        # title that happens to appear early enough in a large file for
        # `perl` to still be running when `grep` stops reading. Writing the
        # unescaped copy to `$WORK` and grepping that file removes the pipe,
        # and with it the race.
        #
        # One unescaped copy per FILE, not per case: `Folders.test.ts` alone
        # backs eight cases in this repository, and re-running `perl` over
        # the whole file for each one is exactly the repeated-work shape the
        # cargo index above exists to avoid.
        local key="${target//\//_}"
        local unescaped="$WORK/vitest-unescaped-$key"
        if [ ! -f "$unescaped" ]; then
          perl -pe 's/\\([\x27"`])/$1/g' "$vfile" > "$unescaped"
        fi
        if ! grep -qF -- "$test" "$unescaped"; then
          echo "TEST NOT FOUND: $label"
          echo "   vitest: ui/$target has no test titled exactly: $test"
          stale=$((stale + 1))
        fi
      fi
      ;;
    *)
      # Unknown runner name. Not this script's boundary to police — see the
      # warning above `case_` for why a name it does not recognise is not
      # treated as either `cargo` or `vitest` — but a case landing here is
      # still made visible two ways rather than left silent: named right
      # here, and counted in the gap between `names_checked` and `checked`
      # that the final assertion below refuses to pass over.
      echo "UNRECOGNISED RUNNER: $label"
      echo "   guard 4 does not know runner=$runner and did not check whether $test exists"
      ;;
  esac

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
  # Guard 3. `s///` without `/g` always returns 0 or 1 — it stops at the
  # first match — so its own return value cannot tell "matched the one
  # intended place" from "matched a wrong place first and stopped there",
  # which is exactly the indentation bug in the header: the pattern DID
  # match, once, just not where it meant to. The question this guard answers
  # is how many places the pattern matches at all, and that needs `/g` on a
  # copy nothing else reads — appending it where it is not already there
  # turns "did it substitute" into "how many times could it have"; an
  # expression that already carries `g` is unchanged by appending it again.
  # A compound expression chaining two statements with `;` (`task-2.sh`'s
  # journal case) only gets this on its last statement — appending to a
  # string can only land at the end — so the first statement's own
  # multiplicity is not independently checked here; the one case in this
  # position is not the shape this guard exists for (two distinct removals,
  # not one ambiguous pattern), and the cost of covering it exactly is not
  # one line any more.
  local count_copy="$WORK/count-copy"
  cp "$REPO/$file" "$count_copy"
  local forced="$expr"
  expr_wants_every_match "$expr" || forced="${expr}g"
  local occurrences
  occurrences=$(perl -0pi -e "my \$mnema_subs = do { $forced }; print STDERR ((\$mnema_subs) + 0);" "$count_copy" 2>&1 1>/dev/null)

  if expr_wants_every_match "$expr"; then
    every_match_count=$((every_match_count + 1))
    if [ "$occurrences" -lt 1 ]; then
      echo "MATCHES NOTHING: $label"
      echo "   the expression carries /g and should match at least once; it matched $occurrences times"
      stale=$((stale + 1))
    fi
  elif [ "$occurrences" -ne 1 ]; then
    echo "MATCHES MORE THAN ONCE: $label"
    echo "   the pattern matches $file $occurrences times, not exactly once — it may be substituting"
    echo "   into code it was not written against"
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
echo "stale: $stale   holding no cases: $empty   hidden by a shebang: $hidden   unreadable: $unreadable   exempted by /g: $every_match_count   test names checked: $names_checked of $checked"
if [ "$names_checked" -ne "$checked" ]; then
  echo "$((checked - names_checked)) case(s) named a runner guard 4 does not recognise and were not checked for guard 4 at all — see UNRECOGNISED RUNNER above"
fi
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
# Five conditions, and four of them are the same one: **a green line must not
# be reachable by checking less.** `stale` is the finding this script is for;
# `empty`, `hidden` and `unreadable` are three ways it could otherwise report
# success over cases it never looked at, and `names_checked == checked` is a
# fourth: an unrecognised `runner=` value leaves `case_`'s runner dispatch on
# its silent default arm, and without this comparison that case would count
# towards `checked` while guard 4 said nothing about it at all — a `stale: 0`
# that is true only because one case's test name went unchecked, and this
# script's whole reason to exist is that such gaps are found here, not read
# past.
[ "$stale" -eq 0 ] && [ "$empty" -eq 0 ] && [ "$hidden" -eq 0 ] && [ "$unreadable" -eq 0 ] \
  && [ "$names_checked" -eq "$checked" ]
