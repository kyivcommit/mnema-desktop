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
// The tagged unions that reach this file from `src-tauri/src/models.rs` and
// `crates/mnema-provider` are `KeyState`, `IndexSettings`, `UnreadableCause`,
// `KeyStoreFailure`, `Refusal`, `Balance`, `RecordId`, `Price`, `InputLimit`
// and `KeyRemoval`. They are named rather than counted, because a count is a
// definition and this list has grown three times. Every one of them exists because
// somebody measured a place where folding two of its values together stated, to
// a person, a fact nobody had established — the last two after a person pressed
// one button and read `$-1000000.000 per million tokens` under one model and a
// `$0.000` that was not free under six others.
// This is the last seam before that person: a distinction lost here is lost.
//
// So each is a **table**, not a `switch` with a `default`, and `render.test.js`
// asserts every table's key set is exactly the union's list of variants. A
// `default` arm is where two states quietly become one pixel; a missing key is
// a test failure. The unions declared in `models.rs` also have a Rust-side pin
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

// The id of one role's picker. Here rather than in `main.js` for the reason six
// discriminants moved here one round ago: `main.js` is an entry point with no
// `export`, so a test cannot see anything defined there and has to restate it.
// A restated rule is checked against the test's own copy of itself — changing
// the derivation in `main.js` left all 51 tests green while every picker in the
// window broke.
export const selectId = (role) => `${role}-model`;

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
// Carries the catalogue, because "the call succeeded" is not enough to say what
// a model's absence from the picker means — see `missingModelReason`.
export const listWasRead = (catalogue) => ({ kind: "read", catalogue });
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
// Why a recorded model is not in the picker, **when the list itself was read**.
//
// "The provider no longer lists this model" is a claim about a third party, and
// a call that succeeded does not establish it. `models_from_json` reads a
// record's `id` off the raw value *before* the decode that failed
// (`crates/mnema-provider/src/catalogue.rs:556-562`), precisely so a broken
// `pricing` or `architecture` block does not cost the one field that names the
// record — so `RecordId::Known { id }` is exactly a model the provider **does**
// list and this build could not read. Its id never reaches `entries`, so the
// picker has no option for it and it looks withdrawn.
//
// Before this table the window could print, one line above the other, under the
// same picker:
//
//   records in the provider's list this build could not read: 1 (vendor/m)
//   The index records “vendor/m”, but the provider no longer lists this model.
//
// Same model, adjacent lines, and the false one is the claim about someone
// else. The partial form is the likely one: a provider renaming a single field
// makes hundreds of records undecodable at once.
export const MISSING_MODEL_REASON = {
  // The provider named it and this build could not read the record. The one
  // arm that contradicts "withdrawn" outright.
  unreadableRecord: ({ recorded }) =>
    `The index records “${recorded}”. The provider does list it, but this build could not read ` +
    "its record, so the picker above does not offer it.",
  // Every record the provider sent is accounted for by id, and this model is in
  // none of them.
  withdrawn: ({ recorded }) =>
    `The index records “${recorded}”, but the provider no longer lists this model. ` +
    "It stays recorded; the picker above does not show it.",
  // Some record could not even be named — `RecordId::Absent` and `NotAString`
  // exist for exactly that — so any of them could be this model. Neither of the
  // two arms above is established, and saying either would be inventing one.
  unknown: ({ recorded }) =>
    `The index records “${recorded}”. It is not among the records this build could read, and ` +
    "some records could not be named at all, so whether the provider still lists it is unknown.",
};

// Which of the three, from the catalogue the call actually returned.
//
// The discriminant is **not** `unreadable > 0`, which was the cheap version:
// the question is whether this build can account, by id, for every record the
// provider sent. Zero unreadable records satisfies that vacuously, which is why
// a clean catalogue still says "withdrawn".
export const missingModelReason = (recorded, catalogue) => {
  const records = catalogue?.unreadableRecords ?? [];
  const named = records.filter((r) => r.id?.kind === "known");
  if (named.some((r) => r.id.id === recorded)) {
    return "unreadableRecord";
  }
  // `records.length >= unreadable` matters on its own: a core that sent the
  // count without the detail behind it leaves records this window cannot name,
  // and an empty `unreadableRecords` must not read as "nothing was unreadable".
  const accountedFor =
    named.length === records.length && records.length >= (catalogue?.unreadable ?? 0);
  return accountedFor ? "withdrawn" : "unknown";
};

export const LIST_STATE_NOTE = {
  // Nothing to say while it loads. Not "could not be read", which is a claim
  // about the machine that nothing has established yet.
  notAsked: () => "",
  read: ({ recorded, listed, list }) =>
    listed
      ? ""
      : MISSING_MODEL_REASON[missingModelReason(recorded, list?.catalogue)]({ recorded }),
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
  )({ recorded, listed, list });
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
  // Two situations, not one, and this build cannot tell which — the platform
  // error arrives already flattened into one variant. Measured 2026-08-11: a
  // macOS keychain that is not locked at all reaches this value when the
  // authorisation dialog is declined, and Linux reaches it both by a locked
  // collection and by a dismissed prompt. Prescribing only the unlock described
  // nothing for the situation somebody is most likely to be in, so both are
  // named and nothing about the cause is claimed.
  locked:
    "This build cannot tell which of two things happened: it is locked, or a confirmation " +
    "was asked for and not given. So unlock it and ask again — and if something asks you to " +
    "confirm this time, answer it. Nothing about your configuration is wrong.",
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

// ─────────────────────────────────────────────────────────────────────────────
// An empty field means two different things, and the window is the only side
// that can tell them apart.
//
// `set_key` refuses an empty submission before it calls anyone, and that guard
// **stays**: the command is reachable over the IPC, and that side is the one
// that decides whether a request leaves the machine. What the guard cannot know
// is that this window clears the field after a successful save — correctly, a
// credential does not live in a DOM input any longer than it must — so pressing
// the button with an empty field is the ordinary state of somebody whose key is
// stored and fine, and "an empty key was submitted, so nothing was sent to the
// provider and nothing was checked" reads to them as a failure. The owner met
// it on the second live run, and the fix for the twentieth introduced it.
//
// Keeping the key in the field was proposed and rejected, with the reason kept
// here because it is the kind of decision that gets re-argued: the input is
// already `type="password"`, so masking is not what is missing, presence is —
// and keeping the value would move the key from "in the window until you press
// the button" to "in the window for the whole session", where anything running
// in the page, a screenshot and an accessibility dump can all reach it.

