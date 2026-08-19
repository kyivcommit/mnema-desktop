// The webview half of the walking skeleton. It draws what the core reports
// and decides nothing: no state here outlives a reload, because the core
// owns it. `render.js` is where every sentence is actually built — pure
// functions, importable and tested (`render.test.js`) without a browser —
// and everything below is DOM: elements, listeners, `invoke`.

import {
  endingSentence,
  searchResultItems,
  toggleState,
  textArmSentence,
  contentArmSentence,
  ROLES,
  indexNotAsked,
  indexOpened,
  indexOpenFailed,
  listNotAsked,
  listWasRead,
  listFailed,
  selectId,
  disclosureSentence,
  asSentence,
  keyStoreNote,
  keyStateSentence,
  keyNotSavedSentence,
  indexStateSentence,
  embeddingProgressText,
  adoptedModelSentence,
  modelOptionLabel,
  keyAcceptedSentence,
  catalogueSentence,
  roleRecordedSentence,
  recordedNoteSentence,
  listNotReadSentence,
  keyRemovedSentence,
  keyNotRemovedSentence,
  keyNotAsked,
  emptyFieldSentence,
  keyFieldPlaceholder,
  keySubmitText,
  embedProgressLine,
  embedEndingSentence,
  embedNotStartedSentence,
  embeddingModelNotRecordedSentence,
  roleNotRecordedSentence,
  KEEP_EXISTING_VECTORS,
  DISCARD_EXISTING_VECTORS,
  discardOffer,
  changeToConfirm,
  discardVectorsLabel,
  discardVectorsNote,
  barState,
  BAR_RUNNING,
  BAR_STOPPED,
  restatedEnding,
} from "./render.js";

const { invoke, Channel } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;

const el = (id) => document.getElementById(id);
const results = el("results");

// ─────────────────────────────────────────────────────────────────────────────
// Round 3: one readiness barrier for `#search-submit`
// (`docs/private/sdd/2026-08-14-search-query-and-fusion/
// ui-readiness-barrier-spec.md`, closes Codex round-3 findings 2, 3 and 9).
//
// Round 2 disabled `#search-submit` from inside the two arm handlers only,
// each handler writing the property itself. Round 3 found the key form, both
// model pickers and the initial load still pressable — and the arm
// handlers' own `disabled = true` quietly undone by an unrelated redraw
// (`drawArmState`, below). Both are the same mistake: a control this window
// has narrowed is not any redraw's to widen back except the one that
// narrowed it.
//
// `pendingConfigWrites` is a count, not a flag (§3.2): two overlapping
// writes must not have the first to settle re-open anything while the
// second is still out. `authoritativeStateRead` is a second, independent
// reason to stay closed (§3.3): at load there has been no write and no read
// either, and `index.html` now starts `#search-submit` `disabled` for
// exactly that window — this flag is what lets the barrier open, once, the
// first time `drawSettings` actually draws.
let pendingConfigWrites = 0;
let authoritativeStateRead = false;

// The one place that answers "may a search be submitted now" (§3.1) — every
// other line in this file that used to write `#search-submit`'s `disabled`
// itself now reports a fact to one of the two variables above and calls this
// instead.
const syncSearchGate = () => {
  el("search-submit").disabled = !authoritativeStateRead || pendingConfigWrites > 0;
};
// Forces the closed reading immediately, rather than waiting for the first
// caller that happens to touch the barrier. The markup (`index.html`)
// already starts `#search-submit` `disabled` for a real browser; this line
// is what gives the same window to anything — this file's own test harness
// included — that models the element without parsing the markup's own
// attributes.
syncSearchGate();

// Wraps a config-mutating handler's **whole** body — its `invoke` and
// whatever redraw follows it, not the `invoke` alone. The disclosure this
// barrier protects (`#disclosure`, drawn by `drawSettings`) is only as
// current as the last redraw, so the gate must stay shut through that
// redraw too: reopening the instant the write's own `invoke` resolves would
// let a search leave on the strength of a promise the screen has not caught
// up to yet — F9's exact defect, one step later. Every config-mutating
// `invoke` in this file goes through this (or, for the two arm handlers,
// through the same two variables directly — see the note above their
// listeners); `main.test.js`'s site test checks that nothing new does not.
const withSearchGated = async (body) => {
  // U-2 (adversarial pass, F-A3): both statements are inside `try` now, not
  // only `body()`. `syncSearchGate()` dereferences `el("search-submit")`; if
  // that throws, the increment must still be visible to `finally`, or the
  // counter never comes back down and search is shut for the rest of the
  // session. Pinned by "a throw inside syncSearchGate's own call does not
  // leak the gate shut for later writes".
  try {
    pendingConfigWrites += 1;
    syncSearchGate();
    return await body();
  } finally {
    pendingConfigWrites -= 1;
    syncSearchGate();
  }
};

// Every write to `#job-status`, and the count of them.
//
// ⚠️ **The question a late write has to ask is "is my line still the newest
// thing anybody put here", and only this can answer it.** Three orderings have
// now been measured against three different answers to that question, and the
// first two were both wrong:
//
//   - `jobRunning` (`3b18859`) lags a press by one IPC round trip in both
//     directions, so it let a stale ending paint over a live run *and*
//     permanently withheld one after a run that ended before its own `invoke`
//     resolved;
//   - a per-press generation (`518e499`) fixed both of those and broke a third:
//     Embed stays enabled during the press's own round trip, so an ordinary
//     double-click produces a refusal that claims a generation the running job
//     never used — and that job's ending is then suppressed for good.
//
// A refusal is not the only thing that can write here, and a press is not the
// only thing that can invalidate a restatement. What invalidates it is somebody
// having written to this line since — which is this number, and nothing else.
// It is why the seam exists rather than fourteen assignments: a writer that
// bypassed it would be invisible, and the next late write would paint over it.
//
// Returns the count at the moment of the write, for a caller that intends to
// come back and revise its own line.
let statusWrites = 0;
const sayJobStatus = (text) => {
  statusWrites += 1;
  el("job-status").textContent = text;
  return statusWrites;
};

// What `open_index` answered, kept because nothing on the core side can carry
// it. `UnreadableCause::NotOpen` is one value over two situations — the window
// has not asked for an index yet, and an open that failed and left none, since
// `AppState::db` is `None` in both — and this variable is the only place that
// difference exists. Without it, a permanent wall (an index written by a newer
// Mnema, which never opens) draws exactly like an ordinary cold start, and
// somebody waits out a state that will not change.
//
// `notAsked` is not reachable while `open_index` is awaited above the settings
// section, as it is today. It is a state of this window all the same, and the
// table that renders it (`INDEX_OPENING_TEXT`) has an arm for it so that
// reordering this file cannot silently produce an unhandled one.
//
// The three values come from constructors in `render.js` rather than from
// string literals written here. Literals were the one place in this window with
// no guard at all: `"opened"` mistyped as `"opend"` falls through
// `UNREADABLE_CAUSE_TEXT.notOpen`'s fallback without reddening anything, and
// the state it silences is exactly the one the order asked to be made visible.
// A named import that does not exist fails when the module loads.
let indexOpening = indexNotAsked();

