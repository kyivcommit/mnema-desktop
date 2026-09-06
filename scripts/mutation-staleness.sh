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
# What it does **not** check: that the mutation compiles, or that anything
# goes red — those need a compiler and a test run. Nor, for the test-name
# guard just described, whether the test is green, whether a misspelled
# `runner=` name was used, or whether a `runner=` field was written somewhere
# other than straight after the test name — see the note above `case_` for
# why those three stay `mutation-check.sh`'s job alone.
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
find_pkg_dir() {
  local target="$1" toml
  while IFS= read -r toml; do
    if grep -qm1 -E '^name[[:space:]]*=[[:space:]]*"'"$target"'"' "$toml"; then
      dirname "$toml"
      return 0
    fi
  done < <(find "$REPO/crates" "$REPO/src-tauri" -maxdepth 2 -name Cargo.toml 2>/dev/null)
  return 1
}

# case_ <label> <file> <perl-expr> <marker> <target> <test-name> [runner=<name>] [args...]
#
# The same signature the case files are written against, so one file serves both
# tools. Guard 4, below, is the only thing that reads `<target>` and
# `<test-name>`; everything after the test name — the runner's own trailing
# arguments — still belongs to the harness and is still ignored here.
#
# ⚠️ **Three things guard 4 still cannot see, and why.** `mutation-check.sh`
# grew an optional `runner=` field so a case can name a vitest test instead of
# a cargo one:
#
#   1. A misspelled runner name (`runner=vitets`) falls through the `case`
#      below to its default arm, which checks nothing and stays silent — this
#      script does not know the set of valid runner names, `mutation-check.sh`
#      does, and refuses one with exit 2. Verified by hand: a case whose
#      seventh field reads `runner=vitets` is skipped by guard 4 exactly like
#      one with no `runner=` field naming a cargo target that happens not to
#      exist would be — both fall to the default arm, silently.
#   2. A `runner=` field written anywhere but straight after the test name is
#      never seen as a runner at all — guard 4 only inspects the seventh
#      field. Verified by hand: `case_ … target test extra-arg runner=vitest`
#      leaves guard 4 treating it as `cargo` (the default), which then asks
#      whether some workspace member is named `target` — the vitest test
#      file's path — finds none, and reports `TEST NOT FOUND` for the right
#      case but the wrong reason. `mutation-check.sh` refuses this shape
#      outright (`puts runner=… after another argument`, exit 2), which is
#      the only place the reason is stated correctly.
#   3. Whether the test is GREEN. Guard 4 is a grep for the name, not a
#      compile or a run — `mutation-check.sh`'s baseline pass is the only
#      place that requires it to pass.
#
# `mutation-check.sh` refuses all three outright — an unknown runner name and
# a misplaced `runner=` both exit 2, a named test that does not exist or is
# not green is a baseline failure and exits 1 — so a green `stale: 0` here
# still says nothing about any of the three; it only closes the fourth gap,
# where a named test had simply stopped existing and nothing outside
# `mutation-check.sh`'s own CI matrix would ever have noticed.
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
      local pkgdir
      if pkgdir=$(find_pkg_dir "$target"); then
        if ! grep -qrE "\bfn[[:space:]]+${want}[[:space:]]*\(" "$pkgdir/src" "$pkgdir/tests" 2>/dev/null; then
          echo "TEST NOT FOUND: $label"
          echo "   cargo, package $target: no \"fn $want(\" under $pkgdir/src or $pkgdir/tests — the case names $test"
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
        # and `set -o pipefail` (line 133) turns perl's SIGPIPE death into
        # the PIPELINE's exit status — 141, non-zero — regardless of what
        # grep found. Eighteen real, unrenamed tests were reported `TEST NOT
        # FOUND` by exactly this, every one of them a title that happens to
        # appear early enough in a large file for `perl` to still be running
        # when `grep` stops reading. Writing the unescaped copy to `$WORK`
        # and grepping that file removes the pipe, and with it the race.
        local unescaped="$WORK/vitest-unescaped"
        perl -pe 's/\\([\x27"`])/$1/g' "$vfile" > "$unescaped"
        if ! grep -qF -- "$test" "$unescaped"; then
          echo "TEST NOT FOUND: $label"
          echo "   vitest: ui/$target has no test titled exactly: $test"
          stale=$((stale + 1))
        fi
      fi
      ;;
    *)
      # Unknown runner name. Not this script's boundary to police — see the
      # warning above `case_` — so guard 4 checks neither existence check and
      # does not count this case towards `names_checked`, which is exactly
      # what makes that count able to fall below `checked` and be noticed.
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
echo "stale: $stale   holding no cases: $empty   hidden by a shebang: $hidden   unreadable: $unreadable   exempted by /g: $every_match_count   test names checked: $names_checked"
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
