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
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  STOPPED_CLEANLY,
  reconciliationRan,
  ENDING_TEXT,
  FROZEN_REASON_TEXT,
  endingSentence,
  hitLocation,
  searchResultItems,
  ROLES,
  ROLE_NAME,
  DISCLOSURE_TEXT,
  LEAVES_NOTHING,
  LEAVES_EVERYTHING,
  KEY_STATE_TEXT,
  asStatusSentence,
  keyStoreNote,
  KEY_STORE_FAILURE_TEXT,
  KEY_STORE_SHOWS_REASON,
  INDEX_SETTINGS_TEXT,
  UNREADABLE_CAUSE_TEXT,
  INDEX_OPENING_TEXT,
  LIST_STATE_NOTE,
  MISSING_MODEL_REASON,
  REFUSAL_TEXT,
  BALANCE_TEXT,
  RECORD_ID_TEXT,
  PRICE_TEXT,
  INPUT_LIMIT_TEXT,
  KEY_REMOVAL_TEXT,
  EMPTY_FIELD_TEXT,
  KEY_FIELD_PLACEHOLDER,
  KEY_SUBMIT_TEXT,
  indexNotAsked,
  indexOpened,
  indexOpenFailed,
  listNotAsked,
  listWasRead,
  listFailed,
  selectId,
  missingModelReason,
  disclosureSentence,
  keyStateSentence,
  keyNotSavedSentence,
  indexStateSentence,
  embeddingProgressText,
  adoptedModelSentence,
  modelOptionLabel,
  keyAcceptedSentence,
  unreadableSentence,
  catalogueSentence,
  roleRecordedSentence,
  recordedNoteSentence,
  keyRemovedSentence,
  keyNotRemovedSentence,
  keyNotAsked,
  emptyFieldSentence,
  keyFieldPlaceholder,
  keySubmitText,
  listNotReadSentence,
  embeddingModelNotRecordedSentence,
  roleNotRecordedSentence,
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
    removed: 0,
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
    removed: 2,
    frozen: [],
    message: null,
  };
  const sentence = endingSentence(ended);
  assert.doesNotMatch(sentence, /reconciliation did not/);
  assert.equal(sentence, "finished: 5 added, 3 unchanged, 2 removed (8 total)");
});