// Opening the index is the first thing that happens, and its failure is
// something the user has to be able to read — which is why the window opens
// before the database does.
try {
  const info = await invoke("open_index");
  el("index-status").textContent = `index ready at ${info.path} (schema v${info.schemaVersion})`;
  indexOpening = indexOpened();
} catch (error) {
  el("index-status").textContent = `the index could not be opened: ${error}`;
  indexOpening = indexOpenFailed(error);
}

// `null` until `pick` answers with a real one. Kept apart from `jobRunning`
// below because the two gate "Index it" for different reasons — one because
// nothing has been chosen yet, the other because something is already
// running — and conflating them would make a reload (which loses this, but
// not necessarily a running job) look identical to "no folder chosen" for
// the wrong reason.
let watchedRootId = null;
let jobRunning = false;

// How many times a press has claimed the job area — the bar, the Cancel button
// and `#job-status`.
//
// ⚠️ **It exists because `jobRunning` cannot answer "is what I am about to draw
// still the newest thing here", and the review of `3b18859` measured both ways
// it gets that wrong.** Nothing sets that flag synchronously: both presses set
// it *after* their await, and the comment above `followUntilIdle` states the
// enabling fact in this file's own words — an ending can arrive before `invoke`
// resolves. So the flag reads `false` while a newer run is already reporting
// progress, and reads `true` for a run that ended before its own `invoke` came
// back. A late draw asking it gets a wrong answer in one direction or the other.
//
// This is incremented by the press itself, **before** the await, so it never
// lags. Every consumer compares a number it captured against this one; none of
// them asks whether a job is running, which is a different question with a
// different answer.
//
// **Not rolled back by a refused press, deliberately.** A refusal writes its own
// sentence to `#job-status`, and that sentence is newer than whatever an earlier
// run was about to restate there — so an earlier restatement must stay
// suppressed, exactly as it would behind a run that really started.
let jobGeneration = 0;

const syncButtons = () => {
  el("walk").disabled = jobRunning || watchedRootId === null;
  // Not gated on `watchedRootId`: embedding works off what the index already
  // holds, so a reload that lost the chosen folder must not take it with it.
  // One flag for the whole application, so the slot is the only thing that
  // disables this.
  el("embed").disabled = jobRunning;
  el("cancel").disabled = !jobRunning;
};

el("pick").addEventListener("click", async () => {
  // The native picker, not a typed path: `dialog:allow-open`
  // (`src-tauri/capabilities/default.json`) is what makes this reachable at
  // all, and D48's own trap — the ACL classifies by origin, and that origin
  // differs between Windows and macOS — is why that capability's own test
  // lives beside every other command's, in `src-tauri/tests/commands.rs`,
  // using the same `local_origin()` rather than a literal.
  let path;
  try {
    path = await open({ directory: true });
  } catch (error) {
    el("folder").textContent = `could not open the folder picker: ${error}`;
    return;
  }
  // `null` means the person closed the dialog without choosing anything —
  // not an error, and not a reason to forget whatever was chosen before.
  if (path === null) {
    return;
  }

  try {
    watchedRootId = await invoke("add_watched_folder", { path });
    el("folder").textContent = path;
    // Belongs to whichever root was walked last; a folder just chosen has no
    // walk behind it yet, so a skip line from the previous one would be read
    // as this one's.
    el("skips").textContent = "";
  } catch (error) {
    el("folder").textContent = `could not watch ${path}: ${error}`;
    watchedRootId = null;
  }
  syncButtons();
});

// Whether the channel has already said how the job ended, and said it better
// than the poller below could.
let endingDescribed = false;

// The core is the authority on whether a job is running; the channel is an
// accelerator, not the only route to the truth.
//
// It cannot be the only route for two reasons. A page that reloaded mid-job has
// no channel at all — the ending goes to the page that started the job, which no
// longer exists. And even on the page that did start it, the ending can arrive
// before `invoke` resolves, which no amount of ordering inside the handler
// fixes: a job over many files cannot do that, a job over an empty folder
// finishes in less than one IPC round trip.
const followUntilIdle = async () => {
  while ((await invoke("job_status")).running) {
    await new Promise((wake) => setTimeout(wake, 500));
  }
  jobRunning = false;
  syncButtons();
  if (!endingDescribed) {
    // `job_status` is a bool, not an `Ended` — this path has no channel to
    // read `reason`, `complete`, `indexed`, `unchanged` or `frozen` from at
    // all (a page reloaded mid-job, or one that opened after the job it is
    // polling started). "the job has finished" was true and said nothing
    // else, which reads as "finished cleanly" to anyone who does not already
    // know the difference — the one thing this page can actually say is that
    // it does not know.
    sayJobStatus(
      "the job is no longer running, but this page has no channel to it and does not " +
        "know how it ended — whether it finished cleanly, or something was left unreconciled",
    );
    // And the bar says the same. This path has no `Ended` to hand `barState`,
    // so it takes the answer that function gives an ending it does not
    // recognise, and for the same reason: "finished" is a claim, and a page
    // with no channel has established nothing. Left alone the bar sits wherever
    // the last report put it, in the colour of a run still going — the defect
    // itself, on the one path that cannot even say how the job ended.
    el("bar").dataset.state = BAR_STOPPED;
  }
  // This is the only route the page has after a reload mid-job: the channel
  // belongs to the page that started the job, so the two handlers that redraw
  // the settings when an ending arrives are both out of reach here. Without
  // this line a page that reloaded while an embedding run was finishing would
  // go on showing the counts it read at load, for as long as it stayed open,
  // with the run's own numbers nowhere on screen to contradict them.
  //
  // `refreshSettings` is declared below and is a `const`; see the note in the
  // walk's own ending handler for why this is not a use before initialisation.
  // The one path that reaches this during module evaluation — the `job_status`
  // check near the top, which calls `follow()` without awaiting it — yields at
  // its own `invoke` and cannot resume before the module has run past that
  // declaration.
  refreshSettings();
};

