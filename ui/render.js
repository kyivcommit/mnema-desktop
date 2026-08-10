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
// `rulesNotApplied` in particular is worded as a guarantee, not an apology.
// Nothing indexed today leaves the machine — there is no embedding call site
// yet — so the claim below is about reading, and it is true precisely because
// `walk_root` returns before phase 1 runs at all for this `StopReason`. It
// will mean more than that later: D29 ships v1 with no local models, so once
// embeddings exist, a walk that refuses to start because it could not apply
// its own exclusion rules is also refusing to send them anywhere.
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

// ─────────────────────────────────────────────────────────────────────────────
// Model configuration.
//
// Seven tagged unions reach this file from `src-tauri/src/models.rs` and
// `crates/mnema-provider` — `KeyState`, `IndexSettings`, `UnreadableCause`,
// `KeyStoreFailure`, `Refusal`, `Balance`, `RecordId` — and every one of them
// exists because somebody measured a place where folding two of its values
// together stated, to a person, a fact nobody had established. This is the last
// seam before that person: a distinction lost here is lost.
//
// So each is a **table**, not a `switch` with a `default`, and `render.test.js`
// asserts every table's key set is exactly the union's list of variants. A
// `default` arm is where two states quietly become one pixel; a missing key is
// a test failure. The four unions in `models.rs` also have a Rust-side pin
// (`every_discriminant_the_window_sees_has_its_camel_case_spelling_pinned`),
// whose own doc says the mirrored half belongs in `render.test.js` — this is
// the renderer that lets it exist.
//
// Every table still has a fallback for a `kind` this build has never seen, and
// every fallback is written to be *honest about not knowing* rather than to
// pick the friendlier of the two neighbours it sits between.
//
// The sentences are English, like the rest of this window. They were written in
// Ukrainian first, to the plan's instruction, and translated when the owner
// settled the question: the interface is English by default and translation
// arrives later as its own task, through a dictionary rather than as localised
// strings written inline. These are the source strings that task inherits, so
// they are phrased to survive it — see `unreadableSentence` for the one place
// that shaped the wording.

// The three roles a model can be chosen for, and the word each is called by.
// `models.rs::role_from` is the Rust half and is pinned there by
// `every_role_the_provider_has_is_named_by_a_string_the_window_can_send`; these
// strings are what that function is sent.
export const ROLES = ["embedding", "rerank", "chat"];

export const ROLE_NAME = {
  embedding: "embedding",
  rerank: "reranking",
  chat: "answering",
};

export const roleRecordedSentence = (role, model) =>
  `The ${ROLE_NAME[role] ?? role} model was recorded: ${model}.`;

// The three states `main.js` can be in about one role's model list, and the
// three it can be in about `open_index`. **Built here rather than written as
// string literals over there**: a named import that does not exist is a
// link-time error the moment the module loads — the same failure that opened
// this task — while a mistyped literal (`"opend"`) falls through a table's
// fallback and reddens nothing. `render.test.js` asserts each constructor's
// `kind` is a key of the table that renders it, so the two cannot drift either.
export const indexNotAsked = () => ({ kind: "notAsked" });
export const indexOpened = () => ({ kind: "opened" });
export const indexOpenFailed = (error) => ({ kind: "failed", error: `${error}` });

export const listNotAsked = () => ({ kind: "notAsked" });
export const listWasRead = () => ({ kind: "read" });
export const listFailed = () => ({ kind: "failed" });

// What to say under a picker that is not showing the model this index records.
//
// Assigning a `<select>` a value no option carries leaves it blank, and a blank
// picker is where a recorded configuration disappears without a word. Three
// states reach that same blank pixel and they are not the same fact:
//
// - nothing is recorded — the blank picker is the truth, and a sentence would
//   be noise;
// - a model is recorded and the provider's list no longer names it — a real
//   statement about the provider, and the model is still what the index uses;
// - a model is recorded and the provider's list could not be read at all —
//   which says **nothing** about the provider, and must not be reported as
//   though it did. This is the pair that made the fix worth making: an empty
//   picker after a network failure is not evidence a model was withdrawn.
//
// ⚠️ The list state is a **three-valued** union and was a boolean for one
// round, which is how the first version reported a failure that had not
// happened: `false` meant both "could not be read" and "has not been asked
// for yet", and the listeners are registered before the three `provider_models`
// round trips finish, so a key submitted in that window drew the failure
// sentence. It is the same shape this file already gives `UnreadableCause::
// NotOpen` two hundred lines down — fixed there, left boolean here.
export const LIST_STATE_NOTE = {
  // Nothing to say while it loads. Not "could not be read", which is a claim
  // about the machine that nothing has established yet.
  notAsked: () => "",
  read: ({ recorded, listed }) =>
    listed
      ? ""
      : `The index records “${recorded}”, but the provider no longer lists this model. ` +
        "It stays recorded; the picker above does not show it.",
  failed: ({ recorded }) =>
    `The index records “${recorded}”. The model list could not be read, so the picker above ` +
    "cannot show it.",
};