// The branch review's own scenario: four hundred files moved out of the
// watched folder. Phase 3 correctly deletes the four hundred `path` rows,
// but before this test the window had no field to read that back from —
// `finished: 0 added, 12 unchanged (12 total)` said nothing about it, making
// four hundred deletions indistinguishable from a walk that touched nothing.
test("a completed walk that deleted paths says how many, not just added and unchanged", () => {
  const ended = {
    reason: "completed",
    done: 12,
    total: 12,
    complete: true,
    skipped: 0,
    indexed: 0,
    unchanged: 12,
    removed: 400,
    frozen: [],
    message: null,
  };
  assert.equal(endingSentence(ended), "finished: 0 added, 12 unchanged, 400 removed (12 total)");
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
    removed: 0,
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
    removed: 0,
    frozen: [],
    message: null,
  };
  // Both directions. `doesNotMatch` alone is satisfied by a function that
  // returns the empty string, and this branch has now produced six tests that
  // passed for exactly that kind of reason — including, before this line, one
  // inside the harness written to catch them.
  const sentence = endingSentence(ended);
  assert.match(sentence, /before reading a single file/);
  assert.doesNotMatch(sentence, /skipped/);
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
    removed: 0,
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

// ─────────────────────────────────────────────────────────────────────────────
// Model configuration.
//
// The canonical discriminant lists for every union the model-configuration
// commands send this window, in the exact camelCase spelling
// `#[serde(rename_all = "camelCase")]` produces.
//
// This is the JS half of an obligation that until now sat only in a Rust test's
// doc comment — `every_discriminant_the_window_sees_has_its_camel_case_spelling_
// pinned` (`src-tauri/src/models.rs`), which says the mirrored lists belong here
// and could not be written before a renderer existed, because a JS list nothing
// asserts against goes stale while looking authoritative. What makes these
// worth having is the paragraph below them: every one is asserted to be exactly
// the key set of a render table, so a variant added on one side and not the
// other reddens something rather than rendering a silent fallback.
//
// ⚠️ Two of the four Rust-side pins stop `models.rs` compiling when a variant is
// added, which is what sends whoever adds one here. **`Refusal`, `Balance` and
// `RecordId` have no such pin** — they live in `crates/mnema-provider` and no
// test there enumerates their spellings — so those three lists are held by this
// file alone. That gap is real and is named in this task's report; do not read
// the symmetry of the lists below as evidence it is closed.
const KEY_STATES = ["present", "absent", "unreadable"];
const KEY_STORE_FAILURES = ["locked", "duplicate", "refused", "defect"];
const INDEX_SETTINGS_KINDS = ["read", "unreadable"];
const UNREADABLE_CAUSES = ["notOpen", "readFailed"];
const REFUSALS = [
  "inputTooSmall",
  "noStatedLimit",
  "limitNotUnderstood",
  "noStatedOutputModalities",
  "noTextOutput",
];
const BALANCES = ["known", "notStated", "unreadable", "envelopeNotUnderstood"];
const RECORD_IDS = ["absent", "notAString", "known"];
// The two the first live run added, and the two whose Rust side is `Price` and
// `InputLimit` — pinned in `models.rs` beside `Refusal`, `Balance` and
// `RecordId`, and held here for the same reason as those three.
const PRICES = ["known", "notStated", "notAPrice", "unreadable"];
const INPUT_LIMITS = ["known", "notStated", "notUnderstood"];
// What `forget_key` answers. It answered `Ok(())` to two events until the
// whole-branch review, and this window turned both into "the key was removed".
const KEY_REMOVALS = ["removed", "nothingToRemove"];
// `KeyState`'s three, plus the one the window holds before it has asked. Not a
// wire union: the core always knows what it measured, and `notAsked` is a fact
// about this page — the same shape as `INDEX_OPENINGS` and `LIST_STATES`.
const KEY_FIELD_STATES = ["notAsked", ...KEY_STATES];

// Two unions `main.js` produces itself rather than reading off the wire: what
// `open_index` answered, and what one role's `provider_models` call answered.
// Listed here for the same reason as the others — the table that renders each
// must have an arm for every state the window can be in — and with one addition
// the wire unions do not need: the *values* `main.js` writes are asserted
// against these lists too, through the constructors it is made to use. A
// mistyped string literal on that side used to fall through a fallback and
// redden nothing, which is the one hole left in this mechanism.
const INDEX_OPENINGS = ["notAsked", "opened", "failed"];
const LIST_STATES = ["notAsked", "read", "failed"];

// Not a wire union either, and not a state of the window: the three answers to
// "why is a recorded model not in the picker", derived from the catalogue the
// call returned. Two of them are statements about the provider and one is the
// refusal to make either.
const MISSING_MODEL_REASONS = ["unreadableRecord", "withdrawn", "unknown"];

test("every union that reaches this window has exactly one table entry per variant", () => {
  assert.deepEqual(Object.keys(DISCLOSURE_TEXT).sort(), [...KEY_STATES].sort());
  assert.deepEqual(Object.keys(KEY_STATE_TEXT).sort(), [...KEY_STATES].sort());
  assert.deepEqual(Object.keys(KEY_STORE_FAILURE_TEXT).sort(), [...KEY_STORE_FAILURES].sort());
  assert.deepEqual(Object.keys(KEY_STORE_SHOWS_REASON).sort(), [...KEY_STORE_FAILURES].sort());
  assert.deepEqual(Object.keys(INDEX_SETTINGS_TEXT).sort(), [...INDEX_SETTINGS_KINDS].sort());
  assert.deepEqual(Object.keys(UNREADABLE_CAUSE_TEXT).sort(), [...UNREADABLE_CAUSES].sort());
  assert.deepEqual(Object.keys(INDEX_OPENING_TEXT).sort(), [...INDEX_OPENINGS].sort());
  assert.deepEqual(Object.keys(LIST_STATE_NOTE).sort(), [...LIST_STATES].sort());
  assert.deepEqual(Object.keys(MISSING_MODEL_REASON).sort(), [...MISSING_MODEL_REASONS].sort());
  assert.deepEqual(Object.keys(REFUSAL_TEXT).sort(), [...REFUSALS].sort());
  assert.deepEqual(Object.keys(BALANCE_TEXT).sort(), [...BALANCES].sort());
  assert.deepEqual(Object.keys(RECORD_ID_TEXT).sort(), [...RECORD_IDS].sort());
  assert.deepEqual(Object.keys(PRICE_TEXT).sort(), [...PRICES].sort());
  assert.deepEqual(Object.keys(INPUT_LIMIT_TEXT).sort(), [...INPUT_LIMITS].sort());
  assert.deepEqual(Object.keys(KEY_REMOVAL_TEXT).sort(), [...KEY_REMOVALS].sort());
  assert.deepEqual(Object.keys(EMPTY_FIELD_TEXT).sort(), [...KEY_FIELD_STATES].sort());
  assert.deepEqual(Object.keys(KEY_FIELD_PLACEHOLDER).sort(), [...KEY_FIELD_STATES].sort());
  assert.deepEqual(Object.keys(KEY_SUBMIT_TEXT).sort(), [...KEY_FIELD_STATES].sort());
  assert.deepEqual(Object.keys(ROLE_NAME).sort(), [...ROLES].sort());
});

// The half a key-set test cannot reach on its own: `INDEX_OPENING_TEXT` had
// three correct arms while `main.js` wrote the three values as its own string
// literals, so `"opened"` mistyped as `"opend"` fell through
// `UNREADABLE_CAUSE_TEXT.notOpen`'s fallback — silently, and on exactly the
// state the order asked to be made visible. `main.js` now imports these
// constructors by name, which fails at link time rather than at the fallback.
test("the states main.js writes are exactly the states these tables render", () => {
  assert.deepEqual(
    [indexNotAsked(), indexOpened(), indexOpenFailed("boom")].map((s) => s.kind).sort(),
    [...INDEX_OPENINGS].sort(),
  );
  assert.deepEqual(
    [listNotAsked(), listWasRead(cleanCatalogue), listFailed()].map((s) => s.kind).sort(),
    [...LIST_STATES].sort(),
  );
  assert.equal(indexOpenFailed("boom").error, "boom", "the wall's reason must survive the trip");
  assert.ok(KEY_FIELD_STATES.includes(keyNotAsked().kind),
    "the state main.js starts in must be one the three key-field tables render");
});

test("the disclosure names the search query, not only indexing", () => {
  const withKey = disclosureSentence({ kind: "present" });
  assert.match(withKey, /every question/,
    "§3.2 of the requirements says 'once, at indexing', and that is false for cloud embeddings");
  // `/i`, because the phrase opens the sentence. The brief's own `/нічого/`
  // against `Нічого` was this same mistake and nothing caught it; here the
  // assertion went red on the first run.
  assert.match(withKey, /every piece/i);
});

test("with no key the disclosure promises nothing leaves", () => {
  assert.match(disclosureSentence({ kind: "absent" }), /nothing/i);
});

// `KeyState` has three values, and a store that would not answer is not a store
// that answered "no key". Promising that nothing leaves the machine, on the
// evidence of a keychain that is merely locked, is a promise this window is in
// no position to make — and the same sentence a key that *is* there would make
// false.
test("a key store that would not answer promises neither everything nor nothing", () => {
  const unknown = disclosureSentence({
    kind: "unreadable",
    cause: "locked",
    reason: "synthetic: the keychain is locked",
  });
  assert.notEqual(unknown, LEAVES_NOTHING);
  assert.notEqual(unknown, LEAVES_EVERYTHING);
  assert.match(unknown, /unknown/);
});

test("a key state this build does not know promises neither everything nor nothing", () => {
  const unknown = disclosureSentence({ kind: "somethingFutureAndUnknown" });
  assert.notEqual(unknown, LEAVES_NOTHING);
  assert.notEqual(unknown, LEAVES_EVERYTHING);
});

// The sentence `Error::NoKey`'s own doc calls forbidden: telling someone whose
// keychain is merely locked that they have entered no key sends them to re-enter
// one they already have. Both directions — `doesNotMatch` alone is satisfied by
// a function returning the empty string, and `absent` is the state that must
// say it.
test("an unreadable key store is never rendered as having no key", () => {
  for (const cause of KEY_STORE_FAILURES) {
    const text = keyStateSentence({ kind: "unreadable", cause, reason: "synthetic diagnostic text" });
    assert.doesNotMatch(text, /no key/i, `"${cause}" told a person with a key that they have none`);
    assert.match(text, /unknown/, `"${cause}" said nothing about what is not known`);
  }
  assert.match(keyStateSentence({ kind: "absent" }), /no key/i);
});

// `KeyStoreFailure` is four values over six error variants, grouped by what the
// person does next. Four sentences that read the same would satisfy the key-set
// test above and lose the whole content of the grouping.
test("each key store failure asks the person for a different thing", () => {
  const said = KEY_STORE_FAILURES.map((cause) =>
    keyStateSentence({ kind: "unreadable", cause, reason: "" }),
  );
  assert.equal(new Set(said).size, KEY_STORE_FAILURES.length, `two failures read alike: ${said}`);
  assert.match(said[KEY_STORE_FAILURES.indexOf("locked")], /unlock it/);
  assert.match(said[KEY_STORE_FAILURES.indexOf("duplicate")], /remove the spare/);
  assert.match(said[KEY_STORE_FAILURES.indexOf("defect")], /defect/);
});

// Measured 2026-08-11 on three platforms: `locked` is one value over at least
// two situations whose actions differ. macOS reaches it with a perfectly
// unlocked keychain, when the authorisation dialog was declined — that store
// authorises against the code identity that wrote the credential, and an ad-hoc
// signature is a hash of the binary, so it changes with every build. Linux
// reaches it the same way ("SS error: prompt dismissed", measured after 279
// seconds of waiting for a human) as well as by a genuinely locked collection.
// The old sentence prescribed unlocking for both, which describes nothing for
// the one somebody is most likely to be in.
//
// What this test can see is narrow, and worth saying plainly: it holds that both
// situations are named. It cannot see that the build has no way to tell them
// apart — the platform error arrives already flattened into `PlatformFailure` —
// which is the reason naming both is the honest answer rather than a hedge.
test("a store that would not answer names both of the things that cause it", () => {
  const locked = keyStateSentence({ kind: "unreadable", cause: "locked", reason: "" });
  assert.match(locked, /unlock/i, "a locked store is one of the two, and unlocking is its action");
  assert.match(
    locked,
    /confirmation/i,
    "a confirmation that was not given is the other, and is what macOS reaches after a rebuild",
  );
});

// `reason` is diagnostic text, not a sentence to show: `mnema_secrets::Error::
// Unavailable` interpolates the platform's own error, and an OS status put in
// front of a person is not an action. It is appended only where `cause` names no
// action of its own and a bug report is the action.
test("a locked keychain shows an action, not the operating system's status code", () => {
  const osStatus = "errSecInteractionNotAllowed (-25308)";
  assert.doesNotMatch(
    keyStateSentence({ kind: "unreadable", cause: "locked", reason: osStatus }),
    /-25308/,
  );
  assert.match(
    keyStateSentence({ kind: "unreadable", cause: "refused", reason: osStatus }),
    /-25308/,
    "`refused` is the one value that names no action, which is what `reason` is for",
  );
});

// I1, the fourteenth "two facts, one message", and the only one that reached the
// window's own text. `set_key` refuses an empty submission before it calls
// anyone and checks the key with the provider before storing it, so every
// reachable failure but one decided nothing about the key: `ProviderUnreachable`
// ("nothing was refused and nothing was decided", by `error.rs`'s own words),
// `Secrets` (the provider accepted it; the store would not keep it), and
// `EmptyKey` (nothing was typed, so nothing was asked). A locked keychain is an
// ordinary state of the machine, and "the key was not accepted" sends its owner
// for a new one; the empty box is the same sentence about a person who has not
// started yet, and it is the one the first real run put on the screen.
test("a key that was not saved is never reported as a key the provider refused", () => {
  const failures = [
    "the provider refused the key",
    "the provider could not be reached: dns error",
    "the credential store: the keychain is locked",
    "an empty key was submitted, so nothing was sent to the provider and nothing was checked",
  ];
  for (const error of failures) {
    const text = keyNotSavedSentence(error);
    // The leading clause on its own — the error's own `Display` string follows
    // the colon and is allowed to say whatever actually happened.
    const clause = text.split(":")[0];
    assert.doesNotMatch(clause, /accepted|rejected|refused/i,
      `"${error}" had a decision about the key attributed to it that nobody made`);
    assert.match(clause, /not saved/,
      "the one thing true of every failure is what the clause may state");
    assert.ok(text.endsWith(error), "the provider's own words carry the fact and must survive");
  }
});

// `UnreadableCause::NotOpen` is one value over two situations — `AppState::db`
// is `None` both before the first `open_index` and after one that failed — and
// the window is the only place that can tell them apart, because it made the
// call and read the answer. A window that skips the correlation makes a
// permanent wall (an index written by a newer Mnema, which never opens) look
// like an ordinary cold start.
test("an index that failed to open is a different sentence from one not asked for yet", () => {
  const index = { kind: "unreadable", cause: "notOpen", reason: "the index is not open" };
  const wall = indexStateSentence(index, {
    kind: "failed",
    error: "this index was written by a newer Mnema (schema v9, this build reads v7)",
  });
  const coldStart = indexStateSentence(index, { kind: "notAsked" });
  assert.notEqual(wall, coldStart);
  assert.match(wall, /schema v9/, "the reason the wall is permanent is the only thing worth saying");
  assert.doesNotMatch(coldStart, /schema v9/);
});

// The third correlation: `open_index` answered `Ok`, so `AppState::db` was set
// (`state.rs`, `open_index` assigns before returning) — a later `notOpen` is
// then a defect of this build and not a folder anybody has to go and choose.
test("an index that opened and then reported itself closed is a bug report", () => {
  const text = indexStateSentence(
    { kind: "unreadable", cause: "notOpen", reason: "the index is not open" },
    { kind: "opened" },
  );
  assert.match(text, /defect/);
  assert.notEqual(text, indexStateSentence(
    { kind: "unreadable", cause: "notOpen", reason: "the index is not open" },
    { kind: "notAsked" },
  ));
});

test("a read that failed is a bug report and never an index nobody opened", () => {
  const text = indexStateSentence(
    { kind: "unreadable", cause: "readFailed", reason: "index: no space with id 7" },
    { kind: "opened" },
  );
  assert.match(text, /defect/);
  assert.match(text, /no space with id 7/, "the diagnostic is the whole value of a bug report");
});

// The entrance to the harm `NoSuchSpace` was written to prevent: an index that
// could not be read, drawn as an index with nothing configured in it.
test("an unreadable index is never drawn as an index with nothing configured", () => {
  for (const cause of UNREADABLE_CAUSES) {
    const index = { kind: "unreadable", cause, reason: "synthetic" };
    const progress = embeddingProgressText(index);
    assert.doesNotMatch(progress, /no embedding model/, `"${cause}" read as "no model chosen"`);
    assert.match(progress, /unknown/, `"${cause}" said nothing about what is not known`);
  }
  assert.match(
    embeddingProgressText({ kind: "read", activeSpace: null, embeddedChunks: 0, totalChunks: 0 }),
    /no embedding model/,
    "the one state that really is 'nothing chosen' must still say so",
  );
});

test("an index kind this build does not know is not read as a readable one", () => {
  const progress = embeddingProgressText({ kind: "somethingFutureAndUnknown" });
  assert.doesNotMatch(progress, /no embedding model/);
  assert.match(progress, /unknown/);
});

test("an active space with nothing embedded says so", () => {
  const text = embeddingProgressText({
    kind: "read",
    activeSpace: 1,
    embeddedChunks: 0,
    totalChunks: 812,
  });
  assert.match(text, /0 of 812/);
});

test("no active space is a different sentence from an empty one", () => {
  const none = embeddingProgressText({
    kind: "read",
    activeSpace: null,
    embeddedChunks: 0,
    totalChunks: 812,
  });
  assert.notEqual(
    none,
    embeddingProgressText({ kind: "read", activeSpace: 1, embeddedChunks: 0, totalChunks: 812 }),
  );
});

// Not a fraction, and never to be divided (`IndexRead::embedded_chunks`). The
// numerator counts one space and the denominator the whole index, so a vector
// that outlives the chunk it embeds — the storage half of which is in the gate
// as `a_vector_outlives_the_chunk_it_embeds` — puts the numerator above the
// denominator legitimately. A percentage of that is above 100, and clamping it
// would be this window inventing a number nobody measured.
test("the two counts are never divided, at any ratio", () => {
  for (const [embedded, total] of [[0, 0], [0, 812], [406, 812], [812, 812], [900, 812]]) {
    const text = embeddingProgressText({
      kind: "read",
      activeSpace: 1,
      embeddedChunks: embedded,
      totalChunks: total,
    });
    assert.doesNotMatch(text, /%/, `${embedded}/${total} was rendered as a percentage`);
    assert.match(text, new RegExp(`\\b${embedded}\\b`), `the numerator ${embedded} went missing`);
    assert.match(text, new RegExp(`\\b${total}\\b`), `the denominator ${total} went missing`);
  }
});

test("a numerator above the denominator is explained rather than left looking broken", () => {
  const over = embeddingProgressText({
    kind: "read",
    activeSpace: 1,
    embeddedChunks: 900,
    totalChunks: 812,
  });
  assert.match(over, /not an error/);
  // Both directions: an explanation printed on every reading would say nothing.
  const under = embeddingProgressText({
    kind: "read",
    activeSpace: 1,
    embeddedChunks: 406,
    totalChunks: 812,
  });
  assert.doesNotMatch(under, /not an error/);
});

// `set_embedding_model` answers `AdoptedModel`, whose `model`, `dim`, `spaceId`
// and `created` sit outside `index` precisely so a read-back that failed on its
// own cannot take them. The sentence after choosing a model must therefore not
// say the model was not chosen when only the read-back failed.
test("a model that was recorded still says so when the read-back failed", () => {
  const text = adoptedModelSentence(
    {
      model: "vendor/embed-3",
      dim: 1536,
      spaceId: 4,
      created: true,
      index: { kind: "unreadable", cause: "readFailed", reason: "index: database is locked" },
    },
    { kind: "opened" },
  );
  assert.match(text, /vendor\/embed-3/);
  assert.match(text, /1536/);
  assert.match(text, /recorded/i, "the model was written; only reading it back failed");
  assert.doesNotMatch(text, /not recorded|not chosen/i);
});

// `created` is stated by the index, never inferred. Deriving it from
// `embeddedChunks` would be wrong in exactly one direction: that number is
// identically zero in this build (D29), so every adoption would read as new.
test("whether a space was created is taken from the field that states it", () => {
  const adopted = {
    model: "vendor/embed-3",
    dim: 1536,
    spaceId: 4,
    index: { kind: "read", activeSpace: 4, embeddedChunks: 0, totalChunks: 0, embeddingDim: 1536 },
  };
  const fresh = adoptedModelSentence({ ...adopted, created: true }, { kind: "opened" });
  const reused = adoptedModelSentence({ ...adopted, created: false }, { kind: "opened" });
  assert.match(fresh, /new vector space/i);
  assert.doesNotMatch(reused, /new vector space/i);
  assert.notEqual(fresh, reused);
});

// Minor 1 — the direction the first round left unpinned. Only the read-back
// *failed* case was asserted, and making the tail unconditional passed all 44
// tests: the `created` test's single `doesNotMatch` looks for "new vector
// space", which the tail does not contain, so nothing anywhere objected to a
// successful read-back being told it had failed. This is the ninth-plus time a
// one-sided assertion has been satisfied by the wrong thing on this project.
test("a read-back that succeeded is never told it failed", () => {
  const text = adoptedModelSentence(
    {
      model: "vendor/embed-3",
      dim: 1536,
      spaceId: 4,
      created: true,
      index: { kind: "read", activeSpace: 4, embeddingDim: 1536, embeddedChunks: 0, totalChunks: 0 },
    },
    { kind: "opened" },
  );
  assert.doesNotMatch(text, /could not be read back/);
  assert.match(text, /vendor\/embed-3/, "and the adoption itself is still stated");
});

// A model entry as the wire carries it, with the two fields a test does not
// care about filled in. Named states, never bare numbers: `pricePerToken: null`
// and `contextLength: null` were the shapes the acceptance run found too narrow,
// and a fixture written in the old shape would silently exercise a fallback.
const entryWith = (fields) => ({
  id: "vendor/x",
  price: { kind: "notStated" },
  inputLimit: { kind: "known", tokens: 8192 },
  refusal: null,
  ...fields,
});

test("a refused model says both numbers", () => {
  const label = modelOptionLabel(
    entryWith({
      id: "thenlper/gte-base",
      price: { kind: "known", amount: 0.000000005 },
      inputLimit: { kind: "known", tokens: 512 },
      refusal: { kind: "inputTooSmall", limit: 512, floor: 2048 },
    }),
  );
  assert.match(label, /512/);
  assert.match(label, /2048/);
});

// The two pairs `Refusal` was split into over three review rounds, and the two
// the brief's own table folded back together under one default. "Did not say"
// and "said, and text was not among it" are opposite statements about the
// provider; so are "stated no limit" and "stated one this build cannot read".
test("a refusal never states about the provider something the provider did not say", () => {
  const notSaid = modelOptionLabel(
    entryWith({ refusal: { kind: "noStatedOutputModalities" } }),
  );
  assert.doesNotMatch(notSaid, /does not write text/);
  assert.match(notSaid, /did not say/);

  const said = modelOptionLabel(entryWith({ id: "vendor/y", refusal: { kind: "noTextOutput" } }));
  assert.match(said, /does not write text/);

  const unread = modelOptionLabel(
    entryWith({
      id: "vendor/z",
      inputLimit: { kind: "notUnderstood", raw: "8192.0" },
      refusal: { kind: "limitNotUnderstood", raw: "8192.0" },
    }),
  );
  assert.doesNotMatch(unread, /did not state/);
  assert.match(unread, /8192\.0/, "the value the provider actually sent is what a bug report needs");

  const absent = modelOptionLabel(
    entryWith({
      id: "vendor/w",
      inputLimit: { kind: "notStated" },
      refusal: { kind: "noStatedLimit" },
    }),
  );
  assert.match(absent, /did not state an input limit/);
});

test("a refusal this build does not know is still shown as a refusal", () => {
  const label = modelOptionLabel(entryWith({ refusal: { kind: "somethingFutureAndUnknown" } }));
  assert.match(label, /unavailable/);
  assert.doesNotMatch(label, /undefined/);
});

// The fixture id carries `:free` on purpose. The oracle here used to be
// `/free/`, which is a substring of a whole family of real OpenRouter ids — it
// was green only because the fixture was called `vendor/x`, and a realistic id
// would have reddened it on *correct* code. An oracle must not share a
// substring with the data it checks; this one aims at the rendering instead.
test("a price the provider did not state is unknown, not free", () => {
  // The fixture id carries `:free` on purpose. The oracle here used to be
  // `/free/`, which is a substring of a whole family of real OpenRouter ids —
  // it was green only because the fixture was called `vendor/x`, and a
  // realistic id would have reddened it on *correct* code. An oracle must not
  // share a substring with the data it checks; this one aims at the rendering.
  const unstated = modelOptionLabel(
    entryWith({ id: "vendor/x:free", price: { kind: "notStated" } }),
  );
  assert.match(unstated, /price unknown/);
  assert.doesNotMatch(unstated, /\$/,
    "an unstated price must not render as any amount at all, free included");
});

// The defect a person met on the first run, and the reason the old version of
// this assertion is gone rather than extended: it required `$0.000` of a stated
// zero, on the rule `Balance::Known { amount: 0 }` follows. The rule is right
// about the *number* and the sentence built on it was not. All six rerank
// models state `"prompt": "0"` — pinned in the provider crate by
// `every_rerank_model_the_provider_lists_states_a_price_of_zero` — and they are
// billed per search, so "$0.000 per million tokens" told their users they would
// not be charged. The payload states no per-search price, so the honest
// sentence says what was stated and refuses the conclusion.
test("a stated zero is a price the provider stated, and never a promise that the model is free", () => {
  const zero = modelOptionLabel(entryWith({ price: { kind: "known", amount: 0 } }));
  assert.match(zero, /not the same as free/);
  assert.doesNotMatch(zero, /\$0\.000/,
    "a bare $0.000 per million tokens is exactly the sentence that reads as a promise");

  // The half the first wording failed, and no assertion here could see: "no
  // charge per token stated" reads as *no price was stated*, which is the one
  // neighbour this sentence exists to be told apart from. So the sentence must
  // name the amount, and the neighbour must not — a state split in the type and
  // merged again in the words is the same defect one layer down.
  assert.match(zero, /\$0\b/, "a stated zero must name the number the provider stated");
  const unstated = modelOptionLabel(entryWith({ price: { kind: "notStated" } }));
  assert.doesNotMatch(unstated, /\$/, "and an absence must name none");

  // Both directions: a table that answered the caveat to every price would
  // satisfy the lines above and say nothing.
  const real = modelOptionLabel(entryWith({ price: { kind: "known", amount: 0.000000015 } }));
  assert.match(real, /\$0\.015 per million tokens/);
  assert.doesNotMatch(real, /not the same as free/);
});

// The label is assembled as `id — price, limit — refusal`, so an em-dash is a
// field boundary and a sentence that carries one of its own destroys the
// boundaries. The first wording of the stated zero did exactly that and put
// three dashes in one line; nothing asserted it, because every state still read
// differently as a string and identically to a person.
test("the label's em-dashes are its field boundaries and nothing else", () => {
  const prices = [
    { kind: "known", amount: 0 },
    { kind: "known", amount: 0.000000015 },
    { kind: "known", amount: 1e-12 },
    { kind: "notStated" },
    { kind: "notAPrice", raw: "-1" },
    { kind: "unreadable", raw: "free" },
    { kind: "somethingFutureAndUnknown" },
  ];
  const limits = [
    { kind: "known", tokens: 8192 },
    { kind: "notStated" },
    { kind: "notUnderstood", raw: "8k" },
    { kind: "somethingFutureAndUnknown" },
  ];
  const refusals = [
    [null, 1],
    [{ kind: "noStatedLimit" }, 2],
    [{ kind: "limitNotUnderstood", raw: "8k" }, 2],
    [{ kind: "inputTooSmall", limit: 512, floor: 2048 }, 2],
    [{ kind: "somethingFutureAndUnknown" }, 2],
  ];
  for (const price of prices) {
    for (const inputLimit of limits) {
      for (const [refusal, expected] of refusals) {
        const label = modelOptionLabel(entryWith({ price, inputLimit, refusal }));
        assert.equal((label.match(/—/g) ?? []).length, expected,
          `an em-dash that is not a field boundary: ${label}`);
      }
    }
  }
});

// A positive price small enough that `toFixed(3)` of a million tokens would
// print `$0.000` — a number this window made up about a provider that stated
// something else. Not a collision with the stated zero: no state of `Price`
// renders as `$0.000` any more, which is why the comment on the branch says so
// and this test does not.
test("a price too small to show at three decimals is not rendered as a zero", () => {
  const tiny = modelOptionLabel(entryWith({ price: { kind: "known", amount: 1e-12 } }));
  assert.match(tiny, /under \$0\.001 per million tokens/);
  assert.doesNotMatch(tiny, /not the same as free/,
    "it is not a stated zero: the provider stated a price, and it is small");
  assert.doesNotMatch(tiny, /\$0\.000/);

  // The boundary in the other direction: a price that does reach three decimals
  // is shown as itself, so the branch above cannot swallow everything cheap.
  const shown = modelOptionLabel(entryWith({ price: { kind: "known", amount: 1e-9 } }));
  assert.match(shown, /\$0\.001 per million tokens/);
  assert.doesNotMatch(shown, /under/);
});

// Provider text this build could not parse is shown quoted, wherever it is
// shown. `"free"` is the measured one: unquoted it is the last word of the
// label and reads as this window's own verdict on the price. The refusal clause
// carries the same value as the limit clause in one line, so a label that
// quoted one and not the other would show one value as two.
test("a value this build could not read is shown as the provider's text, not as a word of ours", () => {
  const price = modelOptionLabel(
    entryWith({ price: { kind: "unreadable", raw: "free" }, refusal: null }),
  );
  assert.match(price, /\("free"\)/);
  assert.doesNotMatch(price, /read \(free\)/, "unquoted, the label's last word is “free”");

  const both = modelOptionLabel(
    entryWith({
      inputLimit: { kind: "notUnderstood", raw: "unlimited" },
      refusal: { kind: "limitNotUnderstood", raw: "unlimited" },
    }),
  );
  assert.equal((both.match(/"unlimited"/g) ?? []).length, 2,
    `one unparsed value must be spelled one way in one label: ${both}`);
  assert.doesNotMatch(both, /\(unlimited\)/);

  // The sharpest of the arms that carry `raw`, and the reason its fixture lives
  // here rather than in "every price state reads differently, and none of them
  // is a number this build invented": `NaN` is a value the provider can state
  // (`a_price_that_is_not_a_finite_number_is_not_a_price` pins it one crate
  // over) **and** the word that test searches a label for when it asks whether
  // this window invented a number. Unquoted, one word means both things in one
  // line, and only the choice of `-1` as a fixture there kept it invisible.
  const notANumber = modelOptionLabel(
    entryWith({ price: { kind: "notAPrice", raw: "NaN" }, refusal: null }),
  );
  assert.match(notANumber, /stated "NaN" per token/);
  assert.doesNotMatch(notANumber, /stated NaN per token/,
    "the provider's word and this window's marker for a defect must not be the same token");
});

// The sentinel that was on the screen: `-1`, which the provider sends for a
// model priced at routing time, went through `× 1e6` and printed
// `$-1000000.000 per million tokens`. Nothing anywhere rejected a negative.
test("a number that cannot be a price is never rendered as one", () => {
  const sentinel = modelOptionLabel(entryWith({ price: { kind: "notAPrice", raw: "-1" } }));
  assert.doesNotMatch(sentinel, /\$/, "no amount may be shown for a number that is not an amount");
  assert.doesNotMatch(sentinel, /-1000000/);
  assert.match(sentinel, /-1/, "what the provider actually sent is what a bug report needs");
  assert.doesNotMatch(sentinel, /undefined|NaN/);
});

// Every state of the price reads as its own fact. A key-set check is satisfied
// by four arms saying the same thing, and "the provider said nothing" against
// "the provider said something this build cannot read" is the pair `Option`
// could not hold — the same fold N1 fixed one field over, for the input limit.
// The fact that used to die on the wire for rerank and chat. The refusals that
// carry it are the embedding role's, so for the other two roles "the provider
// stated no input limit" and "the provider stated one this build cannot read"
// both arrived as `contextLength: null, refusal: null` and drew `input ?` (I4).
// The Rust half is
// `a_limit_stated_unreadably_is_told_apart_from_no_limit_for_every_role`.
test("nothing stated about the input limit reads differently from something unreadable", () => {
  const silent = modelOptionLabel(entryWith({ inputLimit: { kind: "notStated" } }));
  const unreadable = modelOptionLabel(
    entryWith({ inputLimit: { kind: "notUnderstood", raw: "8k" } }),
  );
  const known = modelOptionLabel(entryWith({ inputLimit: { kind: "known", tokens: 8194 } }));
  const unknownState = modelOptionLabel(
    entryWith({ inputLimit: { kind: "somethingFutureAndUnknown" } }),
  );

  const said = [silent, unreadable, known, unknownState];
  assert.equal(new Set(said).size, said.length, `two input limit states read alike: ${said}`);
  for (const text of said) {
    assert.doesNotMatch(text, /undefined|NaN|\[object Object\]/);
  }
  assert.match(unreadable, /8k/, "the value the provider actually sent is what a bug report needs");
  assert.doesNotMatch(silent, /8k/);
  assert.match(known, /8194/);
  // The one thing neither of the two unknowns may be drawn as: a number.
  assert.doesNotMatch(silent, /\d/);
  assert.doesNotMatch(unknownState, /\d/);
});

test("every price state reads differently, and none of them is a number this build invented", () => {
  const said = [
    { kind: "known", amount: 0.000000015 },
    { kind: "known", amount: 0 },
    { kind: "notStated" },
    { kind: "notAPrice", raw: "-1" },
    { kind: "unreadable", raw: "free" },
    { kind: "somethingFutureAndUnknown" },
  ].map((price) => modelOptionLabel(entryWith({ price })));

  assert.equal(new Set(said).size, said.length, `two price states read alike: ${said}`);
  for (const text of said) {
    assert.doesNotMatch(text, /undefined|NaN|\[object Object\]/);
  }
});

test("no state of the balance is ever rendered as a zero", () => {
  for (const balance of [
    { kind: "notStated" },
    // The wire shape is `raw: ProviderMessage`, a tagged object — a bare string
    // here is a canary, not a fixture: any implementation that interpolated
    // `raw` would put "$10.00" in front of a person and redden the assertion
    // below. The real shape is exercised in the test after this one.
    { kind: "unreadable", raw: "$10.00" },
    { kind: "envelopeNotUnderstood" },
  ]) {
    const text = keyAcceptedSentence({ balance });
    assert.doesNotMatch(text, /0[.,]00|\b0\b/,
      `"${balance.kind}" is a thing we do not know, and printing it as a number sends a funded user to pay again`);
  }
  assert.match(
    keyAcceptedSentence({ balance: { kind: "known", amount: 0 } }),
    /0[.,]00/,
    "a real zero the provider sent must still be shown — it is the one state that is a number",
  );
});

test("every balance state reads differently, and none of them leaks the envelope", () => {
  const said = [
    keyAcceptedSentence({ balance: { kind: "known", amount: 6.5 } }),
    keyAcceptedSentence({ balance: { kind: "notStated" } }),
    keyAcceptedSentence({
      balance: { kind: "unreadable", raw: { kind: "text", text: "total_credits: not a number" } },
    }),
    keyAcceptedSentence({ balance: { kind: "envelopeNotUnderstood" } }),
    keyAcceptedSentence({ balance: { kind: "somethingFutureAndUnknown" } }),
  ];
  assert.equal(new Set(said).size, said.length, `two balance states read alike: ${said}`);
  for (const text of said) {
    assert.doesNotMatch(text, /\[object Object\]/);
    assert.doesNotMatch(text, /undefined|NaN/);
  }
  assert.match(said[0], /6[.,]50/);
});

test("an unreadable record with no readable id is still named by its position", () => {
  const text = unreadableSentence({
    unreadable: 2,
    unreadableRecords: [
      { index: 7, id: { kind: "notAString" } },
      { index: 9, id: { kind: "absent" } },
    ],
  });
  assert.match(text, /7/);
  assert.match(text, /9/);
  assert.doesNotMatch(text, /\[object Object\]/,
    "a tagged id read as a string is how an upstream distinction dies at the last seam");
});

// `RecordId::NotAString` carries the value the provider sent. The position must
// come from `index` and not accidentally from that value, which is what the
// fixture above could not tell apart — both records there carry no `raw` at all.
test("a record's position is its own, not a number that happened to be in its id", () => {
  const text = unreadableSentence({
    unreadable: 1,
    unreadableRecords: [{ index: 4, id: { kind: "notAString", raw: "12345" } }],
  });
  assert.match(text, /\b4\b/);
  assert.match(text, /12345/);
});

test("a readable id names the model, and no unreadable records say nothing at all", () => {
  assert.match(
    unreadableSentence({
      unreadable: 1,
      unreadableRecords: [{ index: 0, id: { kind: "known", id: "vendor/broken-pricing" } }],
    }),
    /vendor\/broken-pricing/,
  );
  assert.equal(unreadableSentence({ unreadable: 0, unreadableRecords: [] }), "");
});

// A count with no records behind it is still a count, and dropping it would
// leave a list quietly shorter than the provider's — the defect Task 1 spent
// three fix rounds removing one layer down.
test("records the window was given no detail about are still counted", () => {
  assert.match(unreadableSentence({ unreadable: 3 }), /\b3\b/);
});

// A blank picker is reached by four different facts, and only one of them is
// "nothing is recorded". The pair worth the original fix: a list that could not
// be read says nothing whatever about the provider, and reporting it as a model
// the provider withdrew invents a fact — while saying nothing at all loses the
// configuration the index actually holds.
// A catalogue in which every record was read — the only evidence that actually
// establishes "the provider no longer lists it".
const cleanCatalogue = { entries: [{ id: "vendor/other" }], unreadable: 0, unreadableRecords: [] };

test("a recorded model missing from the picker is never lost, and never blamed on the provider", () => {
  assert.equal(
    recordedNoteSentence({ recorded: null, list: listWasRead(cleanCatalogue), listed: false }),
    "",
    "nothing is recorded — the blank picker is the truth and a sentence would be noise",
  );
  assert.equal(
    recordedNoteSentence({ recorded: "vendor/m", list: listWasRead(cleanCatalogue), listed: true }),
    "",
  );

  const withdrawn = recordedNoteSentence({
    recorded: "vendor/m",
    list: listWasRead(cleanCatalogue),
    listed: false,
  });
  assert.match(withdrawn, /vendor\/m/);
  assert.match(withdrawn, /no longer lists/);

  const unread = recordedNoteSentence({ recorded: "vendor/m", list: listFailed(), listed: false });
  assert.match(unread, /vendor\/m/, "the recorded model must not vanish with the list");
  assert.doesNotMatch(unread, /no longer lists/,
    "a list this window could not read is not evidence the provider withdrew anything");
  assert.notEqual(unread, withdrawn);
});

// The fifteenth "two facts, one message", and the sharpest so far because the
// false half is a claim about somebody else. Reachable by construction, not by
// argument: `models_from_json` reads a record's `id` off the raw value before
// the decode that failed (`catalogue.rs:556-562`), so `RecordId::Known { id }`
// names a model the provider **does** list and this build could not read. Its
// id never reaches `entries`, so the picker has no option for it — and the
// window used to print, under one picker, one line above the other:
//
//   records in the provider's list this build could not read: 1 (vendor/m)
//   The index records “vendor/m”, but the provider no longer lists this model.
test("a model the provider does list is never reported as one it withdrew", () => {
  const catalogue = {
    entries: [{ id: "vendor/other" }],
    unreadable: 1,
    unreadableRecords: [{ index: 7, id: { kind: "known", id: "vendor/m" } }],
  };
  const text = recordedNoteSentence({
    recorded: "vendor/m",
    list: listWasRead(catalogue),
    listed: false,
  });
  assert.doesNotMatch(text, /no longer lists/,
    "the provider named this model in the very same answer; the record is what could not be read");
  assert.match(text, /does list it/);
  assert.match(text, /vendor\/m/);

  // Both ways, or the fix is satisfied by silence: a catalogue this build read
  // whole must still say the model was withdrawn.
  assert.match(
    recordedNoteSentence({
      recorded: "vendor/m",
      list: listWasRead(cleanCatalogue),
      listed: false,
    }),
    /no longer lists/,
    "with every record accounted for, 'the provider no longer lists it' is established",
  );
});

// The third arm, and the reason `unreadable > 0` is the wrong discriminant. A
// record that could not even be named — `RecordId::Absent` or `NotAString` —
// could be this model or could not, and neither of the other two sentences is
// established. The same holds for a count with no records behind it.
test("an unreadable record this build could not even name leaves the question open", () => {
  for (const [label, catalogue] of [
    ["a record with no id at all", {
      entries: [{ id: "vendor/other" }],
      unreadable: 1,
      unreadableRecords: [{ index: 3, id: { kind: "absent" } }],
    }],
    ["a count with no records behind it", {
      entries: [{ id: "vendor/other" }],
      unreadable: 2,
      unreadableRecords: [],
    }],
  ]) {
    const text = recordedNoteSentence({
      recorded: "vendor/m",
      list: listWasRead(catalogue),
      listed: false,
    });
    assert.doesNotMatch(text, /no longer lists/, `${label}: claimed a withdrawal nobody established`);
    assert.doesNotMatch(text, /does list it/, `${label}: claimed a listing nobody established`);
    assert.match(text, /unknown/, `${label}: said nothing about what is not known`);
  }
});

test("the three reasons a recorded model can be missing are told apart", () => {
  const withKnownId = {
    unreadable: 1,
    unreadableRecords: [{ index: 0, id: { kind: "known", id: "vendor/m" } }],
  };
  assert.equal(missingModelReason("vendor/m", withKnownId), "unreadableRecord");
  assert.equal(missingModelReason("vendor/other", withKnownId), "withdrawn",
    "every record is named, and this one is not among them");
  assert.equal(missingModelReason("vendor/m", cleanCatalogue), "withdrawn");
  assert.equal(
    missingModelReason("vendor/m", { unreadable: 1, unreadableRecords: [{ index: 0, id: { kind: "absent" } }] }),
    "unknown",
  );
});

// I2 — the fourth fact, and the one that was a `false` sharing a value with
// "could not be read". The listeners are registered above the three
// `provider_models` round trips, so a key submitted while they are in flight
// draws this state; for one round it drew a failure that had not happened.
test("a list that has not been asked for yet is not a list that could not be read", () => {
  const loading = recordedNoteSentence({
    recorded: "vendor/m",
    list: listNotAsked(),
    listed: false,
  });
  assert.equal(loading, "", "while the list is loading there is nothing yet to say about it");
  assert.notEqual(
    loading,
    recordedNoteSentence({ recorded: "vendor/m", list: listFailed(), listed: false }),
    "'not asked yet' and 'could not be read' are two facts and were one boolean",
  );
});

test("a list state this build does not know does not claim the provider withdrew anything", () => {
  const unknown = recordedNoteSentence({
    recorded: "vendor/m",
    list: { kind: "somethingFutureAndUnknown" },
    listed: false,
  });
  assert.match(unknown, /vendor\/m/);
  assert.doesNotMatch(unknown, /no longer lists/);
});

// I3 — the decision `provider_models` explicitly delegated to whoever renders
// this: `models.rs:135-145` keeps both numbers on the wire precisely so the
// window can tell "the provider has none" from "something upstream ate them",
// and rendering neither leaves a picker with no options and no explanation.
test("a well-formed empty catalogue says so instead of drawing an empty picker", () => {
  const empty = catalogueSentence({ entries: [], unreadable: 0, unreadableRecords: [] });
  assert.match(empty, /lists no models/);

  // The other zero: nothing selectable, because everything in the list was
  // unreadable. A different fact, and the one that is a bug report.
  const eaten = catalogueSentence({
    entries: [],
    unreadable: 2,
    unreadableRecords: [{ index: 0, id: { kind: "absent" } }],
  });
  assert.notEqual(eaten, empty);
  assert.match(eaten, /\b2\b/, "the count is what tells the two zeroes apart");

  // Both directions: a catalogue with entries must not start announcing that
  // the provider lists nothing.
  const full = catalogueSentence({
    entries: [{ id: "vendor/m" }],
    unreadable: 0,
    unreadableRecords: [],
  });
  assert.equal(full, "");
  assert.doesNotMatch(catalogueSentence({
    entries: [{ id: "vendor/m" }],
    unreadable: 1,
    unreadableRecords: [{ index: 3, id: { kind: "absent" } }],
  }), /lists no models/);
});

// The one throwing path `fillRole` had outside its own `try`, and the trigger
// behind Minor 3: `el(id)` answers `null` for an id `main.js` and `index.html`
// disagree about, and the `.replaceChildren()` that follows takes down the
// whole settings draw. Reordering and `allSettled` stop that from blanking
// `#disclosure`; this stops the disagreement existing. It is the closest this
// suite gets to the DOM — text against text, no browser — and it is the only
// check in the file that would notice a `<p>` deleted from the HTML.
test("every element main.js reaches for exists in index.html", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const html = readFileSync(join(here, "index.html"), "utf8");
  const main = readFileSync(join(here, "main.js"), "utf8");
  const ids = new Set(
    [...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]),
  );

  const literals = [...main.matchAll(/\bel\("([^"]+)"\)/g)].map((m) => m[1]);
  // A floor, because a loop over an empty list passes and a regexp that has
  // stopped matching produces exactly that. **52 today, re-measured**: this
  // said "`> 10` against an actual 20" while the actual had been 48 since
  // `667d2ff`, so it tolerated losing thirty-two calls in silence — a
  // number describing a file it had stopped describing, in a test whose whole
  // job is to notice that (whole-branch review, closing check). The margin is
  // what a legitimate edit may remove before this asks to be looked at, and
  // the same rule and the same shape of number are on the block test below.
  assert.ok(literals.length >= 44,
    `only ${literals.length} literal ids found — the regexp has rotted, or main.js shrank`);
  for (const id of literals) {
    assert.ok(ids.has(id), `main.js reaches for #${id}, which index.html does not have`);
  }

  // The derived ones, which the regexp above cannot see. `selectId` is
  // **imported**, not restated: written out here a second time, this loop
  // checked the markup against the test's own copy of the rule, and changing
  // the derivation in `main.js` left all 51 tests green while every picker in
  // the window broke.
  for (const role of ROLES) {
    for (const id of [selectId(role), `${selectId(role)}-unreadable`, `${selectId(role)}-missing`]) {
      assert.ok(ids.has(id), `the ${role} role needs #${id}, which index.html does not have`);
    }
  }
});

