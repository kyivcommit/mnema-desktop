#!/usr/bin/env bash
#
# Print every source citation in this repository beside the line it points at.
#
# A citation here is `some_file.rs:120` or `some_file.rs:120-140` written inside
# a comment, a doc comment or a script header — this project uses them heavily,
# because its rule is that a claim carries a `file:line` or a command. They are
# also the thing that silently rots: adding twelve lines to a file invalidates
# every citation below the insertion, in every OTHER file, and nothing compiles
# differently afterwards.
#
# Why this exists rather than a careful editor. Three fix rounds on one task
# each paid for the same defect, and the third one paid for it in the round
# whose whole subject was that defect: citations were remapped, the new anchors
# were printed and read, and then one more doc edit five lines long moved
# everything again — after the reading. What was reported was "every anchor
# printed and read", and what was in the tree was a uniform map, off by five.
# A sentence about a method is not the method. This prints the artefact.
#
# Usage:
#   scripts/check-citations.sh              # every tracked file
#   scripts/check-citations.sh <path>...    # only these citing files
#   scripts/check-citations.sh --strict     # also fail on the two shapes below
#   scripts/check-citations.sh --self-test  # the citation pattern against a
#                                           # table of strings, and nothing else
#
# Output, one block per citation:
#
#   src-tauri/src/tree.rs:715  ->  crates/mnema-walk/src/rules.rs:369
#     claim | /// 2. `Symlink` — the walk runs `follow_links(false)`
#     :369  |             .follow_links(false)
#
# **It does not decide whether a citation is right.** Whether `:369` is the
# right line for that sentence is a judgement about meaning, and the reader is
# the only one who can make it — which is the whole point: the output is a thing
# to read, not a verdict to trust. What it does decide, and exits non-zero for,
# is the mechanical half: a citation whose target file cannot be resolved, or
# whose line number is past the end of that file.
#
# **The bare `` `:NNN` `` form is caught too.** A sentence that names a file once
# and then writes `` `:336-339` `` for the next citation is the form that hid two
# citations from a hand-written sweep. Those bind to the last file named in the
# nearest preceding lines, and the binding is printed as `(bare, bound to …)` so
# a reader can check the binding itself and not only the line.
#
# **Which targets it can check.** The extensions it recognises are DERIVED from
# the tracked files themselves, never listed here. A hand-written list is what
# a checker's own blind spot looks like: the first one here allowed eight
# extensions, and every citation into `.svelte`, `.md` and `.html` produced no
# line at all — not counted, not listed, not failed. Widening it to the tracked
# set brought 26 into the sweep and three of them were wrong, one of them carrying
# the line numbers the cited file had in an EARLIER pull request — already stale
# in the commit that introduced it. Anything citation-shaped whose extension no
# tracked file carries is now listed rather than passed over, because a checker
# that goes quiet about what it cannot parse is the exact class it exists to
# catch.
#
# **What fails the run, and what is only printed.** A citation that names a real
# path and overshoots that file fails. Two shapes deliberately do not:
#
#   - **an overshoot written as a bare basename.** A bare `lib.rs` overshoot
#     inside a paragraph about `calamine` names a file this repository does not
#     have, and failing on it would teach a reader to switch the check off. The
#     cost is real and belongs in writing: the whole `tree.rs` → `bridge.rs`
#     family here is written as bare basenames rather than full paths, so the
#     citation shape this script exists for sits outside the exit code.
#   - **a target nothing here answers for.** A typo (`rulez.rs` for `rules.rs`),
#     a renamed file and a deliberate reference to `tauri-2.11.5/…` are the same
#     evidence to a script. They are printed in two lists, split by whether any
#     tracked file carries that basename, and neither fails.
#
# `--strict` promotes both to failures. It is a sweep by hand, not the CI form:
# run today it is red, and most of what it lists is deliberate — citations into
# the server repository (`app/…`) and into vendored crates, which nothing here
# can check. Whether any one of them is a defect is a reader's call, which is
# why the default prints them instead.

set -u

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO" || exit 1