// What the window knows about the key store before it has asked. Not a wire
// state — `KeyState` has no such variant, because the *core* always knows what
// it measured — and it is the same shape and the same reason as `indexNotAsked`
// and `listNotAsked`: the listeners below are registered before the first
// `model_settings` round trip finishes, so a press in that window would
// otherwise fall through a table's fallback and say this build did not
// understand an answer nobody had asked for.
export const keyNotAsked = () => ({ kind: "notAsked" });

// The sentence to show **instead of** submitting an empty field, or `null` when
// the empty field is genuinely the command's to answer.
//
// `null` rather than `""`: this is a decision and not an empty sentence, and
// the one call site tells them apart by identity. The state that sends is named
// by its own arm rather than by the absence of one.
//
// ⚠️ **`unreadable` does not send, and does not guess.** It is not "a key is
// stored" and it is not "none is". Sending would put the command's own sentence
// on the screen — true, and *identical to the one `absent` gets* — which folds
// the two states this window keeps apart everywhere else into one message, on
// the one screen where the difference decides whether somebody goes to find a
// key or goes to unlock their keychain. So it says what it knows: nothing was
// typed, and whether a key is already stored is unknown. It deliberately does
// not repeat what to do about the store; `keyStateSentence` is drawn beside it
// and `KEY_STORE_FAILURE_TEXT` is where that action lives.
export const EMPTY_FIELD_TEXT = {
  present: () =>
    "a key is already stored; this window cleared the field after saving it, so type a new one " +
    "only to replace it",
  // The one that goes to the command. With nothing stored there is nothing this
  // window knows that `Error::EmptyKey` does not, and its sentence is right.
  absent: () => null,
  unreadable: () =>
    "nothing was typed, and whether a key is already stored is unknown: the key store did not " +
    "answer",
  notAsked: () => "nothing was typed, and this window has not read the key store yet",
};

export const emptyFieldSentence = (key) =>
  (
    EMPTY_FIELD_TEXT[key?.kind] ??
    (() =>
      "nothing was typed, and this build did not understand what the key store answered, so " +
      "whether one is already stored is unknown")
  )(key);

// The field's own two labels, per the same state.
//
// **`absent`, `notAsked` and `unreadable` read alike here on purpose, and that
// is not the fold this file spends its time avoiding.** These two strings say
// what the button does and what the field is for; they are not statements about
// the store. "Check and save" is true whether or not a key is stored, while
// "Replace the key" claims one is — which is why only `present` gets it, and
// why a store that would not answer must not. The distinction between those
// three is carried where it belongs: `keyStateSentence` above, and
// `EMPTY_FIELD_TEXT` when the button is actually pressed.
export const KEY_FIELD_PLACEHOLDER = {
  present: () => "leave empty to keep the stored key",
  absent: () => "OpenRouter key",
  unreadable: () => "OpenRouter key",
  notAsked: () => "OpenRouter key",
};

// What macOS will do later, said before it happens rather than after.
//
// Measured 2026-08-11: that store authorises every read of a credential against
// the code identity that wrote it, and an ad-hoc signature is a hash of the
// binary. So **an update makes this application a stranger to its own key**, and
// the system asks for the login keychain password — from an application it
// cannot vouch for, with no warning and no explanation. Somebody meeting that
// cold should reasonably refuse it.
//
// The other two platforms are a different mechanism and this would be noise
// there: Secret Service unlocks per session and the Windows Credential Manager
// per logon, neither per binary. Hence the platform on the wire — see
// `ModelSettings::platform`, and the note there on why the window is not allowed
// to work it out from `navigator`.
//
// Only with a key stored: with none there is nothing to be asked about yet, and
// with a store that would not answer `keyStateSentence` is already saying the
// thing that matters.
const KEY_STORE_NOTE = {
  mac: () =>
    "macOS ties this key to the copy of Mnema that saved it, so after an update it will ask " +
    "once for your login keychain password before handing the key back. That request is this " +
    "application asking for its own key; Always Allow answers it until the next update.",
  windows: () => "",
  linux: () => "",
};

export const keyStoreNote = (platform, key) =>
  key?.kind === "present" ? (KEY_STORE_NOTE[platform] ?? (() => ""))() : "";

export const KEY_SUBMIT_TEXT = {
  present: () => "Replace the key",
  absent: () => "Check and save",
  unreadable: () => "Check and save",
  notAsked: () => "Check and save",
};

// The fallbacks are the neutral pair for the same reason `unreadable` gets it:
// a `kind` this build has never seen is not evidence that a key is stored.
export const keyFieldPlaceholder = (key) =>
  (KEY_FIELD_PLACEHOLDER[key?.kind] ?? KEY_FIELD_PLACEHOLDER.absent)(key);
export const keySubmitText = (key) => (KEY_SUBMIT_TEXT[key?.kind] ?? KEY_SUBMIT_TEXT.absent)(key);