test("each role is named in the sentence that says its model was recorded", () => {
  const said = ROLES.map((role) => roleRecordedSentence(role, "vendor/m"));
  assert.equal(new Set(said).size, ROLES.length, `two roles read alike: ${said}`);
  for (const text of said) {
    assert.match(text, /vendor\/m/);
    assert.doesNotMatch(text, /undefined/);
  }
});

// I1 — the button that reported an event it had not caused. `forget_key`
// answered `Ok(())` whether or not there was a key, and this window wrote "the
// key was removed" unconditionally, so somebody who had entered none, or whose
// key a second window had already taken, was told this application removed one.
// The Rust half is `a_deletion_that_removed_a_key_is_told_apart_from_one_that_
// found_none` and `forgetting_says_whether_there_was_anything_to_forget`.
test("a removal that found no key never says one was removed", () => {
  const removed = keyRemovedSentence({ kind: "removed" });
  const none = keyRemovedSentence({ kind: "nothingToRemove" });
  const unknown = keyRemovedSentence({ kind: "somethingFutureAndUnknown" });

  assert.match(removed, /was removed/);
  assert.doesNotMatch(none, /was removed/,
    "nothing was removed, and a person reading this had entered no key");
  // Both directions and the third state: a table answering "there was no key"
  // to everything would satisfy the line above and lie the other way, and a
  // `kind` this build does not know must claim neither.
  const said = [removed, none, unknown];
  assert.equal(new Set(said).size, said.length, `two removal states read alike: ${said}`);
  assert.doesNotMatch(unknown, /was removed/);
  for (const text of said) {
    assert.doesNotMatch(text, /undefined|\[object Object\]/);
  }

  // The failure of the same press is not one of the arms above, and must not
  // read like either: the key is still exactly where it was.
  const refused = keyNotRemovedSentence("credential store: the keychain is locked");
  assert.doesNotMatch(refused, /was removed|no key to remove/);
  assert.match(refused, /could not be removed/);
  assert.ok(refused.endsWith("credential store: the keychain is locked"),
    "the store's own words carry the fact and must survive");
});

