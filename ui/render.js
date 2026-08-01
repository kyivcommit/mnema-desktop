// The window's decisions, apart from the window's drawing of them.
//
// Nothing here touches the DOM or calls `invoke`. That is what makes this
// file importable from `node --test` (`render.test.js`) without a browser or
// a mocked `window.__TAURI__` — a fix round found the Critical bug this file
// exists to keep from happening again by reading the code, not by running
// it, which is exactly the shape of mistake a test on real payloads catches
// and a reading does not.

// Whether `report.stopped` let phase 3 (reconciliation) run, mirroring
// `crates/mnema-ingest/src/walk.rs`'s own `stopped_cleanly` match — a table
// over every `EndReason`, not a shortcut comparison against one of them. The
// core's own comment above that match is the reason this is a table and not
// `reason === "completed"` read as if it were the whole condition:
// `walked.complete` and `report.stopped` are answers to two different
// questions, and neither implies the other.
export const STOPPED_CLEANLY = {
  completed: true,
  cancelled: false,
  failed: false,
  brokenWorker: false,
  rulesNotApplied: false,
  rootUnavailable: false,
  volumeMissing: false,
};

// Reconciliation ran only when phase 1 saw the whole tree (`complete`) AND
// phase 2 finished everything phase 1 handed it (`STOPPED_CLEANLY[reason]`)
// — the exact `walked.complete && stopped_cleanly` the core gates phase 3 on.
// An unrecognised `reason` defaults to "did not run cleanly": the cautious
// side to be wrong on, the same choice `Ended::failed`'s own doc comment
// makes about `complete` at the seam.
export function reconciliationRan(ended) {
  return Boolean(ended.complete) && (STOPPED_CLEANLY[ended.reason] ?? false);
}

// One sentence per `FrozenReason` — owned here, not in the core, which
// deliberately has no `Display` for it (`src-tauri/src/job.rs::FrozenReason`).
// `emptyDirectory` is a question, not a statement: nothing on either side of
// the seam — not `st_dev`, not this window — can tell "you emptied this
// folder on purpose" from "the share it lives on went offline", and stating
// either as fact would be answering a question nobody asked this window.
export const FROZEN_REASON_TEXT = {
  symlinkedSubtree: (prefix) =>
    `${prefix} is a symlink to a directory, which the walk does not follow — it has no ` +
    "evidence about what used to be there before it became one.",
  emptyDirectory: (prefix) =>
    `${prefix} now looks empty — did you empty it on purpose, or could the drive it lives on ` +
    "have gone offline? Either way, nothing under it was removed from the index this run; " +
    "check it by hand.",
  unreadableDirectory: (prefix) => `${prefix} could not be read, most likely a permissions problem.`,
};

// A `reason` this page does not know is still a folder reconciliation left
// alone — the same principle `ENDING_TEXT`'s own fallback below follows for
// an unknown `EndReason`.
export const frozenSentence = (f) =>
  (FROZEN_REASON_TEXT[f.reason] ?? ((prefix) => `${prefix}: left untouched by cleanup`))(f.prefix);

// One sentence per `EndReason`. `rulesNotApplied`, `rootUnavailable`,
// `brokenWorker`, `volumeMissing` and `cancelled` read as five different
// things because they are five different things — collapsing any pair of
// them back into one shared sentence would be the same mistake `reason:
// "failed"` used to make about a missing worker, a broken pool and a panic.
//
// `rulesNotApplied` in particular is worded as a guarantee, not an apology:
// under D29 indexing sends document text to a third-party provider, so a
// walk that refuses to start because it could not apply its own exclusion
// rules is refusing to send anything that might have been excluded — that is
// what "nothing … was opened or sent" below is claiming, and it is true
// precisely because `walk_root` returns before phase 1 runs at all for this
// `StopReason`.
export const ENDING_TEXT = {
  // `removed` always shows, the same way `indexed` and `unchanged` always
  // do: `WalkReport::removed` (`crates/mnema-ingest/src/walk.rs`) stays `0`
  // whenever phase 3 refused to run, so it is never misleading to print,
  // and it is the only count in this sentence that answers "where did my
  // file go?" A window that dropped it at this seam left "finished: 0
  // added, 12 unchanged (12 total)" as the whole story for a walk that had
  // just deleted four hundred `path` rows.
  completed: ({ indexed, unchanged, removed, total }) =>
    `finished: ${indexed} added, ${unchanged} unchanged, ${removed} removed (${total} total)`,
  cancelled: ({ done, total }) => `stopped after ${done} of ${total}, at your request`,
  failed: ({ done, total, message }) =>
    message ? `failed after ${done} of ${total}: ${message}` : `failed after ${done} of ${total}`,
  brokenWorker: ({ done, total }) =>
    `stopped after ${done} of ${total} — the extraction worker looked broken and could not ` +
    "be trusted to continue",
  rulesNotApplied: () =>
    "stopped before reading a single file: the exclusion rules could not be applied, so " +
    "nothing in this folder was opened or sent to the extraction service",
  rootUnavailable: () => "the folder could not be reached at all, before the walk saw a single file",
  // A question, not a statement, for the same reason `FROZEN_REASON_TEXT.
  // emptyDirectory` is one: `mnema-ingest/src/walk.rs` names this exact
  // ambiguity — a folder that genuinely shrank to `done` files and one whose
  // volume went offline partway through look identical from here — in the
  // same words it uses for an emptied directory. "finished" would say more
  // than is known; "stopped" does not.
  volumeMissing: ({ done, total }) =>
    `stopped after ${done} of ${total} — did the folder genuinely have only that many files ` +
    "left, or could it have been unmounted partway through? Nothing on this side can tell the " +
    "two apart.",
};