// What `set_key` failing means, and the one thing this window must not say
// about it. `set_key` refuses an empty submission before it calls anyone and
// checks the key with the provider **before** storing it (`models.rs:58-67`),
// so of its reachable failures only one is a key the provider refused:
//
// | what happened | `Error` | was the key rejected |
// | provider refused it | `Provider` | yes |
// | provider not reached | `ProviderUnreachable` | **no** — nothing was decided |
// | provider accepted it, the store would not keep it | `Secrets` | **no** |
// | nothing was typed | `EmptyKey` | **no** — nothing was even asked |
//
// The last two are the sharpest. A locked keychain is an ordinary state of the
// machine by `KeyState`'s own doc, and "the key was not accepted" sends its
// owner for a new one. This is the seam `ProviderUnreachable` was split out of
// `Provider` for — the split survived four layers and used to die in this one
// sentence. `EmptyKey` is the one a person met on the first real run: the empty
// string went to the provider, and "Missing Authentication header" came back
// and was rendered here as a verdict on a key nobody had typed. The leading
// clause states only what is true of every row, and the `Display` string that
// follows carries the actual fact.
// Every line of prose the settings screen draws goes through here, and their
// text arrives from two languages with opposite conventions: sentences written
// in this file, and `Error` renderings from Rust, which by that language's
// convention begin lower case and carry no full stop — `keyNotSavedSentence`
// below interpolates one whole. Beside a neighbour that *is* a proper sentence
// the result read as unfinished, which the owner met in the acceptance run of
// 2026-08-11.
//
// **Shaped once, at the seam, rather than in each of the fourteen producers.**
// Fixing the words instead would mean editing exactly the ones whose lower case
// is correct where it is written, and would leave the next producer free to pick
// either — which is not a guess: the first attempt at this covered the two
// action lines only, and the very next screenshot showed the same mismatch one
// line further down, between `index-state` and `embedding-progress`.
//
// Idempotent on purpose: a text that is already a sentence comes back unchanged,
// and an empty one stays empty rather than becoming a lone full stop.
export const asSentence = (text) => {
  const trimmed = `${text ?? ""}`.trim();
  if (trimmed === "") return "";
  const opened = trimmed[0].toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(opened) ? opened : `${opened}.`;
};

export const keyNotSavedSentence = (error) => `the key was not saved: ${error}`;

// What pressing "Remove the key" actually did, per `KeyRemoval`.
//
// It was one sentence — "the key was removed" — written unconditionally on a
// command that answered `Ok(())` to two different events. `mnema_secrets::
// forget` is idempotent by design, so somebody who had entered no key, or whose
// key a second window had already taken, pressed the button and was told this
// application had just removed one. The same press and the same class as the
// empty field `set_key` refuses: a button reporting an event it did not cause
// (whole-branch review, I1).
//
// `nothingToRemove` says what is true of the machine now as well as what did
// not happen, because "there was nothing to remove" alone reads as a refusal.
export const KEY_REMOVAL_TEXT = {
  removed: () => "the key was removed",
  nothingToRemove: () => "there was no key to remove — this machine has none stored",
};

export const keyRemovedSentence = (removal) =>
  (
    KEY_REMOVAL_TEXT[removal?.kind] ??
    // Deliberately without the words the `removed` arm uses. An honest
    // sentence built out of "was removed" is still a sentence a person skims as
    // a removal, and it is what an oracle for "never says one was removed"
    // cannot tell apart either.
    (() =>
      "this build did not understand what the key store answered, so what happened to the key " +
      "is unknown")
  )(removal);

// The failure half of the same press. Not folded into `keyRemovedSentence`'s
// table: a store that refused says nothing about whether a key is there, and
// the key is still where it was — which is the opposite of both arms above.
export const keyNotRemovedSentence = (error) => `the key could not be removed: ${error}`;

// The failure sentences for the three commands that record a model, and for the
// list one role's picker is filled from. They are here rather than in `main.js`
// for the reason this file's own header gives and `main.js`'s repeats: a
// sentence outside these tables is a sentence `render.test.js` cannot reach,
// and the one that was outside them is the one that spent this branch telling
// people about an event that had not happened.
export const listNotReadSentence = (error) => `the model list could not be read: ${error}`;
export const embeddingModelNotRecordedSentence = (error) =>
  `the embedding model was not recorded: ${error}`;
// `ROLE_NAME[role] ?? role`, the same fallback `roleRecordedSentence` has. In
// `main.js` this was written without it, so a role this build does not know
// produced "the undefined model was not recorded" — the cost of one sentence
// living outside the table its own success sentence is in.
export const roleNotRecordedSentence = (role, error) =>
  `the ${ROLE_NAME[role] ?? role} model was not recorded: ${error}`;

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

// ⚠️ **Not a fraction, and never divided.** Three measured rules, all of which
// the obvious rendering breaks (`IndexRead::embedded_chunks`):
//
// - `embeddedChunks` counts the active space, `totalChunks` the whole index, so
//   even "X of Y" is already an inexact sentence — which is why the sentence
//   below says which is which rather than leaving a bare ratio.
// - `embeddedChunks` can exceed `totalChunks`: a vector outlives the chunk it
//   embeds, and `a_vector_outlives_the_chunk_it_embeds` holds the storage half
//   of that in the gate. A percentage of it is above 100, and clamping would be
//   this window inventing a number nobody measured.
// - Zero with `activeSpace == null` is not "nothing is embedded", it is "the
//   question does not arise" — told apart by `activeSpace`, never by the zero.
//
// A fourth rule said "both are zero in this build, because nothing embeds yet
// (D29)". It is gone because it stopped being true the moment there was a
// command that embeds; it is recorded here rather than deleted silently,
// because a stale rule in a list of measured ones is worse than no list.
const PROGRESS_NO_MODEL = "no embedding model chosen";
// And an index that could not be read is neither of those: drawn as "nothing
// chosen" it is the entrance to the harm `NoSuchSpace` was written to prevent.
const PROGRESS_UNKNOWN = "how many pieces have embeddings is unknown until the index can be read";

// "1 pieces" is the kind of sentence that makes a person doubt the number beside
// it, the same reason `embeddingsCount` exists further down.
const piecesCount = (n) => (n === 1 ? "1 piece" : `${n} pieces`);

// **The third number, and the whole safety argument for the queue's own rule.**
//
// `Db::chunks_needing_embedding` lets a chunk the provider refused leave the
// queue and does not offer it again until its text changes. That chunk stays in
// the database, the document still shows it and keyword search still finds it —
// and search by meaning will not return it again. `totalChunks −
// embeddedChunks` reads as "not got to them yet", and for these pieces nobody
// ever will, so the difference is exactly the wrong way to learn about them.
// Until this sentence existed, `Db::failed_chunk_count` had no caller outside
// the tests and the number was in front of nobody.
//
// **Said at zero as well**, and that is deliberate rather than noise: a clause
// that appears only when something is wrong cannot be told apart from a build
// that does not report refusals at all, which is the silence this exists to
// break. The harsh half — that they are not retried — is said only when there is
// something for it to be about.
//
// A third arm for "the window was not told", because `failedChunks` absent is
// not `failedChunks: 0`. Saying "none were refused" about a payload that carries
// no such number states a fact this window did not receive — the mistake
// `KeyState`, `Balance` and `Refusal` are each split into named states to avoid.
const refusedClause = (failed) => {
  if (typeof failed !== "number") {
    return "; how many pieces the provider refused is not in what this window was sent";
  }
  return failed > 0
    ? `; ${piecesCount(failed)} the provider refused, and it will not try again until their ` +
        "text changes, so search by meaning does not return them"
    : "; none were refused by the provider";
};