// The four sentences that moved out of `main.js` for I2, checked where they can
// now be reached at all. `roleNotRecordedSentence` is the one with a rule
// rather than an interpolation: it was written in `main.js` without the
// `?? role` fallback its own success sentence has, so a role this build does
// not know rendered "the undefined model was not recorded".
test("a failure sentence names what failed and carries the reason it was given", () => {
  for (const [what, text] of [
    ["the list", listNotReadSentence("dns error")],
    ["the embedding model", embeddingModelNotRecordedSentence("index: no such space 7")],
    ["a role", roleNotRecordedSentence("rerank", "dns error")],
  ]) {
    assert.doesNotMatch(text, /undefined|\[object Object\]/, `${what}: ${text}`);
    assert.match(text, /could not be read|not recorded/, `${what}: ${text}`);
  }

  const said = ROLES.map((role) => roleNotRecordedSentence(role, "dns error"));
  assert.equal(new Set(said).size, ROLES.length, `two roles read alike: ${said}`);
  assert.match(roleNotRecordedSentence("rerank", "dns error"), /reranking/);
  assert.match(roleNotRecordedSentence("somethingFutureAndUnknown", "dns error"),
    /somethingFutureAndUnknown/,
    "a role this build does not know is named, not rendered as undefined");
});

// The twenty-first, found by the owner on the second live run and introduced by
// the fix for the twentieth: this window clears the field after a successful
// save, so pressing the button with an empty field is the ordinary state of
// somebody whose key is stored and fine — and `Error::EmptyKey`'s sentence
// reads to them as a failure.
//
// The states are four, not two, and the one that decides the shape is
// `unreadable`: it is neither "a key is stored" nor "none is", and sending
// would hand it the same sentence `absent` gets, folding the two apart states
// this window keeps apart everywhere else.
test("an empty field is only the command's to answer when nothing is stored", () => {
  // The one that sends. `null` is the decision, not an empty sentence.
  assert.equal(emptyFieldSentence({ kind: "absent" }), null);

  const said = [
    ["present", emptyFieldSentence({ kind: "present" })],
    ["unreadable", emptyFieldSentence({ kind: "unreadable", cause: "locked", reason: "" })],
    ["notAsked", emptyFieldSentence(keyNotAsked())],
    ["a kind this build does not know", emptyFieldSentence({ kind: "somethingFutureAndUnknown" })],
  ];
  for (const [state, text] of said) {
    assert.notEqual(text, null, `${state} must not be sent: the command cannot tell it apart`);
    assert.doesNotMatch(text, /undefined|\[object Object\]/, `${state}: ${text}`);
  }
  assert.equal(new Set(said.map(([, text]) => text)).size, said.length,
    `two states of the key store read alike: ${said.map(([, t]) => t)}`);

  // A key IS stored: the sentence must say so, and must not read as a refusal.
  const stored = emptyFieldSentence({ kind: "present" });
  assert.match(stored, /already stored/);
  assert.doesNotMatch(stored, /nothing was sent|not saved|empty key was submitted/);

  // The store would not answer: neither claim, and no repetition of the remedy
  // — `keyStateSentence` is drawn beside this one and owns that.
  const silent = emptyFieldSentence({ kind: "unreadable", cause: "locked", reason: "" });
  assert.match(silent, /unknown/);
  assert.doesNotMatch(silent, /already stored;|there is no key/,
    "a store that would not answer establishes neither, and guessing either is the defect");
});