export const recordedNoteSentence = ({ recorded, list, listed }) => {
  if (recorded === null || recorded === undefined) {
    return "";
  }
  return (
    LIST_STATE_NOTE[list?.kind] ??
    (({ recorded: r }) =>
      `The index records “${r}”. This build did not understand what happened to the model ` +
      "list, so the picker above may not show it.")
  )({ recorded, listed });
};

// What leaves the machine, per state of the credential store. Two of these
// sentences are promises and the third is the refusal to make one.
//
// `LEAVES_EVERYTHING` is longer than §3.2 of the requirements, which says
// "once, at indexing". That is false for cloud embeddings: the question has to
// be embedded too, on every search (D29).
export const LEAVES_NOTHING = "Nothing leaves this machine. Search works on words.";
export const LEAVES_EVERYTHING =
  "Every piece of every document leaves this machine while indexing — and every question you " +
  "ask while searching.";
// `KeyState::Unreadable` is not `Absent`. Drawn as "nothing leaves" it is a
// promise made on the evidence of a keychain that is merely locked — and the
// same promise a key that is sitting right there would make false.
export const LEAVES_UNKNOWN =
  "Whether anything leaves this machine is unknown: the key store could not be read.";
// A different not-knowing from the one above, and worth its own words: there,
// the store was asked and would not answer; here, it answered something this
// build has no name for.
const LEAVES_UNSAID =
  "Whether anything leaves this machine is unknown: this build did not understand what the " +
  "key store answered.";

export const DISCLOSURE_TEXT = {
  present: LEAVES_EVERYTHING,
  absent: LEAVES_NOTHING,
  unreadable: LEAVES_UNKNOWN,
};

// Takes the `KeyState` field, not the whole `ModelSettings`. Every function in
// this block takes the field it renders — `indexStateSentence` has to, since
// `AdoptedModel` carries an `IndexSettings` of its own — and two conventions
// for the same kind of argument is the shape that let the brief's `keyPresent`
// go stale unnoticed.
export const disclosureSentence = (key) => DISCLOSURE_TEXT[key?.kind] ?? LEAVES_UNSAID;

// `KeyStoreFailure` is four values over six error variants, and the grouping is
// the whole content: what the person does next. Four sentences that read alike
// would satisfy a key-set check and throw the grouping away, which is why
// `render.test.js` also asserts they are four different sentences.
export const KEY_STORE_FAILURE_TEXT = {
  locked: "It is locked: unlock it and ask again — nothing about your configuration is wrong.",
  duplicate:
    "More than one credential is filed under this name: remove the spare, because this build " +
    "will not guess which of them is the key.",
  refused: "It would not hand the key over.",
  defect: "This is a defect in this build rather than a state of your machine: please send a bug report.",
};
const KEY_STORE_FAILURE_UNSAID =
  "This build did not understand what it answered: please send a bug report.";

// ⚠️ `reason` is diagnostic text and **not** the sentence to show.
// `mnema_secrets::Error::Unavailable` interpolates the platform's own error —
// an OS status on macOS and Windows, a D-Bus error on Secret Service — and a
// status code put in front of a person is not an action. It is appended only
// where `cause` names no action of its own and the action is therefore a bug
// report, and even there it is labelled as what it is. It carries no secret:
// every variant of that type names the credential reference and never the
// credential, by construction.
export const KEY_STORE_SHOWS_REASON = {
  locked: false,
  duplicate: false,
  refused: true,
  defect: true,
};