// ⚠️ **While a job is running these counts are stale, and the refusal clause is
// the one that must not be said anyway.**
//
// This line is redrawn only when `model_settings` is asked again, which on the
// embedding path happens at the run's *ending*. So for the whole length of the
// only operation that changes the third number, it holds whatever was true
// before the run began — and because the clause above states the zero case
// rather than omitting it, what it holds is not a stale silence but a stale
// assertion: `none were refused by the provider`, on screen at the same moment
// as `2 refused in this run` beside the progress bar. Review round 1, Important
// 2. Saying the count at zero is still right; saying it about a moment that has
// passed is not.
//
// **Suppressed rather than redrawn**, and the choice is between two honest
// options:
//
// - Redrawing during the run would make it approximately live and drag the whole
//   settings block with it four times a second — including `showRecorded`, which
//   writes `select.value` and would fight a person using the picker — and two
//   live numbers read at different instants would go on disagreeing by up to a
//   batch, which is the trap they were worded apart to avoid in the first place.
// - This says what is true: the counts are from before the run, the run's own
//   line is the one that is moving, and no claim is made about refusals at a
//   moment this window has not read.
//
// It covers a walk as well as an embedding run, deliberately: a walk moves
// `totalChunks`, so the same staleness applies to the denominator.
const PROGRESS_DURING_A_RUN =
  "; a job is running, so these are the counts from before it started — the line beside the " +
  "progress bar is the one with this run's own";

export const embeddingProgressText = (index, jobRunning) => {
  if (index?.kind !== "read") {
    return PROGRESS_UNKNOWN;
  }
  if (index.activeSpace === null || index.activeSpace === undefined) {
    return PROGRESS_NO_MODEL;
  }
  const head =
    `embeddings in the active space: ${index.embeddedChunks} of ${index.totalChunks} ` +
    "pieces in the whole index";
  const counted =
    index.embeddedChunks > index.totalChunks
      ? `${head} — the first number counts one space and the second the whole index, and a ` +
        "vector can outlive the piece it embeds, so the first is sometimes larger; this is not " +
        "an error"
      : head;
  return counted + (jobRunning ? PROGRESS_DURING_A_RUN : refusedClause(index.failedChunks));
};

// ─────────────────────────────────────────────────────────────────────────────
// The one bar, and the picture it must stop showing when a run is over.
//
// Found by the live acceptance run of 2026-08-13, and by nothing else here. A
// run died when the network dropped; the owner turned the network back on and
// then **waited**, because the window still looked like it was working. It was
// not — the run had ended, the slot was free, nothing was running. The bar was
// half of why: it stayed partly filled, in the same blue a live run draws, and
// a partly-filled blue bar is the visual language of "in progress". No test in
// this repository can see that, because what was wrong was not a value but what
// a person concludes from a picture.
//
// Three states rather than two. Drawing every ending the same way would trade
// this defect for its mirror — a run that embedded everything must go on
// looking finished — so "ran to the end" and "ended with work left" are kept
// apart here exactly as they are in the sentences below.
//
// The strings are exported rather than written into `main.js` for the reason
// every other wire spelling in this file is: `"stoped"` assigned to a dataset
// key is a selector in `style.css` that silently matches nothing, and nothing
// reddens. `render.test.js` checks the stylesheet against these same constants.
export const BAR_RUNNING = "running";
export const BAR_FINISHED = "finished";
export const BAR_STOPPED = "stopped";

// One arm per `EndReason`, and it is **not `STOPPED_CLEANLY` under a second
// name.** That table answers whether phase 2 finished everything phase 1 handed
// it, and is never read without `complete` beside it (`reconciliationRan`);
// this answers only whether the bar reached the end of what it was drawn
// against. The two agree on every value today and are answers to two different
// questions — the pair this file keeps apart everywhere else — so folding them
// together would make a later change to either silently move the other.
export const BAR_RAN_TO_THE_END = {
  completed: true,
  cancelled: false,
  failed: false,
  brokenWorker: false,
  rulesNotApplied: false,
  rootUnavailable: false,
  volumeMissing: false,
};

// An ending this build does not recognise is drawn as stopped: "finished" is
// the claim, and an unknown reason establishes nothing. It is the cautious side
// to be wrong on, and the same choice `reconciliationRan` makes about a `reason`
// it has never seen.
export const barState = (ended) =>
  BAR_RAN_TO_THE_END[ended?.reason] ?? false ? BAR_FINISHED : BAR_STOPPED;

// ─────────────────────────────────────────────────────────────────────────────
// The embedding job.
//
// A run's sentences, kept apart from the walk's above. `endingSentence` appends
// a reconciliation clause — "nothing was removed from the index, a file deleted
// from the folder could still answer a search" — which is a statement about a
// folder walk and would be both irrelevant and misleading after a run that
// reconciled nothing and walked nothing. The two jobs share a slot, a bar and a
// Cancel button, and they do not share a vocabulary.
//
// ⚠️ **Every count the ending itself carries is this run's, and the settings
// line above is the space's.** `job::Ended::refused` starts again at zero on the
// next run; `IndexRead::failed_chunks` counts every refusal the space still
// holds. They are different numbers that could otherwise be read as one, so the
// sentences below say "in this run" wherever they name one.
//
// **The ending now states the index's pair as well, and that is the one place
// the rule above is deliberately crossed** — see `embedIndexTail`, and note
// that it is crossed by *naming the other scope out loud* rather than by
// leaving a number unattributed. The pair comes from the same `model_settings`
// read the settings line is drawn from, so the two cannot disagree; what they
// must not do is read as each other, and each says whose number it is.