// The field's two labels. Three of the four states read alike here and that is
// deliberate: these say what the button does, not what the store holds.
// "Replace the key" is the one that claims a key is stored.
test("only a key this window has been told about turns the button into Replace", () => {
  assert.match(keySubmitText({ kind: "present" }), /Replace/);
  assert.match(keyFieldPlaceholder({ kind: "present" }), /leave empty/i);

  for (const key of [
    { kind: "absent" },
    { kind: "unreadable", cause: "locked", reason: "" },
    keyNotAsked(),
    { kind: "somethingFutureAndUnknown" },
    undefined,
  ]) {
    const label = keySubmitText(key);
    const placeholder = keyFieldPlaceholder(key);
    assert.doesNotMatch(label, /Replace/,
      `"${label}" claims a key is stored, and ${JSON.stringify(key)} does not establish one`);
    assert.doesNotMatch(placeholder, /leave empty/i,
      `"${placeholder}" tells somebody an empty field will keep a key that may not be there`);
    assert.doesNotMatch(label, /undefined/);
    assert.doesNotMatch(placeholder, /undefined/);
  }
});

// `main.js`'s own header says every sentence in its model configuration block
// comes from `render.js`. It was false for five of them (whole-branch review,
// I2), and the sentence furthest outside the tables was the one that told
// people a key had been removed when none had. So the claim is checked here
// rather than promised there: a sentence that never enters this file is a
// sentence no test in it can reach, whatever either comment says.
//
// It reads the source, like the test above it, and is scoped to the block —
// the walking skeleton's half above the marker makes no such claim and is
// deliberately not covered, because covering it silently would be this test
// asserting something nobody wrote down.
//
// **Say plainly what this covers and what it does not.** Measured on copies of
// `main.js`, one edit at a time (whole-branch review, closing check):
//
// | a literal reaching the DOM as | this test |
// | --- | --- |
// | `textContent = "..."`, and now `+=`, `innerText`, `innerHTML` | **red** |
// | a local `const` assigned to `textContent` | green |
// | a literal second in a concatenation | green |
// | a ternary whose first branch is not a literal | green |
//
// It goes red for the defect it was written for — the shape all five I2
// sentences actually had — and for the two spellings of it that cost one word
// each to add. It cannot see a literal bound to a name, because "anything that
// is not a quote is a call into `render.js`" is false for an identifier, and
// deciding otherwise needs a parser rather than a regexp. The claim in
// `main.js`'s own header is written to the same limit; an earlier version of
// both overclaimed.
//
// ⚠️ **Nothing in `scripts/mutations/` can reach this file.**
// `mutation-check.sh` runs `cargo test` and nothing else, so the whole of
// `ui/render.test.js` — this regexp included — is held by review and by hand.
// The case file states the same limit from its own side twice; this is the
// side somebody loosening the regexp is actually standing on.
test("every sentence in the model configuration block comes from render.js", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const main = readFileSync(join(here, "main.js"), "utf8");

  const marker = "// Model configuration.";
  const at = main.lastIndexOf(marker);
  assert.ok(at > 0, "the model configuration block's own header is gone, so this test aims at nothing");
  const block = main.slice(at);

  // The first character of what is assigned to text in the DOM. A literal opens
  // with a quote or a backtick. `+=` is here because `textContent = "a"` and
  // `textContent += "a"` put the same word on the same screen; the other
  // property names because a sentence does not become reachable by being
  // written to `innerText` instead, and `placeholder` because it is a sentence
  // a person reads for as long as the field is empty — which, with a key
  // stored, is all the time.
  //
  // `value` is deliberately **not** here. `el("key").value = ""` is not a
  // sentence, it is the field being cleared so a credential does not sit in the
  // DOM, and a rule that reddened on it would be read as a reason to stop.
  const assignments = [
    ...block.matchAll(/\.(?:textContent|innerText|innerHTML|placeholder)\s*\+?=\s*(\S)/g),
  ].map((m) => m[1]);
  // A floor, because an assertion over an empty list passes and a rotted regexp
  // produces exactly that. **19 today**, and the margin is what a legitimate
  // edit may remove before this asks to be looked at. The id test above carries
  // the same rule and its own re-measured number; this one said `>= 10` against
  // 16 and called itself tight "the same way the id test above is", which was
  // a parity claim pointing at a floor that had been stale for six commits.
  // Both numbers are re-measured whenever this file's regexps or main.js's
  // block change, which is the whole point of naming the actual beside them.
  assert.ok(assignments.length >= 17,
    `only ${assignments.length} textContent assignments found — the regexp has rotted, or the block shrank`);
  for (const first of assignments) {
    assert.ok(!["'", '"', "`"].includes(first),
      `a sentence in the model configuration block starts with ${first} — it is a literal in ` +
      "main.js, where render.test.js cannot reach it, and main.js's own header says there are none");
  }
});