// What to say about reconciliation not having run — appended after
// `ENDING_TEXT`'s own sentence, never inside it, so every reason's sentence
// stays about what happened and this stays about what it means for the
// index.
//
// `failed` gets different words than every other non-`completed` reason.
// Phase 3 deletes one path per transaction, deliberately not the whole
// reconciliation under one (`crates/mnema-ingest/src/walk.rs`'s own comment
// above that loop: a large root would otherwise hold the write lock for as
// long as the entire reconciliation takes). An `IngestError` raised by one of
// those transactions — a real, if narrow, way to reach `reason: "failed"` —
// stops the loop with some paths already deleted and others not: "nothing
// was removed" would be false for exactly that shape, which none of the
// other six reasons can produce, since every one of them either never
// reaches phase 3 at all or reaches it and lets it finish. "Did not finish"
// and "may now be out of step, in either direction" are the two things this
// window can still say without claiming which paths, if any, were reached.
function reconciliationClause(ended) {
  if (reconciliationRan(ended)) {
    return "";
  }
  if (ended.reason === "failed") {
    return (
      " (reconciliation did not finish this run — the index may now be out of step with the " +
      "folder, in a way this window cannot tell)"
    );
  }
  return (
    " (reconciliation did not run this time, so nothing was removed from the index — a file " +
    "deleted from the folder could still answer a search)"
  );
}

// The full sentence for one `Ended` payload: `ENDING_TEXT`'s own sentence,
// then `skipped` (except for `rulesNotApplied`, whose own sentence already
// says "before reading a single file" — a count after that would contradict
// it, since what `skipped` can still carry there is `refused`, phase 1's own
// pre-skip journal, not anything the exclusion rules decided), then whether
// reconciliation ran, then one line per folder reconciliation declined to
// touch.
export function endingSentence(ended) {
  const say = ENDING_TEXT[ended.reason];
  let text = say
    ? say(ended)
    : // A reason this page does not know is still an ending. Rendering the
      // literal `undefined` would be the page inventing a word.
      `ended (${ended.reason}) after ${ended.done} of ${ended.total}`;

  if (ended.skipped && ended.reason !== "rulesNotApplied") {
    text += `, ${ended.skipped} skipped`;
  }
  text += reconciliationClause(ended);
  if (ended.frozen && ended.frozen.length) {
    text += " " + ended.frozen.map(frozenSentence).join(" ");
  }
  return text;
}

// What a search hit's location line reads. `relativePath` is `null` for a
// document whose last copy on disk is gone — that is a state, not an empty
// string, and it must not render as one.
export function hitLocation(hit) {
  return hit.relativePath ?? "(no path on disk)";
}

// What `search`'s result list is made of, as plain data rather than DOM
// nodes — `main.js` is what turns this into elements. Zero hits is an
// answer, not the absence of one: `replaceChildren()` with nothing appended
// looks identical to the button having done nothing, on the one manual
// acceptance path this window has no test behind.
export function searchResultItems(hits) {
  if (hits.length === 0) {
    return [{ kind: "empty", text: "no matches" }];
  }
  return hits.map((h) => ({ kind: "hit", where: hitLocation(h), text: h.text }));
}