STRICT=0
NAMED=0
SELFTEST=0
LIST="/tmp/.citation-files.$$"
# 🔴 Fix round 2, B6: the list file is created here and read inside python, so
# an interrupt between the two leaves it in `/tmp` forever. `$$` makes each
# leftover a new name rather than one that gets overwritten.
trap 'rm -f "$LIST"' EXIT INT TERM
: > "$LIST"
for arg in "$@"; do
  case "$arg" in
    --strict) STRICT=1 ;;
    --self-test) SELFTEST=1 ;;
    -*) echo "unknown option: $arg" >&2; exit 2 ;;
    *) printf '%s\n' "$arg" >> "$LIST"; NAMED=1 ;;
  esac
done
if [ "$NAMED" -eq 0 ]; then
  git ls-files > "$LIST"
fi

python3 - "$REPO" "$LIST" "$STRICT" "$NAMED" "$SELFTEST" <<'PY'
import os, re, sys

repo, listing = sys.argv[1], sys.argv[2]
strict, named = sys.argv[3] == "1", sys.argv[4] == "1"
selftest = sys.argv[5] == "1"
files = [f.strip() for f in open(listing) if f.strip()]
os.unlink(listing)

# Every tracked path, for resolving a cited basename to a real file.
tracked = [f for f in os.popen("git ls-files").read().split("\n") if f]

# The extensions a citation can name and still be checkable: exactly the ones
# this repository's own files carry. Longest first, so `.jsonl` is not read as
# `.json` followed by a stray `l`.
EXTS = sorted({e for e in (t.rsplit("/", 1)[-1].rsplit(".", 1)[-1]
                          for t in tracked if "." in t.rsplit("/", 1)[-1])
               if e},
              key=lambda e: (-len(e), e))
EXT_ALT = "|".join(re.escape(e) for e in EXTS)

# A path, then one line or a span of lines, optionally a comma list of either.
SPAN = r"\d+(?:-\d+)?"
# 🔴 Fix round 2, B2. The leading `\.?` is the fix, and what it cost while it was
# missing is worse than a miss: a citation into a dot-directory —
# `.github/workflows/ci.yml:189` — matched from the `g`, resolved nothing, and
# was filed as "names a path this repository does not have". Its line was then
# never range-checked at all. Measured: that same path with a five-digit line
# number nowhere near the end of the file exited 0, where the identical
# overshoot written on an ordinary path exits 1. (Not spelled out here: this
# file is swept like any other, and the overshoot would fail the run.) A checker
# that
# answers "no such file" about a file that exists is worse than one that says
# nothing — it turns a real check into a silent pass, and reports it as a
# finding about the writer.
#
# One optional dot, and then a name character: a bare `.` cannot start a stem,
# so a sentence ending in a full stop before a citation ("…the end. tree.rs:100")
# still binds to `tree.rs` and not to `. tree.rs`. `--self-test` pins both
# directions against the compiled expression rather than against this comment.
STEM = r"\.?[A-Za-z0-9_][A-Za-z0-9_./-]*"
CITED = re.compile(rf"({STEM}\.(?:{EXT_ALT})):({SPAN}(?:,{SPAN})*)")
# The same shape with ANY extension, so a target this repository has no file
# type for is reported instead of skipped in silence.
ANY_EXT = re.compile(rf"({STEM}\.([A-Za-z][A-Za-z0-9]{{0,9}})):({SPAN}(?:,{SPAN})*)")
# The bare form, always inside backticks so prose like "see 12:30" is not one.
BARE = re.compile(rf"`:({SPAN}(?:,{SPAN})*)`")