// What one report says while a run is going.
//
// `secondsLeft` is `null` before anything is measured — a real state, and one
// that must not render as `0` (`job::Progress::seconds_left`). The refusals are
// named as soon as there are any: a person watching the bar is the person the
// third number is for.
export const embedProgressLine = ({ done, total, refused, secondsLeft }) => {
  const eta =
    secondsLeft === null || secondsLeft === undefined ? "estimating…" : `${secondsLeft}s left`;
  const gaveUp = refused > 0 ? `, ${refused} refused in this run` : "";
  return `embedding: ${done} of ${total}${gaveUp} — ${eta}`;
};

// What a run leaves on the screen when it is over, in this run's own terms.
const embedRefusedTail = (refused) =>
  refused > 0
    ? `, ${piecesCount(refused)} the provider refused in this run — they stay in the index and ` +
      "in keyword search, and search by meaning will not return them until their text changes"
    : "";

// Where the index stands now, read back rather than worked out.
//
// **Never `embeddedChunks` from before the run plus the ending's own `done`.**
// That is the arithmetic `main.js` refuses at both endings for the reason it
// states there: two numbers added together can come to disagree with the
// database, and one read from it cannot. So this takes the `IndexRead` that
// `model_settings` answered *after* the run and states what it says.
//
// `null` — not a zero, not an empty pair — for every state where this window
// cannot say: before `model_settings` has been asked again, which is the first
// draw of every ending; an index that could not be read; and no active space at
// all, where by `IndexRead::embedded_chunks`'s own third rule the question does
// not arise rather than answering zero.
const indexNow = (index) => {
  if (index?.kind !== "read") return null;
  if (index.activeSpace === null || index.activeSpace === undefined) return null;
  if (typeof index.embeddedChunks !== "number" || typeof index.totalChunks !== "number") {
    return null;
  }
  return { embedded: index.embeddedChunks, total: index.totalChunks };
};

// The pair, with both scopes named. `embeddedChunks` counts the active space
// and `totalChunks` the whole index, so a bare ratio is already an inexact
// sentence — the rule `embeddingProgressText` is written to further up, and the
// reason this says which is which instead of printing `64 of 227`.
//
// When the first number is the larger — a vector outliving the piece it embeds,
// which is legitimate — this reads as odd rather than as an error, and the
// settings line one element further down, drawn from the same read at the same
// moment, is what explains it. That paragraph is not repeated here.
const withAVector = (now) =>
  `${piecesCount(now.embedded)} with a vector, of ${now.total} in the whole index`;

// **What this run did is not how much of the index is done, and the window said
// only the first.** Two consecutive runs on the owner's archive printed
// `32 of 227` and then `32 of 195`: both true, both this run's, the right-hand
// number the queue as it stood when that run started. The first run taught the
// wrong meaning, because there the queue happened to equal the whole index — so
// the second read as "it did 32 again, nothing moved" when 64 pieces had a
// vector by then. Live acceptance run of 2026-08-13.
//
// Empty when `indexNow` could not say, which is the honest answer and also the
// first draw of every ending: `main.js` writes the run's own sentence the
// instant the ending arrives and restates it with this when the settings come
// back, rather than holding a moving progress line on screen across a database
// read.
const embedIndexTail = (now) =>
  now === null ? "" : ` The active space now has ${withAVector(now)}.`;

// Both endings that leave work behind say so, and it is the same fact in both:
// the queue is computed from the index rather than stored, so nothing has to be
// recovered and a second press simply carries on. It is what a person needs
// after a network drops in the middle of a run.
//
// **It names the press, not only the property.** "Whatever this run embedded
// stays, and starting again continues from there" was true and was a statement
// about the system; what somebody sitting in front of a run that died needs is
// what to do and where it will resume — and the number is what makes that an
// instruction rather than a reassurance. The owner waited in front of exactly
// this sentence.
//
// "Whatever this run embedded" and not "what was embedded", because the count
// can be zero — a run that failed on its very first batch, or one stopped in the
// same second it was started — and the shorter wording states that something was
// embedded on exactly the endings where nothing was.
//
// "Embed" and not the button's whole label. The control says "Embed what is
// indexed" today and the React interface will relabel it; the first word is
// what a person scans a row of buttons for, and it is the part of the label
// least likely to move.
const embedResumable = (now) =>
  now === null
    ? " Whatever this run embedded stays: press Embed again to continue from there."
    : ` Whatever this run embedded stays: press Embed again to continue from ${withAVector(now)}.`;

// `EndReason`'s four walk-only variants. `walk_job.rs` is their only writer, so
// no embedding run produces one — and they have an arm because a table with a
// missing key renders a fallback instead of failing, and this file's rule is one
// arm per variant so that a variant added later reddens a test rather than
// disappearing into a default. It says what it was told rather than inventing a
// sentence about a folder.
const notAnEmbeddingEnding = ({ reason, done, total }) =>
  `ended (${reason}) after ${done} of ${total}`;

// Every arm takes the run's own payload and, second, the index pair `indexNow`
// derived — or `null`, which every arm has to render as a complete sentence
// rather than as a gap, because it is what the first draw of every ending gets.
//
// **"in this run" on the counts, and it is not decoration.** The head of each
// sentence states a pair the reader now sees beside a second pair from another
// scope, and this file's own rule two blocks up is that a sentence says "in this
// run" wherever it names one of this run's numbers. The head did not, and the
// heads are the numbers the owner misread.
export const EMBED_ENDING_TEXT = {
  // `total === 0` is the ordinary answer to a second press, and it is not "all
  // done": the queue is what has no vector *and* is not already refused, so
  // zero can also mean everything left has been given up on. This says only
  // what this run did, which is nothing — and then, since that is exactly the
  // ending after which somebody asks "so is it finished?", where the index
  // stands.
  completed: ({ done, total, refused }, now) =>
    (total === 0
      ? "nothing was waiting to be embedded"
      : `finished: ${done} of ${total} embedded in this run${embedRefusedTail(refused)}`) +
    embedIndexTail(now),
  // `total === 0` here is **not** the empty queue it is on the arm above, and
  // must not borrow its sentence. `mnema_embed::run` asks whether it was
  // cancelled before its first batch, so a Stop landing in that instant has the
  // pass measuring a queue and reporting none of it, and the `0` that reaches
  // this window is "not known" rather than "there was nothing". A run stopped
  // that early says how far it got — nowhere — and states no total, because
  // nobody measured one it could state.
  cancelled: ({ done, total, refused }, now) =>
    (total === 0
      ? `stopped before anything was embedded, at your request${embedRefusedTail(refused)}.`
      : `stopped after ${done} of ${total} embedded in this run, at your request` +
        `${embedRefusedTail(refused)}.`) + embedResumable(now),
  failed: ({ done, total, refused, message }, now) =>
    `failed after ${done} of ${total} embedded in this run${embedRefusedTail(refused)}` +
    (message ? `: ${message}.` : ".") +
    embedResumable(now),
  brokenWorker: notAnEmbeddingEnding,
  rulesNotApplied: notAnEmbeddingEnding,
  rootUnavailable: notAnEmbeddingEnding,
  volumeMissing: notAnEmbeddingEnding,
};