// Never leaves the buttons disabled. If the core cannot be reached, Cancel is
// simply left disabled (nothing is known to be running) rather than the page
// having nothing left to press.
const follow = () =>
  followUntilIdle().catch((error) => {
    jobRunning = false;
    syncButtons();
    sayJobStatus(`lost track of the job: ${error}`);
  });

try {
  const { running } = await invoke("job_status");
  jobRunning = running;
  syncButtons();
  if (running) {
    sayJobStatus("a job started before this page loaded is still running");
    follow();
  }
} catch (error) {
  jobRunning = false;
  syncButtons();
  sayJobStatus(`${error}`);
}

el("walk").addEventListener("click", async () => {
  // A channel, not an event listener: events are documented as unsuited to
  // throughput and may arrive out of order, and a bar that jumps backwards
  // reads as a broken application. Within one channel Tauri guarantees the
  // order itself, so nothing here has to reassemble anything.
  const onProgress = new Channel();
  onProgress.onmessage = ({ event, data }) => {
    if (event === "progress") {
      const { done, total, skipped, secondsLeft } = data;
      el("bar").max = total;
      el("bar").value = done;
      // Set on every report, not only at the start: this is the one place that
      // is certain a job is moving, and a bar left grey by the previous run's
      // ending would say the opposite for as long as it took the state to be
      // put right somewhere else.
      el("bar").dataset.state = BAR_RUNNING;
      const eta = secondsLeft === null ? "estimating…" : `${secondsLeft}s left`;
      sayJobStatus(`${done} of ${total}, ${skipped} skipped — ${eta}`);
      return;
    }

    // The core says how the job ended; the page does not decide. Cancelling is
    // a request, and between the request and the stop there is real work still
    // finishing — a page that printed "cancelled" on the click would be stating
    // as fact something it had not been told.
    //
    // This arrives however the job ended, a panic included, which is what keeps
    // "Index it" from being disabled forever.
    const ending = data;
    el("bar").max = ending.total;
    el("bar").value = ending.done;
    // A walk gets this as much as an embedding run does: one bar, one Cancel,
    // and a walk somebody stopped leaves the identical half-filled blue picture
    // the acceptance run of 2026-08-13 waited in front of. The decision is
    // `barState`'s, in `render.js`, where it can be tested; both handlers ask
    // it rather than each deciding for its own job.
    el("bar").dataset.state = barState(ending);

    sayJobStatus(endingSentence(ending));
    endingDescribed = true;
    jobRunning = false;
    syncButtons();

    if (watchedRootId !== null) {
      renderSkips(watchedRootId);
    }
    // A walk changes how many pieces the index holds, which is the denominator
    // of the settings line further down — left alone, that line goes on
    // describing the index as it was before this run. Redrawn from the
    // database rather than adjusted from the ending, so the screen cannot come
    // to hold a number the index does not.
    //
    // `refreshSettings` is declared below this handler and is a `const`, so it
    // is worth saying why this is not a use before initialisation: the handler
    // runs on a message from a job, a job is started by a click on a button
    // that stays disabled until a folder has been chosen through a native
    // dialog, and the whole module — this file's last line included — has
    // finished evaluating long before any of that.
    refreshSettings();
  };

  if (watchedRootId === null) {
    sayJobStatus("choose a folder above before indexing");
    return;
  }

  // The bar comes back to life on the press rather than on the first report,
  // and **before** the await rather than after it. Both halves are the point.
  // A run's first report can be a batch away, and a bar still wearing the last
  // run's ended colour for those seconds says the press did nothing — the same
  // defect, inverted. And a job that ends before `invoke` resolves has its
  // ending drawn during that await, so a line after it would put "running" back
  // over the picture the ending had just drawn, with nothing left to correct it.
  //
  // Restored on a refusal, because a press that started nothing must not change
  // what the bar says — least of all on the commonest refusal of all, where the
  // bar belongs to the job that is already running.
  const barWas = el("bar").dataset.state ?? "";
  el("bar").dataset.state = BAR_RUNNING;
  // This press claims the job area too, and it must, for the same reason the
  // embedding press does: an embedding run's restatement is still in flight
  // when somebody starts a walk, and it must not land on the walk's line.
  jobGeneration += 1;

  try {
    endingDescribed = false;
    await invoke("start_walk_job", { rootId: watchedRootId, onProgress });
    jobRunning = true;
    // Even here, where this page owns the channel. `syncButtons()` runs after
    // the await, so an ending that arrived first has already been overwritten
    // by the line above, and only the core can put it right.
    syncButtons();
    follow();
  } catch (error) {
    // Refused — most likely because a job is already running. Nothing started,
    // so the buttons must not move, and neither must the bar.
    el("bar").dataset.state = barWas;
    sayJobStatus(`${error}`);
    // The press bumped the generation before the await, and any settings draw
    // that resolved inside it therefore suppressed its refusal clause on the
    // strength of a job that never started. One read puts that right, on a path
    // that has already failed and is paying for a round trip anyway. The
    // embedding press's own refusal does the same.
    refreshSettings();
  }
});

el("cancel").addEventListener("click", async () => {
  // Only the button is disabled here. "Index it" comes back when the job says
  // it has ended, not when the user asks it to.
  el("cancel").disabled = true;
  sayJobStatus("stopping…");
  try {
    await invoke("cancel_job");
  } catch (error) {
    // The request never reached the core, so whatever is running is still
    // running. Left disabled forever, and silent about it, "stopping…"
    // would read as true when it is not — this is the honest alternative:
    // the job is presumably still going, and the button is worth pressing
    // again.
    sayJobStatus(`could not ask the job to stop: ${error}`);
    el("cancel").disabled = false;
  }
});

/// Reads the skip journal for `rootId` and renders it next to the job status.
///
/// Called after every ending, not only a clean one: a job that failed or was
/// cancelled partway through may still have skipped files before it stopped,
/// and those are as real as any other run's.
async function renderSkips(rootId) {
  try {
    const skips = await invoke("skips", { rootId });
    el("skips").textContent = skips.length
      ? skips
          .map((s) => {
            // `pageNo` is `null` for a whole-file skip and set for one page
            // inside an otherwise readable document — two different shapes a
            // single sentence has to say apart.
            const where = s.pageNo === null ? s.relativePath : `${s.relativePath} (page ${s.pageNo})`;
            return `${where}: ${s.reason}`;
          })
          .join("; ")
      : "";
  } catch (error) {
    el("skips").textContent = `could not read the skip log: ${error}`;
  }
}

// The window draws; it does not decide. Every number here came from a
// command — `searchResultItems` and `hitLocation` (`render.js`) decide what
// the list is made of, this only turns that into elements. `search` answers a
// `SearchAnswer`, not a bare hit list — `hits` is what `searchResultItems`
// takes, and `text`/`content` are each arm's own report.
async function search(query) {
  return invoke("search", { query });
}

