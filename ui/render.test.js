// The window's own test suite. Runs with `node --test render.test.js` — no
// browser, no mocked `window.__TAURI__`, because everything in `render.js`
// is a pure function of the payload a command actually returned.
//
// Written after a fix round found the Critical this suite exists to keep
// found: `!complete` alone gated the reconciliation warning, so a walk
// cancelled after phase 1 had already enumerated the whole tree — which has
// `complete: true` — showed no warning at all. Nothing before this suite
// would have caught that regressing; this is what would.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  STOPPED_CLEANLY,
  reconciliationRan,
  ENDING_TEXT,
  FROZEN_REASON_TEXT,
  endingSentence,
  hitLocation,
  searchResultItems,
} from "./render.js";

// The canonical list of `EndReason` discriminants, in the exact camelCase
// spelling `#[serde(rename_all = "camelCase")]` produces. Pinned identically
// in `src-tauri/src/job.rs`'s `every_end_reason_has_its_camel_case_spelling_
// pinned` — that test fails to compile if a variant is added to `EndReason`
// without a matching arm, which is what forces whoever adds one to notice
// this list needs the same addition. Neither side can see the other's
// source; keeping them in step is a habit this comment asks for, not
// something either language enforces on its own.
const END_REASONS = [
  "completed",
  "cancelled",
  "failed",
  "brokenWorker",
  "rulesNotApplied",
  "rootUnavailable",
  "volumeMissing",
];

// Same pairing, for `FrozenReason` — mirrors `job.rs`'s `every_frozen_
// reason_has_its_camel_case_spelling_pinned`.
const FROZEN_REASONS = ["symlinkedSubtree", "emptyDirectory", "unreadableDirectory"];

test("every EndReason has a table entry in both ENDING_TEXT and STOPPED_CLEANLY", () => {
  assert.deepEqual(Object.keys(ENDING_TEXT).sort(), [...END_REASONS].sort());
  assert.deepEqual(Object.keys(STOPPED_CLEANLY).sort(), [...END_REASONS].sort());
});

test("every FrozenReason has a table entry in FROZEN_REASON_TEXT", () => {
  assert.deepEqual(Object.keys(FROZEN_REASON_TEXT).sort(), [...FROZEN_REASONS].sort());
});

// The reconciliation predicate, table-driven over every (reason, complete)
// pair — the shape of test that catches an eighth `StopReason` reusing a
// `complete`-only shortcut, which a single hand-picked example cannot.
// Mirrors `crates/mnema-ingest/src/walk.rs`'s own `stopped_cleanly` match:
// only `Completed` lets phase 3 run, and only when `complete` is also true.
test("reconciliationRan is true only for completed && complete", () => {
  for (const reason of END_REASONS) {
    for (const complete of [true, false]) {
      const expected = reason === "completed" && complete;
      assert.equal(
        reconciliationRan({ reason, complete }),
        expected,
        `reason=${reason} complete=${complete} should be ${expected}`
      );
    }
  }
});

test("an unrecognised reason does not run reconciliation, even with complete: true", () => {
  // The cautious default: a reason this page does not know is not one it can
  // claim finished cleanly.
  assert.equal(reconciliationRan({ reason: "somethingFutureAndUnknown", complete: true }), false);
});

// The exact regression this suite exists for: `mnema-ingest/tests/walk.rs`'s
// own throwaway test (fix round 1's report) confirmed a walk cancelled after
// phase 1 had already enumerated 20 files comes back `complete: true`. The
// old code (`!ending.complete` alone) showed no warning for this. This does.
test("a cancelled walk with complete: true still gets the reconciliation warning", () => {
  const ended = {
    reason: "cancelled",
    done: 2,
    total: 20,
    complete: true,
    skipped: 0,
    indexed: 0,
    unchanged: 0,
    frozen: [],
    message: null,
  };
  assert.equal(reconciliationRan(ended), false);
  assert.match(endingSentence(ended), /reconciliation did not run/);
});

test("a clean, completed walk carries no reconciliation warning", () => {
  const ended = {
    reason: "completed",
    done: 8,
    total: 8,
    complete: true,
    skipped: 0,
    indexed: 5,
    unchanged: 3,
    frozen: [],
    message: null,
  };
  const sentence = endingSentence(ended);
  assert.doesNotMatch(sentence, /reconciliation did not/);
  assert.equal(sentence, "finished: 5 added, 3 unchanged (8 total)");
});

// Fix round 2's own finding: phase 3 deletes one path per transaction, so a
// `failed` walk may have deleted some paths before the transaction that
// raised the error. "Nothing was removed" would be false for that shape;
// "did not finish" is not.
test("a failed walk says reconciliation did not finish, not that nothing was removed", () => {
  const ended = {
    reason: "failed",
    done: 3,
    total: 10,
    complete: false,
    skipped: 0,
    indexed: 0,
    unchanged: 0,
    frozen: [],
    message: "the extraction pool cannot continue: boom",
  };
  const sentence = endingSentence(ended);
  assert.match(sentence, /reconciliation did not finish/);
  assert.doesNotMatch(sentence, /nothing was removed/);
});

test("rulesNotApplied does not append a skipped count, even when refused > 0", () => {
  const ended = {
    reason: "rulesNotApplied",
    done: 0,
    total: 0,
    complete: false,
    skipped: 3,
    indexed: 0,
    unchanged: 0,
    frozen: [],
    message: null,
  };
  assert.doesNotMatch(endingSentence(ended), /skipped/);
});

test("every other reason still appends the skipped count", () => {
  const ended = {
    reason: "cancelled",
    done: 4,
    total: 10,
    complete: true,
    skipped: 4,
    indexed: 0,
    unchanged: 0,
    frozen: [],
    message: null,
  };
  assert.match(endingSentence(ended), /4 skipped/);
});

test("a hit's relativePath: null renders as a state, not an empty string", () => {
  assert.equal(hitLocation({ relativePath: null, text: "x" }), "(no path on disk)");
  assert.equal(hitLocation({ relativePath: "notes/a.txt", text: "x" }), "notes/a.txt");
});

test("zero search hits render an explicit no-matches item, not an empty list", () => {
  assert.deepEqual(searchResultItems([]), [{ kind: "empty", text: "no matches" }]);
});

test("search hits render their location and text", () => {
  assert.deepEqual(searchResultItems([{ relativePath: "a.txt", text: "fox" }]), [
    { kind: "hit", where: "a.txt", text: "fox" },
  ]);
});