const diagnostic = (cause, reason) =>
  (KEY_STORE_SHOWS_REASON[cause] ?? false) && reason ? ` Details for a bug report: ${reason}` : "";

export const KEY_STATE_TEXT = {
  present: () => "The key is stored in this machine's credential store.",
  absent: () => "There is no key here — enter one to turn on the cloud models.",
  // Says what is not known, and never "there is no key". That sentence is the
  // one `Error::NoKey`'s own doc calls forbidden: it sends someone whose
  // keychain is merely locked to re-enter a key they already have.
  unreadable: (key) =>
    "The key store did not answer, so whether a key is there is unknown. " +
    `${KEY_STORE_FAILURE_TEXT[key.cause] ?? KEY_STORE_FAILURE_UNSAID}` +
    diagnostic(key.cause, key.reason),
};

export const keyStateSentence = (key) =>
  (
    KEY_STATE_TEXT[key?.kind] ??
    (() => "The state of the key store is unknown: this build did not understand what it answered.")
  )(key);

// What `set_key` failing means, and the one thing this window must not say
// about it. `set_key` checks the key with the provider **before** storing it
// (`models.rs:42-48`), so of its three reachable failures only one is a key the
// provider refused:
//
// | what happened | `Error` | was the key rejected |
// | provider refused it | `Provider` | yes |
// | provider not reached | `ProviderUnreachable` | **no** — nothing was decided |
// | provider accepted it, the store would not keep it | `Secrets` | **no** |
//
// The last is the sharpest: a locked keychain is an ordinary state of the
// machine by `KeyState`'s own doc, and "the key was not accepted" sends its
// owner for a new one. This is the seam `ProviderUnreachable` was split out of
// `Provider` for — the split survived four layers and used to die in this one
// sentence. The leading clause now states only what is true in all three, and
// the provider's own `Display` string, which carries the actual fact, follows.
export const keyNotSavedSentence = (error) => `the key was not saved: ${error}`;

// What `main.js` knows about `open_index` and `UnreadableCause` structurally
// cannot: whether the window has asked for an index at all, and what the ask
// answered. Not a wire union — the window produces it — but it is rendered, so
// it gets a table like the rest.
export const INDEX_OPENING_TEXT = {
  notAsked: () => "The index has not been opened yet.",
  // `AppState::open_index` assigns `db` before returning `Ok` (`state.rs`), so
  // an index that opened and then reports itself closed is a defect of this
  // build — not a folder anybody has to go and choose.
  opened: () =>
    "The index opened at start-up and is now unavailable. This is a defect in this build rather " +
    "than a state of your machine: please send a bug report.",
  // The permanent wall: an index written by a newer Mnema never opens at all,
  // and a window that skipped this correlation would draw it as an ordinary
  // cold start — a state someone waits out instead of acting on.
  failed: (opening) =>
    `The index could not be opened, so its settings cannot be read: ${opening.error}`,
};

export const UNREADABLE_CAUSE_TEXT = {
  notOpen: (index, opening) =>
    (INDEX_OPENING_TEXT[opening?.kind] ??
      (() =>
        "The index is unavailable, and this build does not know how the attempt to open it " +
        "ended."))(opening),
  readFailed: (index) =>
    "The index is open, but its settings could not be read from it. This is a defect in this " +
    `build: please send a bug report. Details: ${index.reason}`,
};

export const INDEX_SETTINGS_TEXT = {
  // The selects and the progress line carry the choices; what is left, and is
  // nowhere else in the window, is the width the vector space was built at.
  read: (index) =>
    index.activeSpace === null || index.activeSpace === undefined
      ? ""
      : `Active vector space #${index.activeSpace}, width ${index.embeddingDim ?? "unknown"}.`,
  unreadable: (index, opening) =>
    (UNREADABLE_CAUSE_TEXT[index.cause] ??
      ((i) =>
        `The settings could not be read, and this build does not know why. Details: ${i.reason}`))(
      index,
      opening,
    ),
};

export const indexStateSentence = (index, opening) =>
  (
    INDEX_SETTINGS_TEXT[index?.kind] ??
    (() => "The state of the index is unknown: this build did not understand what it answered.")
  )(index, opening);