// `index` is the whole `IndexRead` — the field of `ModelSettings`, the same one
// `embeddingProgressText` and `discardOffer` are handed — and it is optional:
// called with one argument this answers the run's own sentence and nothing
// about the index, which is what the ending's first draw needs and what every
// caller that has no settings to hand should get.
export const embedEndingSentence = (ended, index) =>
  (EMBED_ENDING_TEXT[ended.reason] ?? notAnEmbeddingEnding)(ended, indexNow(index));

// The same line a second time, from the settings read back after the run — or
// `null` for "leave the line alone".
//
// **`null` rather than a sentence, and the caller writes nothing at all for
// it.** This is `discardOffer`'s shape, for `discardOffer`'s reason: a decision
// is not an empty sentence, and it belongs where `render.test.js` can reach it.
// Written in `main.js` as an `if`, the decision would be a branch no test in
// this repository can see — the argument this file's header makes, and the one
// the bar's own state was moved here under.
//
// The one thing it refuses on: **the status line having been claimed by a
// newer press.** The read is an IPC round trip wide and somebody can press
// Embed inside it. Landing then, this would paint the previous run's ending —
// carrying a pair of numbers measured before the new run started — over a line
// describing a run in flight. That is the stale assertion this cycle has
// already removed from the settings line (Important 2), from the discard
// button's label (Minor C) and from the bar, and it must not come back in
// through the door built to fix it.
//
// ⚠️ **It asks a generation and not `jobRunning`, and that is a measured
// correction rather than a refinement.** The first version of this guard asked
// the flag, which is set *after* the await on both press paths — so the flag
// lags the truth by one IPC round trip, in **both** directions, and the review
// of `3b18859` drove `main.js` and produced both:
//
//   - reads `false` while a newer run is live, because that run's first
//     progress event can arrive before its own `invoke` resolves (`main.js`
//     says so itself, above `followUntilIdle`) — so the guard let exactly the
//     paint-over it was written to stop happen anyway; and
//   - reads `true` with nothing live at all, because a run that ends before its
//     own `invoke` resolves has the flag set for a run already over — so the
//     guard suppressed a restatement that nothing was competing with, and
//     **nothing ever retries it**. That lands on `total === 0`, the ordinary
//     answer to a second press. The guard defended the rare case by breaking
//     the common one, which is the shape this branch has now paid for twice.
//
// A generation is exact in both directions because it is incremented **before**
// the await, by the press itself, so there is no window in which it lags. It
// counts presses that claim the status line, not jobs that are running, and it
// is deliberately not rolled back by a refused press: a refusal writes its own
// sentence to that same line, and that sentence is newer than this one and is
// what the person needs to read.
export const restatedEnding = (ended, index, ownGeneration, latestGeneration) =>
  ownGeneration === latestGeneration ? embedEndingSentence(ended, index) : null;

// A run that never started. The refusals are `Error::NoKey`, `Error::Secrets`,
// `Error::JobAlreadyRunning` and `Error::Index(_)` — the last from
// `open_job_index`, which this command calls instead of `with_index`, so
// `Error::IndexNotOpen` is not among them. The first three already say what
// they are and what to do about it; `Error::Index(_)` does not — its
// `Display` just forwards whatever SQLite said. This sentence only says that
// nothing started, so a message about a key (or an unreadable index) is not
// read as a run that failed halfway.
export const embedNotStartedSentence = (error) => `nothing was embedded: ${error}`;

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
// `embeddedChunks` is the tempting proxy — wrong in exactly one direction, a
// found-but-empty space reading as freshly minted, never the reverse (minted
// is always zero, so it never reads as found). See the note above for why
// that direction now fires only on found-and-empty spaces instead of every
// found space.
// The two values `existingVectors` may take on the wire, spelled once. They are
// here and not in `main.js` for the reason every other wire spelling is: a typo
// in a literal over there is a rejected command with a message about arguments,
// on the one press in this window that a person had to be asked about first.
// `tests/commands.rs` sends both through the real handler, which is what makes
// these two strings checked rather than agreed.
export const KEEP_EXISTING_VECTORS = "keep";
export const DISCARD_EXISTING_VECTORS = "discard";

// `1 embeddings` is the kind of sentence that makes a person doubt the number
// beside it, and this number is the whole content of the confirmation.
const embeddingsCount = (n) => (n === 1 ? "1 embedding" : `${n} embeddings`);