if selftest:
    # 🔴 Fix round 2, B2. A table of strings whose right answer is written beside
    # them, run against the expression COMPILED ABOVE — not a copy of it, which
    # is how a self-test comes to pass over a pattern nobody uses. The first row
    # is the defect: before the leading `\.?`, `CITED` matched from the `g` and
    # the citation was reported as naming a path this repository does not have.
    #
    # ⚠️ The colon is written `@` and swapped in at run time, deliberately: a
    # probe spelled out in full would be a citation in this file's own source,
    # and the sweep would range-check the table — the `99999` row would then
    # fail the very run it is here to protect. Found by running it.
    def probe(text):
        return text.replace("@", ":")

    PROBES = [
        (".github/workflows/ci.yml@189", [".github/workflows/ci.yml"]),
        ("`.github/workflows/ci.yml@99999` overshoots", [".github/workflows/ci.yml"]),
        ("see crates/mnema-index/src/write.rs@580", ["crates/mnema-index/src/write.rs"]),
        # The other direction, and the reason the dot is optional and followed
        # by a name character: a full stop ending a sentence is not the start of
        # a path.
        ("that is the end. tree.rs@100 follows", ["tree.rs"]),
        ("two on a line: tree.rs@10 and bridge.rs@20", ["tree.rs", "bridge.rs"]),
        # No citation at all: a time of day must not become one.
        ("the meeting is at 12@30", []),
    ]
    bad = 0
    for text, want in PROBES:
        text = probe(text)
        got = [m.group(1) for m in CITED.finditer(text)]
        if got != want:
            bad += 1
            print(f"SELF-TEST FAILURE: {text!r}\n  expected {want}\n  got      {got}")
    if bad:
        print(f"--- {bad} of {len(PROBES)} probes failed ---")
        sys.exit(1)
    print(f"--- self-test: {len(PROBES)} probes, all as written ---")
    sys.exit(0)

def shared_prefix(a, b):
    """How many leading path components two files share."""
    x, y = a.split("/"), b.split("/")
    n = 0
    while n < len(x) and n < len(y) and x[n] == y[n]:
        n += 1
    return n

def resolve(cited, citing):
    """A cited path to a tracked file, or `None` when nothing in this
    repository can answer for it.

    Ambiguity is the interesting half. This repository holds several
    `lib.rs` and two `rules.rs`, and `tray.rs` citing `lib.rs:314` means the
    one in its own crate — so a tie is broken by the longest shared path
    prefix with the CITING file, and only then by preferring `src/`. Getting
    that wrong is not harmless: it reports a real citation as out of range
    against a file it never named."""
    if cited in tracked:
        return cited, None, False
    # Matching is by full path SUFFIX only, never a fallback to the basename,
    # and that is the whole guard against the defect measured while writing
    # this: `tauri-2.11.5/src/manager/mod.rs:339` falling back to `mod.rs` and
    # being reported as an out-of-range line in `src-tauri/tests/support/mod.rs`
    # — a file that citation never mentioned. Relaxing this line to a basename
    # match brings it back.
    hits = [t for t in tracked if t.endswith("/" + cited)]
    if not hits:
        return None, None, False
    by_basename = "/" not in cited
    if len(hits) == 1:
        return hits[0], None, by_basename
    best = max(hits, key=lambda h: (shared_prefix(h, citing), "/src/" in h))
    return (best,
            f"ambiguous ({len(hits)} matches), took the nearest to the citing file",
            by_basename)

cache = {}
def lines_of(path):
    if path not in cache:
        try:
            body = open(path, encoding="utf-8", errors="replace").read().split("\n")
        except OSError:
            cache[path] = None
            return None
        # A file ending in a newline splits into a trailing empty element, and
        # counting it made the line after the last one read as in range: a
        # 20-line file reported 21 lines, and the line after its end printed a
        # blank and passed.
        if body and body[-1] == "":
            body.pop()
        cache[path] = body
    return cache[path]

def show(citing, lineno, claim, target, span, note, problems, outside,
         by_basename=False):
    body = lines_of(target)
    if body is None:
        problems.append(f"{citing}:{lineno}: cannot read {target}")
        return
    parts = span.split("-")
    a = int(parts[0]); b = int(parts[-1])
    tag = f" ({note})" if note else ""
    print(f"{citing}:{lineno}  ->  {target}:{span}{tag}")
    print(f"  claim | {claim.strip()[:110]}")
    for n in ({a, b} if b != a else {a}):
        if n < 1 or n > len(body):
            # A citation written as a bare BASENAME that lands past the end is
            # more often a file outside this repository — `lib.rs:958` inside a
            # paragraph about `calamine` — than a stale in-repo line. It is
            # ambiguous evidence either way, so it is reported for a reader
            # rather than failing the run; a citation that named a real path
            # and still overshoots is a defect and does fail.
            where = ("resolved by basename alone, so this may name a file outside "
                     "this repository" if by_basename else "stale")
            (outside if by_basename else problems).append(
                f"{citing}:{lineno}: {target}:{n} is past the end "
                f"({len(body)} lines) — {where}")
            print(f"  :{n:<4} | *** OUT OF RANGE ***")
        else:
            print(f"  :{n:<4} | {body[n - 1][:110]}")
    print()