// ⚠️ **Not a fraction, and never divided.** Four measured rules, all of which
// the obvious rendering breaks (`IndexRead::embedded_chunks`):
//
// - `embeddedChunks` counts the active space, `totalChunks` the whole index, so
//   even "X of Y" is already an inexact sentence — which is why the sentence
//   below says which is which rather than leaving a bare ratio.
// - `embeddedChunks` can exceed `totalChunks`: a vector outlives the chunk it
//   embeds, and `a_vector_outlives_the_chunk_it_embeds` holds the storage half
//   of that in the gate. A percentage of it is above 100, and clamping would be
//   this window inventing a number nobody measured.
// - Both are zero in this build, because nothing embeds yet (D29).
// - Zero with `activeSpace == null` is not "nothing is embedded", it is "the
//   question does not arise" — told apart by `activeSpace`, never by the zero.
const PROGRESS_NO_MODEL = "no embedding model chosen";
// And an index that could not be read is neither of those: drawn as "nothing
// chosen" it is the entrance to the harm `NoSuchSpace` was written to prevent.
const PROGRESS_UNKNOWN = "how many pieces have embeddings is unknown until the index can be read";

export const embeddingProgressText = (index) => {
  if (index?.kind !== "read") {
    return PROGRESS_UNKNOWN;
  }
  if (index.activeSpace === null || index.activeSpace === undefined) {
    return PROGRESS_NO_MODEL;
  }
  const head =
    `embeddings in the active space: ${index.embeddedChunks} of ${index.totalChunks} ` +
    "pieces in the whole index";
  return index.embeddedChunks > index.totalChunks
    ? `${head} — the first number counts one space and the second the whole index, and a vector ` +
        "can outlive the piece it embeds, so the first is sometimes larger; this is not an error"
    : head;
};

// `set_embedding_model` answers `AdoptedModel`, not `ModelSettings`, and its
// `model`, `dim`, `spaceId` and `created` sit **outside** `index` precisely so a
// read-back that failed on its own cannot take them with it. The first version
// of that command returned the settings it read back, which meant the window
// could not tell "nothing was written" from "written, and reading it back
// failed" — no wording could have told them apart, because the fact was not in
// the message. This sentence is the other end of that fix: it states the
// adoption from the fields that carry it, and adds the reading failure as a
// separate clause rather than letting it rewrite the first one.
//
// `created` comes from the field that states it and is never re-derived.
// `embeddedChunks` is the tempting proxy and is wrong in exactly one direction:
// it is identically zero in this build (D29), so every adoption would read as a
// freshly minted space.
export const adoptedModelSentence = (adopted, opening) => {
  const head =
    `The embedding model was recorded: ${adopted.model}, width ${adopted.dim}, ` +
    `space #${adopted.spaceId}.`;
  const space = adopted.created
    ? " A new vector space was created."
    : " An existing vector space was used.";
  // Only when the read-back actually failed. An unconditional tail passed every
  // assertion this file had for one round — the `created` test's only
  // `doesNotMatch` looked for the words "new vector space", which the tail does
  // not contain — so the reverse direction is now pinned on its own.
  const tail =
    adopted.index?.kind === "read"
      ? ""
      : " The settings could not be read back — that does not affect the model that was " +
        `recorded. ${indexStateSentence(adopted.index, opening)}`;
  return head + space + tail;
};

const price = (perToken) =>
  perToken === null || perToken === undefined
    ? "price unknown"
    : `$${(perToken * 1e6).toFixed(3)} per million tokens`;

// One sentence per `Refusal`, and the pairs are what the table is for. "The
// provider did not say whether this model writes text" and "the provider said,
// and text was not among it" are opposite statements *about the provider*; so
// are "stated no input limit" and "stated one in a shape this build cannot
// read". Each pair cost a review round to split upstream (F3/N2 and N1), and a
// `default` arm here would have folded both back at the last seam.
export const REFUSAL_TEXT = {
  inputTooSmall: (r) => `input limit ${r.limit} tokens, at least ${r.floor} needed`,
  noStatedLimit: () => "the provider did not state an input limit",
  // `raw` is provider text, capped to 64 bytes on the Rust side for exactly
  // this use — a malformed value must not become an unbounded label in a
  // picker. It reaches the DOM through `textContent` only, never as markup.
  limitNotUnderstood: (r) =>
    `the provider stated an input limit in a shape this build does not understand (${r.raw})`,
  noStatedOutputModalities: () => "the provider did not say whether this model writes text",
  noTextOutput: () => "this model does not write text",
};