// Whether this window may offer to throw vectors away, and what it would say
// they cost — `null` for "do not offer", never a partly-filled offer.
//
// `model` is the change that was refused; without one there is nothing to
// confirm. The rest is one rule: **the number on the button is the number that
// will go, and there is no button when there is nothing to go.**
//
// - An index it could not read has no number in it, so the button would have to
//   fall back to "are you sure?", which is the sentence this whole control
//   exists instead of.
// - `embeddedChunksEverywhere === 0` is nothing to destroy — no space in the
//   index holds anything — so a confirmation would be a question about nothing.
// - `key.kind !== "present"` is not about price but about the offer being real.
//   `refusedChange` is set on every failed change, and `set_embedding_model`
//   fails on the credential store before it ever reaches the index — so without
//   this, a refusal that means "you have entered no key" produces a button
//   offering to delete embeddings, which is destruction proposed as the cure for
//   somebody else's ailment.
// - **A job running is the same rule as an index that could not be read.** The
//   counts here were taken before the run and are moving while it goes, so this
//   window cannot state what the button costs — and the whole control exists
//   instead of a button that says "are you sure?". Without it, a button left
//   from an earlier refusal sits there through the run still naming the count it
//   was drawn with, which is Important 2's stale assertion wearing a label
//   instead of a sentence. Pressing it is refused by the slot in any case, so
//   what is withdrawn is a control that could not have worked.
//
// **The number is `embeddedChunksEverywhere` and not `embeddedChunks`, and the
// guard that stood here instead is gone.** Review round 1 fixed the divergence —
// the command retires every space in the way while the label named one — by
// withholding the button unless `spaceCount === 1`. That was the wrong half to
// fix: `Db::adopt_embedding_model` never removes the space it moves off, so
// anybody who has ever tried a second model has two spaces for the life of the
// index (`tests/adopt.rs`, `returning_to_a_model_already_tried_creates_nothing`,
// pins it at two), and the button would then never appear again — with no other
// way to change the model at all. The number was already right in that state,
// because an abandoned space is empty and contributes nothing; what was wrong
// was naming one space in the sentence. So the sum is stated and no space is
// named, and the guard has nothing left to hide.
//
// ⚠️ **What this does not reach**, since a guard's gaps belong beside it. **Two
// of them, listed rather than counted** — this said "one gap" and a second
// arrived with the embedding job one commit later, which is the shape this
// project pays for most often:
//
// 1. A change refused by the *provider* — unreachable, or a model it does not
//    have — leaves this window with a key present, a full index and no way to
//    tell that refusal from the index's. The button appears, and its sentence
//    about what the index holds is still true; pressing it destroys nothing,
//    because the provider check runs before the index is touched, and produces
//    the same provider refusal again.
// 2. A change refused because **a job holds the slot** does not survive that
//    argument, because a job ends and a provider outage does not. Refused
//    mid-run, redrawn at the run's ending against a count the run has just made
//    *larger*, the button would then succeed — destroying exactly what the run
//    paid for, for a refusal that had nothing to do with vectors and that
//    waiting would have resolved. That one is closed one layer up, by
//    [`changeToConfirm`], because it is decidable from state and this function
//    is not given the reason. Review round 1, Important 1.
//
// Telling refusals apart in general needs them to carry their own shape rather
// than a string, which is deliberately not this cycle's work — so gap 1 stands,
// and **one more thing is deferred with it rather than separately**, because the
// same typed refusal closes both: [`changeToConfirm`] is given `jobRunning` read
// *after* the await, so it answers "is a job running now" rather than "was this
// refusal about the slot". The window is one IPC return wide. Whoever gives
// `Error` a shape the wire carries closes gap 1 and that at once; closing either
// alone leaves the other looking handled.
export const discardOffer = (model, index, key, jobRunning) => {
  if (model === null || model === undefined) return null;
  if (jobRunning) return null;
  if (key?.kind !== "present") return null;
  if (index?.kind !== "read") return null;
  if (!(index.embeddedChunksEverywhere > 0)) return null;
  return { model, embeddedChunks: index.embeddedChunksEverywhere };
};

// What a refused change leaves behind for the confirmation button to act on, or
// `null` for "nothing to confirm".
//
// **The one reason for a refusal this window can name from state**, and it has
// to be named here rather than in `discardOffer`, which is given the model and
// the index and never the reason. A change refused while a job holds the slot is
// not a change anything should be offered about: nothing is in the way, no
// vectors need destroying, and the answer is to wait. Setting it anyway is what
// turned the one destructive control in this window into the cure for "a job is
// already running" — see gap 2 on `discardOffer`.
//
// It **clears** rather than leaving whatever was there. A refusal is the last
// thing this window knows about the person's attempt, and the last one was not
// about vectors; an offer surviving from an earlier one would be a button
// answering a question nobody asked twice. Pressing the picker again brings the
// real refusal, and the offer with it.
export const changeToConfirm = (model, jobRunning) => (jobRunning ? null : model);

// The label on the button, and the line under it. Both take the offer `null`
// included and answer with the empty string for it, so that `main.js` writes no
// literal of its own — including the empty one that clears them.
//
// **It names no space**, which is a correction rather than an omission. It said
// "in vector space #N" while the change retires every space in the way, so the
// number and the place disagreed the moment there was more than one. The number
// is now the whole index's and the sentence says so; which spaces actually went
// is `retiredSpacesClause`, afterwards, from what the command measured.
export const discardVectorsLabel = (offer) =>
  offer === null
    ? ""
    : `Change to ${offer.model} and delete the ${embeddingsCount(offer.embeddedChunks)} ` +
      "this index holds";

// What it costs, in the two directions that matter: what goes, and what does
// not. The second half is not reassurance — it is the difference between this
// button and one that would look identical and remove the archive.
export const discardVectorsNote = (offer) =>
  offer === null
    ? ""
    : `${embeddingsCount(offer.embeddedChunks)} will be deleted from this machine, and the new ` +
      "model has to embed everything again before search by meaning finds anything. Your " +
      "documents, their text and the keyword search are not touched.";

// What a confirmed change actually destroyed, reported by the command rather
// than by the button.
//
// They are two different numbers about two different moments and this window
// says the second: the button's came from `embeddedChunks`, which counts the
// active space at the moment before the press, and `retired` is what the index
// counted as it destroyed it. A person who paid for embeddings is owed the
// second.
//
// Empty for every call that retired nothing, which is every refused change and
// every confirmed one that met nothing in the way.
export const retiredSpacesClause = (retired) =>
  !retired || retired.length === 0
    ? ""
    : ` ${retired
        .map(
          (space) =>
            `Vector space #${space.spaceId} was retired and its ` +
            `${embeddingsCount(space.embeddedChunks)} deleted.`,
        )
        .join(" ")}`;

export const adoptedModelSentence = (adopted, opening) => {
  const head =
    `The embedding model was recorded: ${adopted.model}, width ${adopted.dim}, ` +
    `space #${adopted.spaceId}.`;
  const space = adopted.created
    ? " A new vector space was created."
    : " An existing vector space was used.";
  // Conditional for the reason the read-back clause below is: an unconditional
  // tail is a sentence no assertion in this suite distinguishes from its own
  // absence, which is how one of these got through a whole round.
  const retired = retiredSpacesClause(adopted.retired);
  // Only when the read-back actually failed. An unconditional tail passed every
  // assertion this file had for one round — the `created` test's only
  // `doesNotMatch` looked for the words "new vector space", which the tail does
  // not contain — so the reverse direction is now pinned on its own.
  const tail =
    adopted.index?.kind === "read"
      ? ""
      : " The settings could not be read back — that does not affect the model that was " +
        `recorded. ${indexStateSentence(adopted.index, opening)}`;
  return head + space + retired + tail;
};