problems, outside, absent, unchecked, unbound, unreadable = [], [], [], [], [], []
count = 0
for f in files:
    body = lines_of(f)
    if body is None:
        # A path named on the command line that cannot be read is a typo in the
        # caller, and letting it pass is how a step reports success over a
        # subject it never opened. A tracked file that cannot be read is not the
        # caller's doing, so it is listed rather than failed.
        (problems if named else unreadable).append(
            f"{f}: named on the command line but cannot be read" if named
            else f"{f}: tracked but cannot be read, so it was not scanned")
        continue
    # `None` means "nothing has been named"; `False` means "the last thing
    # named is not in this repository". The difference matters: without it a
    # bare `:1414` following a citation into a vendored crate binds to
    # whichever file was named before THAT, and is then reported out of range
    # against a file the sentence never mentioned. Measured on
    # `src-tauri/src/lib.rs:146`, where it named `state.rs`.
    last_file, last_seen = None, -99
    for i, line in enumerate(body, start=1):
        checkable = []
        for m in CITED.finditer(line):
            checkable.append(m.span())
            target, note, by_basename = resolve(m.group(1), f)
            last_seen = i
            if target is None:
                last_file = False
                bucket = outside if "/" in m.group(1) else absent
                why = ("names a path this repository does not have"
                       if "/" in m.group(1) else
                       "no tracked file carries that basename")
                bucket.append(f"{f}:{i}: {m.group(1)}:{m.group(2)} — {why}")
                continue
            last_file = target
            for span in m.group(2).split(","):
                show(f, i, line, target, span, note, problems, outside,
                     by_basename)
                count += 1
        for m in ANY_EXT.finditer(line):
            # Citation-shaped, but naming a file type no tracked file carries.
            # It cannot be checked; the one thing it must not do is vanish.
            if any(s < m.end() and m.start() < e for s, e in checkable):
                continue
            unchecked.append(f"{f}:{i}: {m.group(1)}:{m.group(3)} — no tracked file "
                             f"has the extension `.{m.group(2)}`, so nothing here "
                             f"can check it")
        for m in BARE.finditer(line):
            # Bound to the last file named within a few lines — one comment
            # block. Printed, so the binding is checkable and not assumed.
            if last_file is None or i - last_seen > 6:
                unbound.append(f"{f}:{i}: bare `:{m.group(1)}` — no file named nearby, "
                               f"so probably not a citation")
                continue
            if last_file is False:
                outside.append(f"{f}:{i}: bare `:{m.group(1)}` — follows a citation "
                               f"into a file outside this repository")
                continue
            for span in m.group(1).split(","):
                show(f, i, line, last_file, span,
                     f"bare, bound to {last_file} named at :{last_seen}",
                     problems, outside)
                count += 1

print(f"--- {count} citations printed ---")
for label, rows in (
        ("outside this repository, nothing here can check them", outside),
        ("naming a basename no tracked file carries — a typo, a rename, or a "
         "file outside this repository", absent),
        ("citation-shaped but of a file type this repository does not hold",
         unchecked),
        ("bare `:NNN` with no file nearby, so read as prose rather than a citation",
         unbound),
        ("tracked but unreadable, so nothing in them was scanned", unreadable)):
    if rows:
        print(f"--- {len(rows)} {label} ---")
        for r in rows:
            print(f"  {r}")
if strict:
    # `--strict` is the sweep form: the two shapes the default prints for a
    # reader become failures, and the reader is the exit code.
    problems.extend(outside)
    problems.extend(absent)
if problems:
    print(f"--- {len(problems)} mechanical problem(s) ---")
    for p in problems:
        print(f"  {p}")
    sys.exit(1)
print("--- every resolvable citation points inside its file ---")
print("--- whether each line is the RIGHT one is for a reader, which is the point ---")
PY