const refusalText = (refusal) =>
  `unavailable: ${(REFUSAL_TEXT[refusal.kind] ?? (() => "this build did not recognise the reason"))(refusal)}`;

export const modelOptionLabel = (entry) => {
  const head = `${entry.id} — ${price(entry.pricePerToken)}, input ${entry.contextLength ?? "?"}`;
  return entry.refusal ? `${head} — ${refusalText(entry.refusal)}` : head;
};

// The balance has four states and none of them is zero. Zero is a number the
// provider actually sent; the other three are things we do not know, and each
// needs different words. Rendering "unknown" as 0 is what sends a funded user
// to pay again — the whole reason the type has four states rather than two.
//
// `Unreadable` carries `raw`, which is a `ProviderMessage` — a tagged object,
// not a string — and it is deliberately not interpolated: a shape this window
// misread would print "[object Object]" beside a number the person is about to
// act on, and the two defects the pair names are ours to report, not theirs to
// read.
export const BALANCE_TEXT = {
  known: (b) => `the account balance is $${b.amount.toFixed(2)}`,
  notStated: () => "the provider does not state a balance for this account",
  unreadable: () => "the balance arrived in a shape this application does not understand",
  envelopeNotUnderstood: () => "the provider's answer is not a shape this build knows",
};

export const keyAcceptedSentence = (status) =>
  "the key was accepted; " +
  (BALANCE_TEXT[status.balance?.kind] ??
    (() => "the balance arrived in a state this build does not know"))(status.balance);

// Records the provider listed and this build could not read. Silence here would
// mean a list quietly shorter than the provider's, with nothing saying so —
// the defect Task 1 spent three fix rounds removing one layer down.
//
// `id` is a tagged shape, not a string-or-null: Task 2's fix round gave it three
// states — a readable id, a value that is present but not a string, and no id
// field at all — because folding the last two together stated a fact about the
// provider that was false. Reading it as `r.id` here would print
// "[object Object]" for every record, which is how a distinction drawn upstream
// dies at the last seam.
export const RECORD_ID_TEXT = {
  known: (record) => record.id.id,
  notAString: (record) =>
    `record ${record.index}: the identifier is not a string` +
    (record.id.raw ? ` (${record.id.raw})` : ""),
  absent: (record) => `record ${record.index}: no identifier`,
};

const recordName = (record) =>
  (RECORD_ID_TEXT[record.id?.kind] ??
    ((r) => `record ${r.index}: the identifier is in a state this build does not know`))(record);

// The count is phrased with the number last on purpose. English needs "1
// record" against "3 records" and Ukrainian needs three forms rather than two;
// a count in front of a noun is a plural rule in every language this sentence
// will be translated into, and a window that gets one wrong reads as machine
// output. Nothing about the count is lost by moving it, and the dictionary task
// inherits one string instead of a rule.
export const unreadableSentence = (catalogue) => {
  if (!catalogue.unreadable) {
    return "";
  }
  const named = (catalogue.unreadableRecords ?? []).map(recordName);
  const tail = named.length ? ` (${named.join(", ")})` : "";
  return `records in the provider's list this build could not read: ${catalogue.unreadable}${tail}`;
};

// What to say about a whole catalogue, including the decision `provider_models`
// explicitly left here: whether zero selectable models is worth alarming
// anybody about, and — the part only this function can answer — whether it is
// the provider's own answer or something upstream that ate them. Both numbers
// have to still be present to tell those apart, which is why the command hands
// the window the `Catalogue` rather than its `entries`.
//
// A well-formed empty answer is a success by construction: `models_from_json`
// returns `entries: []` with `unreadable: 0` for `{"data":[]}`. Rendered as
// nothing at all it is a picker with no options and no explanation — the same
// pixel as a list that has not loaded, and an empty state is exactly where a
// defect stops being wrong and becomes invisible.
export const catalogueSentence = (catalogue) => {
  const records = unreadableSentence(catalogue);
  if ((catalogue.entries ?? []).length > 0) {
    return records;
  }
  return catalogue.unreadable
    ? `no model in the provider's list for this role could be read by this build — ${records}`
    : "the provider lists no models for this role";
};
