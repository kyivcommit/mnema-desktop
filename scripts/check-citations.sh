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

set -u

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO" || exit 1

if [ "$#" -gt 0 ]; then
  printf '%s\n' "$@" > /tmp/.citation-files.$$
else
  git ls-files > /tmp/.citation-files.$$
fi

python3 - "$REPO" "/tmp/.citation-files.$$" <<'PY'
import os, re, sys

repo, listing = sys.argv[1], sys.argv[2]
files = [f.strip() for f in open(listing) if f.strip()]
os.unlink(listing)

# Every tracked path, for resolving a cited basename to a real file.
tracked = [f for f in os.popen("git ls-files").read().split("\n") if f]

# `name.ext:120` or `name.ext:120-140`, optionally a comma list of either.
SPAN = r"\d+(?:-\d+)?"
CITED = re.compile(rf"([A-Za-z0-9_][A-Za-z0-9_./-]*\.(?:rs|sql|ts|tsx|py|toml|yml|sh)):({SPAN}(?:,{SPAN})*)")
# The bare form, always inside backticks so prose like "see 12:30" is not one.
BARE = re.compile(rf"`:({SPAN}(?:,{SPAN})*)`")

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
    hits = [t for t in tracked if t.endswith("/" + cited)]
    # A cited token that carries directories names a PATH, and if no tracked
    # file ends with it the path is not in this repository — falling back to
    # its basename is how `tauri-2.11.5/src/manager/mod.rs:339` gets reported
    # as an out-of-range line in `src-tauri/tests/support/mod.rs`, which is a
    # file that citation never mentioned. Measured while writing this.
    if not hits and "/" not in cited:
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
            cache[path] = open(path, encoding="utf-8", errors="replace").read().split("\n")
        except OSError:
            cache[path] = None
    return cache[path]

def show(citing, lineno, claim, target, span, note, problems, by_basename=False):
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

problems, outside, unbound, count = [], [], [], 0
show.__globals__["outside"] = outside
for f in files:
    body = lines_of(f)
    if body is None:
        continue
    # `None` means "nothing has been named"; `False` means "the last thing
    # named is not in this repository". The difference matters: without it a
    # bare `:1414` following a citation into a vendored crate binds to
    # whichever file was named before THAT, and is then reported out of range
    # against a file the sentence never mentioned. Measured on
    # `src-tauri/src/lib.rs:146`, where it named `state.rs`.
    last_file, last_seen = None, -99
    for i, line in enumerate(body, start=1):
        for m in CITED.finditer(line):
            target, note, by_basename = resolve(m.group(1), f)
            last_seen = i
            if target is None:
                last_file = False
                outside.append(f"{f}:{i}: {m.group(1)}:{m.group(2)} — not in this repository")
                continue
            last_file = target
            for span in m.group(2).split(","):
                show(f, i, line, target, span, note, problems, by_basename); count += 1
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
                     f"bare, bound to {last_file} named at :{last_seen}", problems)
                count += 1

print(f"--- {count} citations printed ---")
for label, rows in (("outside this repository, nothing here can check them", outside),
                    ("bare `:NNN` with no file nearby, so read as prose rather than a citation",
                     unbound)):
    if rows:
        print(f"--- {len(rows)} {label} ---")
        for r in rows:
            print(f"  {r}")
if problems:
    print(f"--- {len(problems)} mechanical problem(s) ---")
    for p in problems:
        print(f"  {p}")
    sys.exit(1)
print("--- every resolvable citation points inside its file ---")
print("--- whether each line is the RIGHT one is for a reader, which is the point ---")
PY