// A stated zero, a number that cannot be a price, a value this build cannot
// read, and nothing said at all — `Price`'s own states, named rather than
// counted, and every one of them arrives at this line as something to say
// rather than as a number to format.
//
// Two of them were on the screen at the first real run. `-1`, which the
// provider sends for a model it prices at routing time, went through the
// multiplication below and printed `$-1000000.000 per million tokens`. And all
// six rerank models state `"prompt": "0"` — true, and billed per search rather
// than per token, so `$0.000 per million tokens` told six models' worth of
// people they would not be charged. Nothing in the payload states the
// per-search price, so this window cannot say what they cost; what it can do is
// stop concluding "free" from a zero.
//
// **A noun phrase naming the statement, and no dash inside it.** "No charge per
// token stated" was the first wording, and it fails in the two ways this whole
// change is about. It reads as *no price was stated* — which is `notStated`,
// the one neighbour this sentence exists to be told apart from, so a state
// split in the type merged again in the words. And the label is assembled as
// `id — price, limit — refusal`, so a sentence carrying its own em-dash put
// three in one line and the field boundaries stopped being readable. Neither is
// visible to a test that only asserts the states read differently: the strings
// differ and a person does not.
const ZERO_PRICE = "a stated price of $0 per token (not the same as free)";
// A positive price small enough that `toFixed(3)` of a million tokens would
// print `$0.000` — a number this window made up, about a provider that stated
// something else. That is the whole of it: the collision with a stated zero
// that first suggested this branch does not exist, because no state of `Price`
// renders as `$0.000` any more.
//
// The threshold is deliberately wider than the rounding it guards: `toFixed(3)`
// only reaches `0.000` below `0.0005`, and in `[0.0005, 0.001)` the plain
// rendering would print `$0.001`, which is not a lie. "Under $0.001 per million
// tokens" is true across the whole band, so the extra width costs a person
// nothing and saves this constant from being two numbers that have to agree.
const SMALLEST_SHOWN_PER_MILLION = 0.001;

const statedPrice = (amount) => {
  if (amount === 0) {
    return ZERO_PRICE;
  }
  const perMillion = amount * 1e6;
  return perMillion < SMALLEST_SHOWN_PER_MILLION
    ? `under $${SMALLEST_SHOWN_PER_MILLION.toFixed(3)} per million tokens`
    : `$${perMillion.toFixed(3)} per million tokens`;
};

export const PRICE_TEXT = {
  known: (p) => statedPrice(p.amount),
  // Not "free", and not a zero. The provider said nothing.
  notStated: () => "price unknown",
  // `raw` is provider text, capped to 64 bytes on the Rust side and reaching
  // the DOM through `textContent` only, never as markup — the same rule
  // `limitNotUnderstood` states below.
  //
  // Quoted for the reason `unreadable` gives, and with a sharper case than
  // either of its neighbours: `NaN` is pinned in Rust as a value that reaches
  // this arm (`a_price_that_is_not_a_finite_number_is_not_a_price`), and `NaN`
  // is also what `render.test.js` searches a label for when it asks whether
  // this window invented a number. Unquoted, the provider's text and this
  // window's own marker for a defect are the same word in the same line.
  notAPrice: (p) => `the provider stated "${p.raw}" per token, which is not a price`,
  // Quoted, because `raw` here can be a word that is itself a claim about the
  // price — `"free"` is the measured one — and unquoted it becomes the last
  // word of the label, where it reads as this window's own verdict rather than
  // as the provider's text this build could not parse. The same quoting is on
  // `INPUT_LIMIT_TEXT.notUnderstood`, whose `raw` can just as easily be
  // `unlimited`, and on `REFUSAL_TEXT.limitNotUnderstood`, which a refused
  // model renders in the same line as the limit clause.
  unreadable: (p) => `price stated in a shape this build cannot read ("${p.raw}")`,
};

const priceText = (p) =>
  (PRICE_TEXT[p?.kind] ?? (() => "the price is in a state this build does not know"))(p);

// One phrase per `InputLimit`. "The provider stated no input limit" and "the
// provider stated one this build cannot read" are opposite statements about the
// provider, and for rerank and chat they used to arrive here as the same
// `null`: the refusals that carry the distinction are the embedding role's, so
// the other two roles rendered both as `input ?`. Fixing that is what put a
// union on this field (I4).
export const INPUT_LIMIT_TEXT = {
  known: (l) => `input ${l.tokens}`,
  notStated: () => "input limit not stated",
  // Quoted for the reason `PRICE_TEXT.unreadable` gives: this `raw` can be a
  // word that reads as a statement about the limit — `unlimited` — and the
  // quotes are what keep it the provider's text rather than this window's.
  notUnderstood: (l) => `input limit in a shape this build cannot read ("${l.raw}")`,
};

const inputLimitText = (l) =>
  (INPUT_LIMIT_TEXT[l?.kind] ?? (() => "the input limit is in a state this build does not know"))(l);

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
  // Quoted for the reason `PRICE_TEXT.unreadable` gives, and because a refused
  // model shows this clause **beside** `INPUT_LIMIT_TEXT.notUnderstood` in one
  // line: the same unparsed value rendered two ways in one label reads as two
  // different values.
  limitNotUnderstood: (r) =>
    `the provider stated an input limit in a shape this build does not understand ("${r.raw}")`,
  noStatedOutputModalities: () => "the provider did not say whether this model writes text",
  noTextOutput: () => "this model does not write text",
};

const refusalText = (refusal) =>
  `unavailable: ${(REFUSAL_TEXT[refusal.kind] ?? (() => "this build did not recognise the reason"))(refusal)}`;

export const modelOptionLabel = (entry) => {
  const head = `${entry.id} — ${priceText(entry.price)}, ${inputLimitText(entry.inputLimit)}`;
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