// The acceptance run of 2026-08-11 found the longer placeholder cut to "leave
// empty to keep the sto" at the field's default width — the one sentence whose
// whole job is to say an empty field is fine here, unreadable exactly when it
// is shown. This reads main.js as text because a width has no rendering to
// measure in this suite, and that is also why every window test missed it: what
// broke was not a value but the space it was drawn in.
//
// It pins the shape rather than a number: the width comes from the text being
// shown, so a longer placeholder cannot be cut by a width chosen once. Both
// halves are asserted because either alone is satisfied by the other's absence
// — a literal width passes the first, and a placeholder assigned straight from
// the call passes the second with nothing left to derive a width from.
test("the key field is sized from the text it shows, not from a number", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const main = readFileSync(join(here, "main.js"), "utf8");
  assert.match(
    main,
    /placeholder = keyFieldPlaceholder\([^)]*\);\s+el\("key"\)\.placeholder = placeholder;/,
    "the placeholder is no longer held in a binding a width can be derived from",
  );
  assert.match(
    main,
    /el\("key"\)\.size = placeholder\.length/,
    "the field's width is not derived from the placeholder it is showing",
  );
});

// The status line is written from nine places and its text comes from two
// languages: sentences written in render.js, and `Error` renderings from Rust,
// which by that language's convention begin lower case and carry no full stop.
// Under a state line that is a proper sentence, the mixture read as unfinished —
// found by the owner in the acceptance run of 2026-08-11, and by nothing else.
//
// Both directions, because a shaper that only ever added a stop would pass a
// test that only checked one: it must also leave a text that is already a
// sentence alone, or "…was created." becomes "…was created..".
test("a status line is shaped into a sentence, and an already-shaped one is left alone", () => {
  assert.equal(asStatusSentence("the key was accepted"), "The key was accepted.");
  assert.equal(
    asStatusSentence("The embedding model was recorded: baai/bge-m3."),
    "The embedding model was recorded: baai/bge-m3.",
  );
  assert.equal(asStatusSentence(""), "", "an empty status must stay empty, not become a full stop");
  assert.equal(asStatusSentence(null), "", "nothing to say is not a sentence either");
});