// Round 3, F3: two searches can now be genuinely concurrent — D111 hoisted
// the embedding call out of the index mutex, so two paid provider requests
// can be in flight at once. Without a generation of its own, an older
// completion settling after a newer one used to overwrite it silently on
// success, and clear the *newer* search's arm-state lines on the older
// one's failure. `searchAsked`/`searchDrawn` are the same idiom as
// `settingsAsked`/`settingsDrawn` below: issue numbers, not a flag, because
// two searches can be in flight and can come back in either order, and both
// the success and the failure render paths have to ask the same question.
let searchAsked = 0;
let searchDrawn = 0;

el("search-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = el("query").value;
  searchAsked += 1;
  const issue = searchAsked;
  try {
    const answer = await search(query);
    // An answer older than one already on screen has nothing to add and
    // could only take something away.
    if (issue <= searchDrawn) {
      return;
    }
    searchDrawn = issue;
    results.replaceChildren(...searchResultItems(answer.hits).map((item) => {
      const li = document.createElement("li");
      if (item.kind === "empty") {
        li.className = "muted";
        li.textContent = item.text;
        return li;
      }
      const where = document.createElement("p");
      where.className = "muted";
      where.textContent = item.where;
      const text = document.createElement("p");
      text.textContent = item.text;
      li.append(where, text);
      return li;
    }));
    el("text-arm-state").textContent = textArmSentence(answer.text);
    el("content-arm-state").textContent = contentArmSentence(answer.content);
  } catch (error) {
    // The same guard on the failure path: an older rejection is exactly the
    // path that used to erase a newer answer, clearing both arm-state lines
    // it had never touched.
    if (issue <= searchDrawn) {
      return;
    }
    searchDrawn = issue;
    const li = document.createElement("li");
    li.textContent = `search failed: ${error}`;
    results.replaceChildren(li);
    // Left alone, a failed search keeps showing the previous successful
    // search's arm report — a real number about a different attempt,
    // indistinguishable from a current one.
    el("text-arm-state").textContent = "";
    el("content-arm-state").textContent = "";
  }
});

// Whether each arm can run at all, cached from the last `model_settings` draw
// so a checkbox's own `change` can redraw `toggleState` without asking again.
let armKeyPresent = false;
let armModelChosen = false;
// True only until the first `model_settings` draw — `drawSettings` below
// overwrites both from `read.searchTextArm`/`read.searchContentArm` the
// moment an index read answers, so this default is never what a person
// with a saved choice actually sees.
let savedTextArm = true;
let savedContentArm = true;

// Whether a `set_search_arms` write is currently outstanding — round 2's
// `armWriteGeneration` counts *presses*, which is right for "does a read
// issued before this write predate it" (below) but wrong for "is a write
// still out right now": a press that has already settled still holds a
// generation. This is incremented before the `invoke` and decremented once
// it settles, success or refusal, so it is exactly zero when nothing this
// window started is still waiting to hear back.
let armWritesInFlight = 0;

// Returns the `toggleState` it drew from, so a caller that needs the same
// facts reads them off this one call instead of asking `toggleState` again
// with its own copy of these four fields.
//
// Round 3, F2: this used to write `toggleState`'s `disabled` unconditionally,
// so a `model_settings` redraw landing while a `set_search_arms` write was
// still in flight re-enabled both checkboxes out from under it — the pending
// write's own `disabled = true` (in the handlers below) quietly undone, and
// a second click then sent a *second*, overlapping write. §3.4's fix: the
// value actually written is `toggleState`'s disabled **or** the barrier's —
// a redraw may only ever add a disable here, never remove one a pending
// write still owns. This is the one function that writes these two
// properties, and it is the only place that fix belongs.
const drawArmState = () => {
  const state = toggleState({
    savedText: savedTextArm,
    savedContent: savedContentArm,
    keyPresent: armKeyPresent,
    modelChosen: armModelChosen,
  });
  el("arm-text").checked = state.text.checked;
  el("arm-text").disabled = state.text.disabled || armWritesInFlight > 0;
  el("arm-text-note").textContent = state.text.note;
  el("arm-content").checked = state.content.checked;
  el("arm-content").disabled = state.content.disabled || armWritesInFlight > 0;
  el("arm-content-note").textContent = state.content.note;
  return state;
};

// The disclosure sentence's `contentArmRuns` and `textRuns` are this same
// draw's `content.checked`/`text.checked`, not a second read of the toggle —
// `drawArmState`'s return, not a fresh `toggleState` call, is what makes
// that one fact instead of two that could disagree. Called everywhere
// `drawArmState` used to be called alone: a checkbox `change` moves the
// toggle without a fresh `model_settings` round trip, and the sentence
// promising what a search sends must move with it, in the same paint.
// Pinned per arm by `switching the content arm off updates the disclosure
// sentence too` and `switching the text arm off with an absent key updates
// the disclosure sentence too`.
const drawArmStateAndDisclosure = () => {
  const { text, content } = drawArmState();
  el("disclosure").textContent = asSentence(
    disclosureSentence(keyState, { contentArmRuns: content.checked, textRuns: text.checked }),
  );
};

// Whether an arm write (`set_search_arms`) has started, as a generation that
// only grows — the same idiom as `jobGeneration`/`askedAt` below. A read
// issued before a write started must not restore the checkbox once that
// write lands, even after the write itself has already settled, which a
// simple in-flight flag could not tell. Pinned by `a settings read issued
// before an arm write does not revert it once the write has landed`.
let armWriteGeneration = 0;

// Both handlers write optimistically and undo on refusal — the same
// catch-and-say convention `#search-form`'s own listener uses above,
// applied to a control this window must not leave believing a choice was
// saved when `set_search_arms` never returned. Both also claim the search
// form's submit for the length of the write, the same way they already
// claim each other's checkbox — through `withSearchGated`, the same barrier
// every other Class A handler in this file uses, not by writing
// `#search-submit`'s `disabled` themselves (§3.1). `armWritesInFlight` is
// separate from `pendingConfigWrites`, and is decremented *inside* the
// wrapped body, before `drawArmStateAndDisclosure()` — a moment earlier than
// `withSearchGated`'s own bookkeeping settles — because that draw is what
// `armWritesInFlight` gates (§3.4), and it has to see this write's own
// count already back down before it decides whether either checkbox may
// re-enable. Pinned per handler by `the search form's submit is disabled
// while an arm write is in flight, and re-enabled once it settles` and its
// content-arm counterpart of the same name.
el("arm-text").addEventListener("change", async () => {
  const previous = savedTextArm;
  savedTextArm = el("arm-text").checked;
  el("arm-text").disabled = true;
  el("arm-content").disabled = true;
  armWriteGeneration += 1;
  armWritesInFlight += 1;
  await withSearchGated(async () => {
    try {
      await invoke("set_search_arms", { text: savedTextArm, content: savedContentArm });
    } catch (error) {
      savedTextArm = previous;
      armWritesInFlight -= 1;
      drawArmStateAndDisclosure();
      el("arm-text-note").textContent = `the choice was not saved: ${error}`;
      return;
    }
    armWritesInFlight -= 1;
    drawArmStateAndDisclosure();
  });
});
el("arm-content").addEventListener("change", async () => {
  const previous = savedContentArm;
  savedContentArm = el("arm-content").checked;
  el("arm-text").disabled = true;
  el("arm-content").disabled = true;
  armWriteGeneration += 1;
  armWritesInFlight += 1;
  await withSearchGated(async () => {
    try {
      await invoke("set_search_arms", { text: savedTextArm, content: savedContentArm });
    } catch (error) {
      savedContentArm = previous;
      armWritesInFlight -= 1;
      drawArmStateAndDisclosure();
      el("arm-content-note").textContent = `the choice was not saved: ${error}`;
      return;
    }
    armWritesInFlight -= 1;
    drawArmStateAndDisclosure();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Model configuration.
//
// Every sentence below comes from `render.js`, which is where the states are
// told apart and where `render.test.js` can reach them. This half is elements
// and listeners, and its own job is to keep two facts out of one element.
//
// **The claim above was false** until the whole-branch review (I2) enumerated
// the sentences that broke it — listed rather than counted, for the reason the
// `render.js` header gives about its own union list: `the model list could not
// be read`, `the key was removed`, `the key could not be removed`, `the
// embedding model was not recorded`, and the one for rerank and chat.
//
// The exception was not even consistent: for rerank and chat the *success*
// sentence was in `render.js` while its *failure* sentence was a literal here,
// so a reader could not derive the rule from the code and the comment stated
// the wrong one. All of them moved rather than the sentence being softened,
// because the rule is what keeps a sentence inside the surface
// `render.test.js` can reach — and the sentence furthest outside it was `the
// key was removed`, which was also I1: a sentence about an event that had not
// happened.
//
// It is checked rather than promised now: `every sentence in the model
// configuration block comes from render.js` in `render.test.js` reads this file
// and fails on a string or template literal assigned to a `textContent`,
// `innerText` or `innerHTML` below this line, by `=` or by `+=`. The block
// above it is the walking skeleton's and is deliberately not covered — that
// claim would be a different one, and this comment does not make it.
//
// **What that check does not reach**, said here because this is the file
// somebody writing a sentence is looking at: a literal bound to a name first,
// or sitting second in a concatenation or a ternary, is invisible to it —
// telling those from a call needs a parser rather than a regexp, and the test
// says the same from its own side with the shapes measured one at a time. The
// rule above is a rule to keep, not a fence that will stop you.

// What `provider_models` has answered for this role, in three states rather
// than two. `false` used to mean both "could not be read" and "has not been
// asked for yet", and the listeners below are registered *before* the three
// round trips finish — so a key submitted in that window drew a sentence about
// a failure that had not happened. It is the same shape as `indexOpening`
// twenty lines up, and the constructors come from `render.js` for the same
// reason: a mistyped literal here would fall through a table's fallback in
// silence.
const listState = Object.fromEntries(ROLES.map((role) => [role, listNotAsked()]));

// What the credential store last answered, kept because the submit handler has
// to know it and `model_settings` is the only thing that asks. `keyNotAsked()`
// rather than `null` for the reason `listState` above is a three-valued union:
// this listener is registered before the first `refreshSettings()` resolves, so
// a press in that window must not fall through a fallback and report an answer
// nobody had asked for.
let keyState = keyNotAsked();

// The model a refused change was trying to reach, or `null`. It is the only
// thing that can make the discard button do anything, and it is set nowhere
// except where a change was actually refused — so a button left on screen by a
// stale draw cannot act on a model this window has stopped showing.
let refusedChange = null;

// Every option carries its own label, refused ones disabled. Refused rather
// than absent: a model the provider lists and this window hides sends the user
// looking for a fault here.
const fillRole = async (role) => {
  const select = el(selectId(role));
  select.replaceChildren();
  try {
    const catalogue = await invoke("provider_models", { role });
    for (const entry of catalogue.entries) {
      const option = document.createElement("option");
      option.value = entry.id;
      // `textContent`, never markup: the label can carry provider text
      // (`Refusal::LimitNotUnderstood`'s `raw`), which is capped upstream but
      // is still untrusted.
      option.textContent = modelOptionLabel(entry);
      option.disabled = entry.refusal !== null && entry.refusal !== undefined;
      select.append(option);
    }
    // Both numbers, and both zeroes. A list quietly shorter than the provider's
    // is the failure Task 1 spent three fix rounds removing; a well-formed
    // empty answer drawn as nothing at all is the same failure with no records
    // to point at, and `provider_models` keeps `unreadable` on the wire so this
    // seam can tell those apart.
    el(`${selectId(role)}-unreadable`).textContent = catalogueSentence(catalogue);
    // The catalogue travels with the state. "The call succeeded" does not
    // establish that a model missing from the picker was withdrawn — a record
    // this build could not decode still names itself — and only the catalogue
    // can tell those apart.
    listState[role] = listWasRead(catalogue);
  } catch (error) {
    // Not into `key-status`: this endpoint needs no key (`provider_models` is
    // called without one), so a network failure here has nothing to do with
    // the credential store and must not be read as though it had.
    listState[role] = listFailed();
    el(`${selectId(role)}-unreadable`).textContent = listNotReadSentence(error);
  }
};

// What the index records, shown in the picker — and said in words when the
// picker cannot show it. Assigning a `value` no option carries leaves the
// select blank, which is a recorded configuration disappearing quietly; whether
// that blank is worth a sentence, and which sentence, is
// `recordedNoteSentence`'s decision, because four different facts reach it.
const showRecorded = (role, recorded) => {
  const select = el(selectId(role));
  select.value = recorded ?? "";
  el(`${selectId(role)}-missing`).textContent = recordedNoteSentence({
    recorded,
    list: listState[role],
    // Asked of the element after the assignment rather than of the catalogue:
    // this is what the person is actually looking at.
    listed: select.value === recorded,
  });
};

// `askedAt` is `jobGeneration` and `armAskedAt` is `armWriteGeneration`, both
// as they stood when this draw's `model_settings` was **issued**, not when
// it came back.
const drawSettings = (settings, askedAt, armAskedAt) => {
  // ⚠️ **Whether a job has these counts, and why `jobRunning` alone is the
  // wrong question.** Review of `3b18859`, Important 2, measured rather than
  // argued: this function reads the flag when it runs, which is after its own
  // await, and the flag is set only after a press's await — so a run that
  // started inside this round trip and is already reporting progress is still
  // `false` here. The line then printed `none were refused by the provider`
  // beside a live run, which is the exact stale assertion the suppression was
  // written for.
  //
  // The generation cannot lag, because the press increments it before awaiting
  // anything. So the honest predicate is "a job is running, **or** a press has
  // claimed the slot since this read was issued" — and the second half is what
  // the flag could not say. A press that is then refused makes this briefly
  // over-cautious rather than wrong; both refusal handlers redraw for exactly
  // that reason.
  const aJobHasTheSlot = jobRunning || askedAt !== jobGeneration;
  keyState = settings.key;
  el("key-state").textContent = asSentence(keyStateSentence(settings.key));
  // The field and its button are drawn from the store's answer too. With a key
  // stored this window has just cleared the field, so an empty one is the
  // ordinary state and must not read as something missing.
  const placeholder = keyFieldPlaceholder(settings.key);
  el("key").placeholder = placeholder;
  // Sized from the text it is showing rather than from a width chosen once. The
  // acceptance run of 2026-08-11 found the longer of the two placeholders cut to
  // "leave empty to keep the sto" at the field's default width — and that is the
  // one sentence whose whole job is to say an empty field is fine here, so a
  // person who cannot finish reading it is back where the message started.
  el("key").size = placeholder.length;
  el("key-submit").textContent = keySubmitText(settings.key);
  // Empty on two of the three platforms and with no key stored, which is why it
  // is written every time rather than only when there is something to say: a
  // line that is set once and never cleared outlives the state it described.
  el("key-note").textContent = asSentence(keyStoreNote(settings.platform, settings.key));
  el("index-state").textContent = asSentence(indexStateSentence(settings.index, indexOpening));
  // `aJobHasTheSlot` crosses, because these counts were read at a moment and
  // that moment is in the past for the whole length of a run — and the line says
  // "none were refused" at zero, so what it holds while it is stale is an
  // assertion rather than a silence. `embeddingProgressText` is where the two
  // are told apart; the predicate is built at the top of this function, where
  // the measurement that made it a generation and not the flag is written down.
  el("embedding-progress").textContent = asSentence(
    embeddingProgressText(settings.index, aJobHasTheSlot),
  );
  // An index that could not be read says nothing about which models are
  // recorded, so the pickers show nothing chosen and `index-state` carries the
  // reason. Leaving the first option selected would have the window state a
  // configuration it has not read.
  // Redrawn from the settings on every pass rather than set once at the moment
  // of refusal: the number on that button is what the index says now, and a
  // number left over from an earlier draw is the one thing this control may not
  // show. Written every time for the same reason `key-note` is — a line set once
  // and never cleared outlives the state it described, and this one outliving it
  // is a button offering to delete embeddings that are already gone.
  // `aJobHasTheSlot` for the reason the line above it takes it: the counts these
  // were read at are moving while a job runs, and a button whose label names a
  // number that is changing is the same stale assertion, worn as a label. It
  // takes the same predicate rather than the raw flag because it is stale in the
  // same window and for the same reason — fixing one of the two and leaving its
  // neighbour is the half-fix this cycle keeps catching.
  const offer = discardOffer(refusedChange, settings.index, settings.key, aJobHasTheSlot);
  el("discard-vectors").hidden = offer === null;
  el("discard-vectors").textContent = discardVectorsLabel(offer);
  el("discard-vectors-note").textContent = discardVectorsNote(offer);
  const read = settings.index?.kind === "read" ? settings.index : null;
  showRecorded("embedding", read && read.embeddingModel);
  showRecorded("rerank", read && read.rerankModel);
  showRecorded("chat", read && read.chatModel);
  armKeyPresent = settings.key?.kind === "present";
  armModelChosen = Boolean(read && read.embeddingModel);
  // Only while `read` actually answers, and only while no arm write has
  // started since this read was issued: `armAskedAt !== armWriteGeneration`
  // means a write landed first, and overwriting here would revert the
  // checkbox this read knows nothing about. Pinned by `a settings read
  // issued before an arm write does not revert it once the write has
  // landed`.
  if (read && armAskedAt === armWriteGeneration) {
    savedTextArm = read.searchTextArm;
    savedContentArm = read.searchContentArm;
  }
  drawArmStateAndDisclosure();
  // §3.3: this is the first authoritative read landing, or the tenth — either
  // way, `syncSearchGate` is what turns that into `#search-submit` opening
  // (or staying shut, if a config write is still out). Set every time this
  // function actually draws, which per `refreshSettings`'s own guard is only
  // for the newest read issued.
  authoritativeStateRead = true;
  syncSearchGate();
};

// No `.catch()`, and that is deliberate. `model_settings` returns no `Result`
// at all: every state of the credential store and every state of the index is
// an answer, so this command has no rejection to catch. A blanket `.catch()`
// here would never fire, and would read as though the two `Unreadable` states
// were being handled — they are handled in `drawSettings`, by being drawn.
//
// It answers with what it drew. The embedding run's ending needs the same read
// this draws from — the index's own pair, to say beside the run's — and asking
// twice would put two reads taken at two instants in two lines of one window,
// which is the disagreement every other number on this screen is arranged to
// make impossible.
// How many reads have been issued, and the newest one that has been drawn.
//
// ⚠️ **Two reads can be in flight and can come back in the other order**, and
// then the older one is drawn last and wins. `main.test.js` produced it: a press
// refused inside an earlier run's read redraws correctly, and the earlier read
// then lands on top and puts the suppressed line back for a job that never
// started. It is not only mine to have caused — a run's ending and
// `followUntilIdle` both call this at the same moment, and have raced since the
// day the poller was written — but the refusal handlers below make two reads
// overlap deliberately, so it stops being theoretical here.
//
// A number rather than a timestamp: these are issue numbers, not instants, and
// nothing needs to know how long a read took.
let settingsAsked = 0;
let settingsDrawn = 0;

const refreshSettings = async () => {
  settingsAsked += 1;
  const issue = settingsAsked;
  // Same idea as `askedAt` just below, for an arm write instead of a job:
  // captured here so `drawSettings` can tell a write that started after this
  // point from one already running when this read was issued.
  const armAskedAt = armWriteGeneration;
  // Captured before the await, never after. This is the whole of what lets
  // `drawSettings` tell "no job has touched these counts" from "one started
  // while I was asking" — the distinction `jobRunning` cannot draw, because
  // nothing sets that flag until a press's own await has returned.
  const askedAt = jobGeneration;
  const settings = await invoke("model_settings");
  // A read older than one already on screen has nothing to add and can only
  // take something away. The answer is still returned, because the caller that
  // asked for it may have its own use and its own guard — the ending's
  // restatement does.
  if (issue > settingsDrawn) {
    settingsDrawn = issue;
    drawSettings(settings, askedAt, armAskedAt);
  }
  return settings;
};

el("key-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  // An empty field means two different things, and only this side knows which:
  // it clears the field after a successful save, so an empty field with a key
  // stored is somebody in a perfectly good state pressing a button. `set_key`'s
  // own guard is untouched and still refuses the empty string — this is a
  // distinction added in front of it, not a refusal taken over from it. A
  // `null` sentence is the one state where the command's answer is the right
  // one; see `EMPTY_FIELD_TEXT`, and note `unreadable` is not that state.
  if (el("key").value === "") {
    const instead = emptyFieldSentence(keyState);
    if (instead !== null) {
      el("key-status").textContent = asSentence(instead);
      return;
    }
  }
  // Class A (§2): a saved key changes what leaves the machine, and the
  // barrier stays shut through the trailing `refreshSettings()` too, not
  // only the `invoke` — F9's sequence is a key saved and a search sent
  // while `#disclosure` still reads the old promise, and that promise is
  // only put right by the redraw this call awaits.
  await withSearchGated(async () => {
    try {
      const status = await invoke("set_key", { key: el("key").value });
      el("key").value = "";
      el("key-status").textContent = asSentence(keyAcceptedSentence(status));
    } catch (error) {
      // Not "the key was not accepted": every reachable failure of `set_key`
      // except `Provider` decided nothing about the key. Said without a total,
      // because the total was wrong for one commit — this comment counted three
      // while its two neighbours, the table on `keyNotSavedSentence` and the test
      // above it, were updated. `keyNotSavedSentence` has the enumeration.
      el("key-status").textContent = asSentence(keyNotSavedSentence(error));
    }
    await refreshSettings();
  });
});

el("forget").addEventListener("click", async () => {
  // Class A (§2): removing the key changes what leaves the machine just as
  // saving one does — see the note on `key-form`'s own listener above.
  await withSearchGated(async () => {
    try {
      el("key-status").textContent = asSentence(keyRemovedSentence(await invoke("forget_key")));
    } catch (error) {
      // The key is still there. Saying "removed" because the button was pressed
      // would state as fact something the store refused to do — and the next
      // line of the window, redrawn from the store itself, would contradict it.
      el("key-status").textContent = asSentence(keyNotRemovedSentence(error));
    }
    await refreshSettings();
  });
});

// The embedding role is not the other two and does not share their handler.
// It answers `AdoptedModel` — a model, a width, a space, whether that space was
// minted and what it retired — while `set_rerank_model` and `set_chat_model`
// write a string and answer nothing. One handler for both would have to throw
// the adoption away to have something in common with the other two.
//
// Both presses that can record an embedding model go through this one function,
// differing only in `existingVectors`. Two copies would be two places for the
// destructive spelling to end up on the harmless press.
// Class A (§2): the embedding model decides whether questions leave the
// machine at all, and the discard-vectors confirmation reaches this too —
// `recordEmbeddingModel` is one function with two call sites, gated once
// here rather than at each press, or the second press would keep the hole
// F9 exists to close.
const recordEmbeddingModel = async (model, existingVectors) => {
  await withSearchGated(async () => {
    try {
      const adopted = await invoke("set_embedding_model", { model, existingVectors });
      // Cleared on success, so the button below cannot survive the change it was
      // offered for and act a second time on a model nobody is looking at.
      refusedChange = null;
      el("model-status").textContent = asSentence(adoptedModelSentence(adopted, indexOpening));
    } catch (error) {
      // The refusal already says how many vectors stand in the way; showing it
      // whole is better than a sentence of our own that says less.
      //
      // Set on every failure this window cannot attribute, and narrowed by
      // `discardOffer` rather than here. Not for tidiness: the refusal arrives as
      // a string, so this `catch` cannot tell "a space blocks the change" from
      // "you have entered no key" without matching on message text — the failure
      // mode `crate::error::Error`'s own header says that type exists to avoid.
      // What can be decided is decided from state, one line down, where the guards
      // and their gaps are written out.
      //
      // The one exception is the one this window *does* know from state: with a
      // job running, the refusal is the slot, and a slot refusal must leave
      // nothing to confirm — otherwise the run's own ending redraws the button
      // against a count that run has just made larger, and pressing it destroys
      // what it paid for. `changeToConfirm` is where that is written.
      refusedChange = changeToConfirm(model, jobRunning);
      el("model-status").textContent = asSentence(embeddingModelNotRecordedSentence(error));
    }
    await refreshSettings();
  });
};

el(selectId("embedding")).addEventListener("change", async (event) => {
  // Never the discarding value. A change nobody has been asked about must not
  // be able to delete anything, whatever this window later offers.
  await recordEmbeddingModel(event.target.value, KEEP_EXISTING_VECTORS);
});

// The confirmed half. It re-sends the model the refusal was about rather than
// whatever the picker is showing: `drawSettings` puts the *recorded* model back
// into the select, so by the time this button is on screen the select no longer
// holds the model the person chose.
el("discard-vectors").addEventListener("click", async () => {
  if (refusedChange === null) {
    return;
  }
  await recordEmbeddingModel(refusedChange, DISCARD_EXISTING_VECTORS);
});

// Starting the embedding pass.
//
// Its button sits in the job area at the top of the window, with "Index it" and
// the one Cancel, because there is one job slot and one bar. The listener is
// registered **here** for two reasons that both belong to this block: every
// sentence it writes comes from `render.js`, which is this block's rule and is
// checked by `render.test.js`; and it redraws the settings when the run is over,
// which needs `refreshSettings` to have been declared.
el("embed").addEventListener("click", async () => {
  // This press claims the job area, before anything is awaited. It is read by
  // the settings block, which has to know that a job may have taken the slot
  // while its own read was in flight — see `jobGeneration` and `aJobHasTheSlot`.
  //
  // The ending's restatement deliberately does **not** use it: a press is the
  // wrong event to count for "is my line still the newest thing here", because
  // a refused press claims a number the running job never used. That question
  // is `statusWrites`, at the seam.
  jobGeneration += 1;

  // A channel of its own, and a handler of its own. It is not the walk's with a
  // flag: `endingSentence` appends a clause about folder reconciliation, which
  // an embedding run neither did nor could do, and one handler serving both
  // would be one sentence deciding which job it is about.
  const onProgress = new Channel();
  onProgress.onmessage = ({ event, data }) => {
    if (event === "progress") {
      el("bar").max = data.total;
      el("bar").value = data.done;
      // The walk's own handler says why this is on every report and not only
      // at the start.
      el("bar").dataset.state = BAR_RUNNING;
      sayJobStatus(embedProgressLine(data));
      return;
    }

    const ending = data;
    el("bar").max = ending.total;
    el("bar").value = ending.done;
    // Half of why the owner waited: the bar stayed partly filled in the colour
    // of a live run after the run had died. Drawn here, in the same tick as the
    // ending's own sentence, so there is no moment where the two disagree.
    el("bar").dataset.state = barState(ending);
    // The count at the moment this line was written, kept so the restatement
    // below can ask whether anything has been written here since.
    const wroteAt = sayJobStatus(embedEndingSentence(ending));
    endingDescribed = true;
    jobRunning = false;
    syncButtons();
    // The counts this run reported are this run's; the settings line states the
    // space's, and they have just changed. Read back from the database rather
    // than added up from the ending, which is the only way the two numbers
    // cannot come to disagree.
    //
    // And the ending's own line is written a second time from that same read,
    // because "32 of 195" is this run's queue and says nothing about how much
    // of the index now has a vector — the two consecutive runs that read as
    // "nothing moved" are on `embedIndexTail`. It is a second write and not an
    // await before the first, deliberately: awaiting the settings would hold a
    // moving progress line and a live-looking bar on screen for the length of a
    // database read, which is the defect this whole task is about.
    //
    // ⚠️ **Whether it may land at all is `restatedEnding`'s decision, not an
    // `if` here.** A person can press Embed again inside that round trip, and a
    // restatement landing afterwards would paint the previous run's ending —
    // carrying numbers measured before the new run started — over a line
    // describing a run in flight. Written as a branch in this file it would be a
    // branch no test could reach; `render.test.js` asserts both directions, and
    // `main.test.js` drives this handler through both orderings that made the
    // flag the wrong thing to ask.
    //
    // The two numbers, and neither of them is `jobRunning` or a press: `wroteAt`
    // is the count when this handler wrote its own line, `statusWrites` is the
    // count now. Equal means nothing has been written here since, which is the
    // only thing that makes revising this line safe.
    //
    // ⚠️ **A press was the wrong event to count, and an ordinary double-click
    // is what showed it.** Embed stays enabled through its own round trip, so a
    // second click gives a refusal that claimed a per-press generation the
    // running job never used, and the running job's ending was then suppressed
    // for good. Counting *writes to this line* has no such gap: a refusal writes
    // here and is therefore respected when it is the newest thing, and ignored
    // when — as in the double-click — it was written before the ending it would
    // otherwise have silenced.
    refreshSettings().then((settings) => {
      const restated = restatedEnding(ending, settings.index, wroteAt, statusWrites);
      if (restated !== null) {
        sayJobStatus(restated);
      }
    });
  };

  // Before the await, and restored on a refusal; the walk's press carries the
  // reasoning for both halves.
  const barWas = el("bar").dataset.state ?? "";
  el("bar").dataset.state = BAR_RUNNING;

  // Class A, the weakest member (§2): `start_embed_job` does not change
  // whether a question leaves this machine, only the coverage numbers the
  // arm report states — but it moves them the moment the core accepts the
  // press, before this window has read anything back, so a search submitted
  // in that gap would report content-arm coverage that is already wrong.
  // Gated for the length of *this* round trip only — accepted-or-refused,
  // not the whole run: the run's own ending is asynchronous, arrives on
  // `onProgress` above and settles `refreshSettings()` of its own accord
  // (`:1012`-ish, in the handler above), which is a Class B concern
  // `drawArmState`'s barrier-aware redraw already covers, not something
  // `#search-submit` needs held shut for minutes at a time.
  await withSearchGated(async () => {
    try {
      endingDescribed = false;
      await invoke("start_embed_job", { onProgress });
      jobRunning = true;
      // After the await, for the reason the walk's own press gives: an ending
      // that arrived first has already been overwritten by the line above, and
      // only the core can put it right.
      syncButtons();
      follow();
    } catch (error) {
      // Refused before anything started — no key, no index, or a job already
      // running. Nothing began, so the buttons must not move, and neither must
      // the bar.
      el("bar").dataset.state = barWas;
      sayJobStatus(embedNotStartedSentence(error));
      // For the reason the walk's own refusal gives: this press bumped the
      // generation before awaiting, so a settings draw that resolved inside the
      // refusal suppressed its refusal clause for a job that never started.
      await refreshSettings();
    }
  });
});

// The spec (§2) classifies these two as Class B, not Class A — neither
// touches what leaves the machine — but they reach the core through a
// **computed** command name, `command` below rather than a string literal,
// which is exactly the shape `main.test.js`'s site test has to be able to
// see so a future Class A mutation cannot hide behind the same pattern.
// Gated here anyway, deliberately wider than §2 strictly asks:
// telling "this identifier happens to be Class B" from "this identifier is
// a new Class A write" needs more than a source-text pin can prove, and
// gating every config mutation through the one barrier — matching model or
// not — costs a search no more than the round trip these two already pay
// for. See the report for the argument in full.
for (const [role, command] of [
  ["rerank", "set_rerank_model"],
  ["chat", "set_chat_model"],
]) {
  el(selectId(role)).addEventListener("change", async (event) => {
    const model = event.target.value;
    await withSearchGated(async () => {
      try {
        await invoke(command, { model });
        el("model-status").textContent = asSentence(roleRecordedSentence(role, model));
      } catch (error) {
        el("model-status").textContent = asSentence(roleNotRecordedSentence(role, error));
      }
      await refreshSettings();
    });
  });
}

// Settings first, so the one sentence §3.2 actually requires is on screen
// before three network round trips have to finish. `showRecorded` finds empty
// pickers on this pass and says nothing about them, which is what the
// `notAsked` list state is for.
await refreshSettings();

// `allSettled`, not `all`. `fillRole` handles its own rejections, but it has
// one throwing path outside its own `try` — `el(...)` returning null for an id
// this file and the HTML disagree about — and with `all` any such rejection
// skipped the draw below entirely, leaving `#disclosure` blank. A blank
// disclosure reads as "no promise was made", not as "this window could not
// tell you", and it is the one line the requirements do not let this window
// omit.
await Promise.allSettled(ROLES.map(fillRole));

// Again, now that the pickers have options to hold the recorded values.
await refreshSettings();