// And the seam is only a seam if everything goes through it. Nine writes today;
// the floor is a floor rather than a total, so a tenth is not a failure and a
// regexp that has rotted to nothing is.
test("every write to the status line goes through the seam", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const main = readFileSync(join(here, "main.js"), "utf8");
  const writes = [...main.matchAll(/el\("(?:key|model)-status"\)\.textContent\s*=\s*([^\n]*)/g)].map(
    (m) => m[1],
  );
  assert.ok(
    writes.length >= 9,
    `only ${writes.length} status writes found — the regexp has rotted, or the handlers moved`,
  );
  for (const written of writes) {
    assert.match(
      written,
      /^asStatusSentence\(/,
      `a status line is written without the seam, so its shape depends on which producer ran: ${written}`,
    );
  }
});

// The note exists because an update makes macOS ask for the login keychain
// password on behalf of an application it cannot vouch for (measured
// 2026-08-11), and somebody meeting that with no warning should refuse it.
//
// Three assertions rather than one, and the last two are what stop this being a
// sentence that appears everywhere: it is for one platform, and for the one key
// state where there is something to be asked about later.
test("the note about a future request is drawn on the platform that will make it", () => {
  const present = { kind: "present" };
  assert.match(keyStoreNote("mac", present), /Always Allow/);
  for (const platform of ["windows", "linux"]) {
    assert.equal(
      keyStoreNote(platform, present),
      "",
      `${platform} was told about a mechanism it does not have`,
    );
  }
  for (const key of [{ kind: "absent" }, { kind: "unreadable", cause: "locked", reason: "" }]) {
    assert.equal(
      keyStoreNote("mac", key),
      "",
      `"${key.kind}" was warned about a key that is not stored`,
    );
  }
  assert.equal(keyStoreNote("plan9", present), "", "a platform this build does not know said something");
});
