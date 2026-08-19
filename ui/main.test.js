// `main.js` driven, rather than read.
//
// Everything in `render.js` is a pure function and `render.test.js` calls it
// directly. `main.js` is the other half — elements, listeners, `invoke` — and
// for the whole of this branch the only thing any test could do with it was
// read its source and match a regexp. That floor caught real defects and is
// still there; what it cannot do is decide a question about **ordering**, and
// the two defects this file exists for are both orderings:
//
//   - a run that ends before its own `invoke` resolves, and
//   - a press that lands inside another run's settings round trip.
//
// Both were found by the review of `3b18859` doing exactly what is below —
// stubbing `window.__TAURI__` and `document`, importing `main.js`, and holding
// individual IPC calls open until the interleaving under test had happened.
// The guard that commit added asked `jobRunning`, which is set only *after* a
// press's await; it therefore answered wrongly in both directions, and in one
// of them it permanently withheld a sentence on `total === 0` — the ordinary
// answer to a second press. A source-text assertion cannot see that. This can.
//
// ⚠️ No browser, no jsdom, no dependency: the fake below is about sixty lines
// and does exactly what this window asks of the DOM. `ui/` has no `node_modules`
// and this file is not the place to give it one.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// A deferred promise, which is the whole technique: an IPC call the test can
// leave hanging until the thing that must happen first has happened.
const deferred = () => {
  let settle;
  const promise = new Promise((resolve, reject) => {
    settle = { resolve, reject };
  });
  return { promise, ...settle };
};

// Let every already-resolved promise chain run to its end. Two turns of the
// macrotask queue is more than the deepest chain here needs; the assertions
// below are about what did or did not happen, so over-draining is safe and
// under-draining is the flake.
const settleEverything = async () => {
  for (let i = 0; i < 4; i += 1) {
    await new Promise((wake) => setTimeout(wake, 0));
  }
};

// The smallest element this window is happy with. `value` mimics a real
// `<select>` deliberately: assigning one no option carries leaves it blank,
// which is the behaviour `showRecorded` reads back to decide whether the
// recorded model is on screen.
const makeElement = () => {
  const element = {
    textContent: "",
    dataset: {},
    disabled: false,
    hidden: false,
    placeholder: "",
    size: 0,
    max: 0,
    className: "",
    options: [],
    listeners: new Map(),
    _value: "",
    get value() {
      return this._value;
    },
    set value(next) {
      this._value =
        this.options.length === 0 || this.options.some((o) => o.value === next) ? next : "";
    },
    addEventListener(type, handler) {
      this.listeners.set(type, handler);
    },
    replaceChildren(...children) {
      this.options = children.filter((c) => c.value !== undefined);
    },
    append(...children) {
      this.options.push(...children.filter((c) => c.value !== undefined));
    },
  };
  return element;
};

// One window, one `main.js`, one set of stubs.
//
// `main.js` is imported with a unique query string, because Node caches modules
// by specifier and this file boots several independent windows — without it the
// second test would silently reuse the first one's module state, which is the
// kind of shared-state defect that reads as a flake.
let bootCount = 0;

const boot = async (answers = {}) => {
  const elements = new Map();
  const el = (id) => {
    if (!elements.has(id)) {
      elements.set(id, makeElement());
    }
    return elements.get(id);
  };

  const calls = [];
  const channels = [];
  // Commands the test wants to hold open: `pending.model_settings` is a queue of
  // deferreds handed out in call order.
  const pending = {};

  const settings = () => ({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });

  const defaults = {
    open_index: () => ({ path: "/tmp/index", schemaVersion: 1 }),
    job_status: () => ({ running: false }),
    model_settings: () => settings(),
    provider_models: () => ({ entries: [], unreadable: 0, unreadableRecords: [] }),
    start_embed_job: () => null,
    start_walk_job: () => null,
    skips: () => [],
    cancel_job: () => null,
  };

  const invoke = (command, args) => {
    calls.push(command);
    if (args && args.onProgress) {
      channels.push(args.onProgress);
    }
    if (pending[command] && pending[command].length) {
      return pending[command].shift().promise;
    }
    const answer = (answers[command] ?? defaults[command] ?? (() => null))(args);
    return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
  };

  class Channel {
    constructor() {
      this.onmessage = null;
    }
  }

  globalThis.window = {
    __TAURI__: { core: { invoke, Channel }, dialog: { open: async () => null } },
  };
  globalThis.document = {
    getElementById: el,
    createElement: () => makeElement(),
  };

  bootCount += 1;
  await import(`./main.js?window=${bootCount}`);
  await settleEverything();

  return {
    el,
    calls,
    channels,
    // Hold the next call to `command` open, and hand back its resolver.
    hold(command) {
      const d = deferred();
      pending[command] = pending[command] ?? [];
      pending[command].push(d);
      return d;
    },
    async press(id) {
      elements.get(id).listeners.get("click")({});
      await settleEverything();
    },
    async send(channel, message) {
      channel.onmessage(message);
      await settleEverything();
    },
    status() {
      return el("job-status").textContent;
    },
  };
};

const progress = (data) => ({ event: "progress", data });
const ending = (data) => ({ event: "ended", data });

// Boots a window without waiting for it to settle — the one thing `boot()`
// cannot produce, because it always awaits the import to completion, and by
// the time it returns every one of `main.js`'s own initial
// `refreshSettings()` calls has already resolved (`hold()` only exists on
// the object `boot()` hands back, by which point it is too late to hold
// anything the module asks for on its way up). This holds `model_settings`
// *before* the module is imported, so its own first top-level
// `await refreshSettings()` genuinely suspends there, and the caller can
// read the DOM while that suspension is real — round 3, §3.3.
const bootPending = (answers = {}) => {
  const elements = new Map();
  const el = (id) => {
    if (!elements.has(id)) {
      elements.set(id, makeElement());
    }
    return elements.get(id);
  };

  const channels = [];
  const pending = {};

  const settings = () => ({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });

  const defaults = {
    open_index: () => ({ path: "/tmp/index", schemaVersion: 1 }),
    job_status: () => ({ running: false }),
    model_settings: () => settings(),
    provider_models: () => ({ entries: [], unreadable: 0, unreadableRecords: [] }),
    start_embed_job: () => null,
    start_walk_job: () => null,
    skips: () => [],
    cancel_job: () => null,
  };

  const invoke = (command, args) => {
    if (args && args.onProgress) {
      channels.push(args.onProgress);
    }
    if (pending[command] && pending[command].length) {
      return pending[command].shift().promise;
    }
    const answer = (answers[command] ?? defaults[command] ?? (() => null))(args);
    return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
  };

  class Channel {
    constructor() {
      this.onmessage = null;
    }
  }

  globalThis.window = {
    __TAURI__: { core: { invoke, Channel }, dialog: { open: async () => null } },
  };
  globalThis.document = {
    getElementById: el,
    createElement: () => makeElement(),
  };

  // Queued *before* `import()` runs, so the module's own first
  // `invoke("model_settings")` call — issued from its first top-level
  // `await refreshSettings()` — is the one that finds it and suspends.
  const modelSettings = deferred();
  pending.model_settings = [modelSettings];

  bootCount += 1;
  const importPromise = import(`./main.js?window=${bootCount}`);

  return { el, channels, modelSettings, importPromise };
};

// ─────────────────────────────────────────────────────────────────────────────

// **Important 1B, and the expensive half.** A run that ends before its own
// `invoke` resolves sets the flag `true` for a run that is already over — and
// the ending's `model_settings` was issued earlier than `follow()`'s
// `job_status`, so answering first is the *expected* ordering, not a rare one.
// The guard that asked `jobRunning` returned `null` there and nothing ever
// retried it: the line kept one pair for good, on `total === 0`, which is the
// ordinary answer to a second press.
test("a run that ended before its own invoke resolved still gets the index's pair", async () => {
  const w = await boot();

  const start = w.hold("start_embed_job");
  const read = w.hold("model_settings");
  const poll = w.hold("job_status");

  await w.press("embed");
  const channel = w.channels[w.channels.length - 1];

  // ⚠️ **The precondition, asserted rather than arranged.** This scenario is
  // only about anything while the press's own `invoke` is still outstanding —
  // `jobRunning` is set after it returns, and `syncButtons` disables Embed from
  // that flag, so an enabled Embed is this window saying it has not come back.
  // 1A's own history is the argument for spelling this out: it was green about
  // nothing until its premise was made explicit (review, Minor 3).
  assert.equal(
    w.el("embed").disabled,
    false,
    "premise: the press's invoke has already resolved, so this is no longer the ordering \
     under test",
  );

  // The ending arrives first — before the press's own `invoke` has come back.
  await w.send(channel, ending({ reason: "completed", done: 0, total: 0, refused: 0 }));
  assert.match(w.status(), /nothing was waiting/, "the run's own sentence is not on screen");
  assert.doesNotMatch(
    w.status(),
    /with a vector/,
    "premise: the restatement is still in flight, so the guard is what decides it",
  );

  // Now the press resolves, which is what sets the flag for a run already over.
  start.resolve(null);
  await settleEverything();

  // And the settings answer, still ahead of the poll that would clear the flag.
  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  assert.match(
    w.status(),
    /8 pieces with a vector, of 9 in the whole index/,
    "the run's line never got the index's pair, and nothing retries it — the person is left \
     with 'nothing was waiting to be embedded' and no answer to 'so is it finished?'",
  );
  poll.resolve({ running: false });
  await settleEverything();
});

// **Important 3, and the ordinary way in: a double-click.** Embed stays enabled
// through its own round trip, so two clicks give a refusal while the first run
// is starting. Counting *presses* let that refusal claim a number the running
// job never used, and the running job's ending was then suppressed for good —
// a regression against the flag, which had cost a double-click nothing.
test("a double-click's refusal does not silence the run that is actually starting", async () => {
  // The first press starts a run and its `invoke` is left hanging — which is
  // the whole window this defect lives in. The second is refused by the slot,
  // exactly as the core does. ⚠️ Answered here rather than through `hold`,
  // because `hold` is consumed before `answers` runs, so the counter would
  // never see the first call and the second click would succeed instead of
  // being refused. Measured: the premise assertion below caught it.
  const startA = deferred();
  let started = 0;
  const w = await boot({
    start_embed_job: () => {
      started += 1;
      return started === 1 ? startA.promise : new Error("a job is already running");
    },
  });

  const poll = w.hold("job_status");
  const read = w.hold("model_settings");

  await w.press("embed");
  // The second click of the double-click, inside the first press's round trip.
  await w.press("embed");
  assert.match(w.status(), /nothing was embedded/, "premise: the second click was refused");

  startA.resolve(null);
  await settleEverything();

  const channel = w.channels[0];
  await w.send(channel, ending({ reason: "completed", done: 3, total: 5, refused: 0 }));
  assert.match(w.status(), /3 of 5 embedded in this run/, "premise: the run that started ended");

  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  assert.match(
    w.status(),
    /8 pieces with a vector, of 9 in the whole index/,
    `a double-click cost the run its index pair, permanently: "${w.status()}"`,
  );
  poll.resolve({ running: false });
  await settleEverything();
});

// The other side of the same rule, and the reason a refused press must not
// simply give its number back: a refusal that lands **after** an ending is the
// newest thing on the line and is what the person needs to read. Rolling the
// press's number back would let the older ending paint over it.
test("a refusal after an ending is not painted over by that ending's restatement", async () => {
  let started = 0;
  const w = await boot({
    start_embed_job: () => {
      started += 1;
      return started === 1 ? null : new Error("no provider key has been entered");
    },
  });

  const poll = w.hold("job_status");
  const read = w.hold("model_settings");

  await w.press("embed");
  const channel = w.channels[0];
  await w.send(channel, ending({ reason: "completed", done: 3, total: 5, refused: 0 }));
  assert.match(w.status(), /3 of 5 embedded in this run/, "premise: the run's ending is on the line");

  // Pressed after the ending, and refused for a reason of its own.
  await w.press("embed");
  assert.match(w.status(), /no provider key/, "premise: the refusal is the newest thing said");

  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  assert.match(
    w.status(),
    /no provider key/,
    `the previous run's ending was painted over the reason this person's press failed: \
     "${w.status()}"`,
  );
  poll.resolve({ running: false });
  await settleEverything();
});

// **Important 1A, the other direction of the same lag.** A newer run can be
// reporting progress before its own `invoke` resolves, so the flag still reads
// `false` when the older run's settings come back — and the older ending, with
// numbers measured before the newer run started, is painted over a live line.
test("a stale restatement does not land on a newer run's line", async () => {
  const w = await boot();

  // ⚠️ The poller is held for the whole scenario. Left to answer, it calls
  // `refreshSettings` itself and takes the held read — and then the ending's own
  // read resolves immediately, the restatement lands *before* the second press,
  // and the guard is never reached. The first version of this test passed that
  // way: green, and about nothing.
  const poll = w.hold("job_status");
  const readA = w.hold("model_settings");

  await w.press("embed");
  const channelA = w.channels[w.channels.length - 1];
  await w.send(
    channelA,
    ending({ reason: "failed", done: 3, total: 5, refused: 0, message: "the network went away" }),
  );
  assert.match(w.status(), /the network went away/, "premise: run A's ending is on the line");
  assert.doesNotMatch(
    w.status(),
    /with a vector/,
    "premise: A's restatement is still in flight, so the guard is what decides it",
  );

  // Run B: pressed inside that round trip, and reporting before its own
  // `invoke` comes back — which is what leaves the flag saying `false`.
  const startB = w.hold("start_embed_job");
  await w.press("embed");
  const channelB = w.channels[w.channels.length - 1];
  await w.send(channelB, progress({ done: 1, total: 4, refused: 0, secondsLeft: 90 }));
  assert.match(w.status(), /embedding: 1 of 4/, "premise: a newer run is on the line");

  // A's read comes back last. It must not speak.
  readA.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  assert.match(
    w.status(),
    /embedding: 1 of 4/,
    `the previous run's ending was painted over a live run's progress line: "${w.status()}"`,
  );
  assert.doesNotMatch(w.status(), /the network went away/);
  startB.resolve(null);
  poll.resolve({ running: false });
  await settleEverything();
});

// **Important 2, the same root reaching the settings line.** `drawSettings`
// runs after its own await, so a run that started inside that read was
// invisible to it — and the line went on stating `none were refused by the
// provider` beside a run that was live and could be refusing chunks as it was
// read.
test("the settings line makes no claim about refusals when a run started inside its read", async () => {
  const w = await boot();

  // Held for the reason the test above gives: an unheld poller consumes the
  // read this scenario is built around.
  const poll = w.hold("job_status");
  const readA = w.hold("model_settings");

  await w.press("embed");
  const channelA = w.channels[w.channels.length - 1];
  await w.send(channelA, ending({ reason: "completed", done: 3, total: 5, refused: 0 }));

  // A press inside the read, still awaiting its own `invoke`.
  const startB = w.hold("start_embed_job");
  await w.press("embed");

  readA.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  const line = w.el("embedding-progress").textContent;
  assert.doesNotMatch(
    line,
    /none were refused/,
    `a claim about a moment that has passed, beside a run that has started: "${line}"`,
  );
  assert.match(line, /counts from before it started/);
  startB.resolve(null);
  poll.resolve({ running: false });
  await settleEverything();
});

// Both directions of the same predicate: with nothing else pressed, the ordinary
// run says its pair and the settings line goes back to answering about
// refusals. Without this, everything above is satisfied by a window that has
// stopped saying either.
test("with nothing newer pressed, the ending speaks and the settings line answers", async () => {
  const w = await boot();

  await w.press("embed");
  const channel = w.channels[w.channels.length - 1];
  await w.send(
    channel,
    ending({ reason: "failed", done: 3, total: 5, refused: 0, message: "the network went away" }),
  );

  assert.match(w.status(), /3 of 5 embedded in this run/);
  assert.match(w.status(), /press Embed/);
  assert.match(w.status(), /8 pieces with a vector, of 9 in the whole index/);
  assert.match(w.el("embedding-progress").textContent, /none were refused/);
});

// The bar, in the one place its appearance is decided rather than described:
// what the element actually carries after each kind of ending, and after a
// press. `render.test.js` pins the decision and the stylesheet; this pins that
// the element receives it.
test("the bar carries an ended state after a run, and a running one on the press", async () => {
  const w = await boot();

  // The poller is held open for the length of each run, which is what the core
  // does for real: left to answer `running: false` straight away it would take
  // the no-channel path and describe a job that had only just started.
  const firstPoll = w.hold("job_status");
  await w.press("embed");
  assert.equal(w.el("bar").dataset.state, "running", "the press left the bar looking ended");

  const channel = w.channels[w.channels.length - 1];
  await w.send(
    channel,
    ending({ reason: "failed", done: 3, total: 5, refused: 0, message: "the network went away" }),
  );
  assert.equal(
    w.el("bar").dataset.state,
    "stopped",
    "a run that died left the bar in the colour of one still going — the defect the owner waited in \
     front of",
  );
  firstPoll.resolve({ running: false });
  await settleEverything();

  // Both directions: a run that embedded everything must not be drawn as one
  // that stopped short.
  const secondPoll = w.hold("job_status");
  await w.press("embed");
  const second = w.channels[w.channels.length - 1];
  await w.send(second, ending({ reason: "completed", done: 5, total: 5, refused: 0 }));
  assert.equal(w.el("bar").dataset.state, "finished");
  secondPoll.resolve({ running: false });
  await settleEverything();
});

// `search` answers a `SearchAnswer` (`{ hits, text, content }`), not a bare
// hit array — `searchResultItems` needs `hits` specifically, and reading the
// whole answer as the array throws inside `.map`, which a source-text read
// cannot see coming from the command's own shape changing underneath it.
// The two arms carry different numbers on purpose: `TEXT_ARM_TEXT` was
// written by copying `CONTENT_ARM_TEXT` (`render.js`), and a fixture that
// gave both arms the same count could not tell a copy-paste that kept the
// wrong word ("content") from one that did not — nor could it tell a swap
// of which line gets which report. Full-sentence equality closes both.
test("search draws the hits and both arm sentences from one SearchAnswer", async () => {
  const w = await boot({
    search: () => ({
      hits: [{ relativePath: "a.txt", text: "fox" }],
      text: { kind: "answered", matched: 5 },
      content: { kind: "answered", matched: 7, embedded: 10, total: 10, reachable: 10 },
    }),
  });

  w.el("query").value = "fox";
  await w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.el("results").options.length,
    1,
    "the hit from `hits` did not reach the list",
  );
  const [hitLi] = w.el("results").options;
  assert.equal(hitLi.options.length, 2, "a hit li does not carry its two lines");
  assert.equal(hitLi.options[0].textContent, "a.txt");
  assert.equal(hitLi.options[1].textContent, "fox");
  assert.equal(w.el("text-arm-state").textContent, "Search by text returned 5.");
  assert.equal(w.el("content-arm-state").textContent, "Search by content returned 7.");
});

// Codex round 2, Finding 4: the `catch` block replaced the result list with
// an error but never touched `text-arm-state`/`content-arm-state`, which
// `search()` only sets on success — so a failed search left the *previous*
// successful search's arm report on screen, indistinguishable from a report
// about the failed attempt.
test("a failed search clears the previous arm-state text instead of leaving it stale", async () => {
  let succeed = true;
  const w = await boot({
    search: () => {
      if (succeed) {
        succeed = false;
        return {
          hits: [{ relativePath: "a.txt", text: "fox" }],
          text: { kind: "answered", matched: 5 },
          content: { kind: "answered", matched: 7, embedded: 10, total: 10, reachable: 10 },
        };
      }
      return new Error("the index could not be reached");
    },
  });

  w.el("query").value = "fox";
  await w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();
  assert.equal(
    w.el("text-arm-state").textContent,
    "Search by text returned 5.",
    "premise: a successful search reported an arm state",
  );

  w.el("query").value = "fox again";
  await w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.el("text-arm-state").textContent,
    "",
    "a failed search left the previous successful search's arm-state text on screen",
  );
  assert.equal(
    w.el("content-arm-state").textContent,
    "",
    "a failed search left the previous successful search's arm-state text on screen",
  );
});

// Review round 1, Important 1: a checkbox that always drew on, regardless of
// what `set_search_arms` had saved, contradicted its own sentence the moment
// somebody saved an arm off and reopened the window — the sentence beside it
// (`contentArmSentence`, drawn from a real search) said "is off" while the
// box drew checked. `drawSettings` must read `read.searchTextArm` /
// `read.searchContentArm` back, not default both to on.
test("a saved-off arm is drawn off, not defaulted to on, once model_settings answers", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: false,
      },
    }),
  });

  assert.equal(
    w.el("arm-content").checked,
    false,
    "a saved-off content arm was drawn checked",
  );
});

// The other half of the same fix: an index that stops being readable must not
// reset a choice already read back to the true default, which would silently
// turn a saved-off arm back on the moment a read failed for any reason.
//
// ⚠️ Text, not content: `content.checked` also depends on
// `armModelChosen`, which itself goes false the moment `read` is null — so a
// content-arm assertion here would pass even without the guard, satisfied by
// that unrelated dependency rather than by the guard this test is for.
// `text.checked` is `savedTextArm` alone, with no such neighbour to hide
// behind.
test("an index that stops being readable does not reset a saved-off arm to on", async () => {
  let call = 0;
  const w = await boot({
    model_settings: () => {
      call += 1;
      if (call === 1) {
        return {
          key: { kind: "present" },
          platform: "mac",
          index: {
            kind: "read",
            activeSpace: 1,
            embeddedChunks: 8,
            totalChunks: 9,
            failedChunks: 0,
            embeddedChunksEverywhere: 8,
            embeddingModel: "vendor/m",
            rerankModel: null,
            chatModel: null,
            searchTextArm: false,
            searchContentArm: true,
          },
        };
      }
      return {
        key: { kind: "present" },
        platform: "mac",
        index: { kind: "unreadable", cause: "readFailed", reason: "boom" },
      };
    },
  });

  assert.equal(call, 2, "premise: model_settings was asked twice while booting");
  assert.equal(
    w.el("arm-text").checked,
    false,
    "an index that stopped being readable reset the saved choice back to on",
  );
});

// Review round 1, Important 2: `savedTextArm`/`savedContentArm` used to be
// written before `set_search_arms`'s own `await`, with nothing undoing that
// write on a refusal — so a rejected save left the window believing meta
// held a choice it never received. `#search-form`'s own listener already
// catches and says; the two arm handlers must do the same.
test("a rejected set_search_arms reverts the checkbox and says why", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
    set_search_arms: () => new Error("disk is full"),
  });

  assert.equal(w.el("arm-text").checked, true, "premise: the arm started checked");

  w.el("arm-text").checked = false;
  await w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  assert.equal(
    w.el("arm-text").checked,
    true,
    "the checkbox did not revert after a rejected write",
  );
  assert.match(w.el("arm-text-note").textContent, /disk is full/);
});

// Codex review on PR #10: `savedTextArm`/`savedContentArm` are written
// synchronously, before either handler's own `await`. A second click on the
// *other* checkbox while the first save is still in flight reads the first
// handler's already-written variable and sends its own `set_search_arms`
// call with a state nothing has confirmed yet — reachable because nothing
// before this fix stopped the second checkbox from being clicked at all
// while the first was mid-flight.
test("clicking one arm disables both checkboxes until the save resolves", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.equal(w.el("arm-text").disabled, false, "premise: the text arm starts clickable");
  assert.equal(w.el("arm-content").disabled, false, "premise: the content arm starts clickable");

  const held = w.hold("set_search_arms");
  w.el("arm-text").checked = false;
  const change = w.el("arm-text").listeners.get("change")({});

  assert.equal(
    w.el("arm-text").disabled,
    true,
    "the clicked checkbox itself stayed clickable while its own save was in flight",
  );
  assert.equal(
    w.el("arm-content").disabled,
    true,
    "the other checkbox could still be clicked while the first save was in flight",
  );

  held.resolve(null);
  await change;
  await settleEverything();

  assert.equal(
    w.el("arm-text").disabled,
    false,
    "the text arm was left disabled after its own save finished",
  );
});

// The freeze above must lift on a refusal too. The catch block already
// reverted its own checkbox before this fix existed; the *other* checkbox
// was frozen by the same click, and nothing in the catch branch ever
// unfroze it, so a rejected save used to leave it stuck disabled.
test("a rejected save re-enables the other checkbox too, not only the one that was clicked", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
    set_search_arms: () => new Error("disk is full"),
  });

  w.el("arm-text").checked = false;
  await w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  assert.equal(
    w.el("arm-content").disabled,
    false,
    "the content arm was left disabled after a rejected save on the text arm",
  );
});

// Codex round 2, Finding 1: `refreshSettings` can be issued from several
// places (`forget`, `key-form`, embedding-model handlers, polling) and any
// of them can land *after* an arm write has started but *before* it settles.
// `drawSettings` used to overwrite `savedTextArm` from whatever that read
// carried unconditionally — reverting the optimistic write to what the
// checkbox looked like before the click, with no error shown.
test("a settings read issued before an arm write does not revert it once the write has landed", async () => {
  const w = await boot();

  assert.equal(w.el("arm-text").checked, true, "premise: the text arm starts checked");

  // Issued from `forget`, not from the write below — this is the read that
  // must not win.
  const read = w.hold("model_settings");
  await w.press("forget");

  const write = w.hold("set_search_arms");
  w.el("arm-text").checked = false;
  const change = w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  // Resolves with the *old* saved value — exactly what a read issued before
  // the write started would still carry.
  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  await settleEverything();

  assert.equal(
    w.el("arm-text").checked,
    false,
    "a stale settings read reverted the optimistic write while it was still in flight",
  );

  write.resolve(null);
  await change;
  await settleEverything();
});

// The other half of the same finding: nothing stopped a search while an arm
// write was in flight, so a search could run against the arm state from
// before the change the user just made, with nothing telling them so.
test("the search form's submit is disabled while an arm write is in flight, and re-enabled once it settles", async () => {
  const w = await boot();

  assert.equal(w.el("search-submit").disabled, false, "premise: submit starts enabled");

  const write = w.hold("set_search_arms");
  w.el("arm-text").checked = false;
  const change = w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    true,
    "the submit stayed clickable while an arm write was in flight",
  );

  write.resolve(null);
  await change;
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    false,
    "the submit was left disabled after its arm write settled",
  );
});

// Codex round 2 review: the two tests above drive only `arm-text`, and the
// `arm-content` handler's own `armWriteGeneration += 1`/`search-submit`
// lines were unpinned — deleting them left the suite green. Same scenario,
// driven through the content checkbox instead.
test("a settings read issued before a content-arm write does not revert it once the write has landed", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.equal(w.el("arm-content").checked, true, "premise: the content arm starts checked");

  // Issued from `forget`, not from the write below — this is the read that
  // must not win.
  const read = w.hold("model_settings");
  await w.press("forget");

  const write = w.hold("set_search_arms");
  w.el("arm-content").checked = false;
  const change = w.el("arm-content").listeners.get("change")({});
  await settleEverything();

  // Resolves with the *old* saved value — exactly what a read issued before
  // the write started would still carry.
  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: "vendor/m",
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  await settleEverything();

  assert.equal(
    w.el("arm-content").checked,
    false,
    "a stale settings read reverted the optimistic write while it was still in flight",
  );

  write.resolve(null);
  await change;
  await settleEverything();
});

test("the search form's submit is disabled while a content-arm write is in flight, and re-enabled once it settles", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.equal(w.el("search-submit").disabled, false, "premise: submit starts enabled");

  const write = w.hold("set_search_arms");
  w.el("arm-content").checked = false;
  const change = w.el("arm-content").listeners.get("change")({});
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    true,
    "the submit stayed clickable while a content-arm write was in flight",
  );

  write.resolve(null);
  await change;
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    false,
    "the submit was left disabled after its content-arm write settled",
  );
});

// Review round 1, Important 1: `drawArmState()` alone never touched
// `#disclosure`, so unticking "Search by content" left the sentence still
// promising "every question you ask" — the very promise that checkbox had
// just switched off, until the next `model_settings` round trip caught up.
// `drawArmStateAndDisclosure` closes that; this drives the checkbox the way
// a person would, not `drawArmState` directly.
test("switching the content arm off updates the disclosure sentence too", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.match(
    w.el("disclosure").textContent,
    /every question you ask/,
    "premise: the content arm started on, so the disclosure named the question",
  );

  w.el("arm-content").checked = false;
  await w.el("arm-content").listeners.get("change")({});
  await settleEverything();

  assert.doesNotMatch(
    w.el("disclosure").textContent,
    /every question you ask/,
    "the disclosure still promised questions leave after the arm that sends them was switched off",
  );
});

// Codex round 2 review: the test above covers `contentArmRuns`, but the
// `textRuns: text.checked` half of the same `disclosureSentence` call
// (`ui/main.js`, `drawArmStateAndDisclosure`) had no `main.test.js` pin —
// deleting that one line left the suite green. Same shape, text checkbox,
// absent key.
test("switching the text arm off with an absent key updates the disclosure sentence too", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "absent" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: null,
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.match(
    w.el("disclosure").textContent,
    /Search works on words/,
    "premise: the text arm started on, so the disclosure claimed search works",
  );

  w.el("arm-text").checked = false;
  await w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  assert.doesNotMatch(
    w.el("disclosure").textContent,
    /Search works on words/,
    "the disclosure still claimed search works on words after the arm that runs it was switched off",
  );
});

// A press that is refused starts nothing, so it must not leave the bar claiming
// a run — and it must put the settings line back, because it bumped the
// generation before awaiting and any read still in flight will therefore
// suppress its refusal clause for a job that never existed.
//
// ⚠️ **The read has to be genuinely in flight for this to test anything.** The
// first version of this pressed a refused button on a freshly booted window and
// asserted the clause was present — which it was, from the boot's own draw,
// with the refusal handler removed as well. Satisfied by a neighbouring
// defence, which is the sixth instance of that shape this cycle; measured, not
// reasoned about.
test("a refused press leaves the bar as it was and puts the settings line back", async () => {
  let refuse = false;
  const w = await boot({
    start_embed_job: () => (refuse ? new Error("a job is already running") : null),
  });

  // A run ends, and its settings read is held open. ⚠️ The poller is held for
  // the same reason as in the two tests above, and here it is what makes the
  // test test anything: left to answer, it issues a read of its own, the
  // ending's read is superseded by it, and the refusal's redraw is then
  // indistinguishable from its absence. Measured — this case stayed green
  // against the revert that removes the redraw until the poll was held.
  const poll = w.hold("job_status");
  const readA = w.hold("model_settings");
  await w.press("embed");
  const channel = w.channels[w.channels.length - 1];
  await w.send(channel, ending({ reason: "completed", done: 3, total: 5, refused: 0 }));
  const barAfterTheRun = w.el("bar").dataset.state;
  assert.equal(barAfterTheRun, "finished", "this test's premise is a run that finished");

  // A press inside that read, which is refused.
  refuse = true;
  await w.press("embed");
  assert.match(w.status(), /nothing was embedded/);
  assert.equal(
    w.el("bar").dataset.state,
    barAfterTheRun,
    "a press that started nothing changed what the bar says",
  );

  // The held read now comes back, and is suppressed — correctly, because when
  // it was issued this window could not yet know the press would be refused.
  readA.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
    },
  });
  await settleEverything();

  assert.match(
    w.el("embedding-progress").textContent,
    /none were refused/,
    `the settings line is left saying a job is running, for a press that was refused and \
     started nothing: "${w.el("embedding-progress").textContent}"`,
  );
  poll.resolve({ running: false });
  await settleEverything();
});

// ─────────────────────────────────────────────────────────────────────────────
// Round 3: one readiness barrier for `#search-submit`
// (`docs/private/sdd/2026-08-14-search-query-and-fusion/
// ui-readiness-barrier-spec.md`). Closes Codex round-3 findings 2, 3 and 9.

// Round 3, the floor test (spec §5): a source-text pin, because JS gives no
// type to lean on — the same technique this branch already uses for the
// `ORDER BY` secondary key in `search.rs`/`space.rs` (`crates/mnema-index/
// src/{search,space}.rs`), with the same trap avoided: that pin first read
// its own `#[cfg(test)]` block back into its own search and could never go
// red, fixed by `split_once` before matching. This one reads a *different*
// file (`main.js`), so there is no self-inclusion to guard against — but it
// is still a pin over source text, not a real parser, and the gaps below
// are the price of that, stated rather than left to be discovered.
//
// This does **not** enumerate Class A commands by name and check each one is
// present — that is the exact shape of mistake this floor test exists to
// end: a check that passes forever while an eighth site arrives untested.
// It finds **every** `invoke(...)` call in the file and requires each one
// to be either explicitly out of scope (§6, plus the two read-only commands
// the barrier is itself built around — `search`, which it gates from the
// outside, and `model_settings`, the read that opens it) or lexically
// inside a `withSearchGated(...)` call and not deferred out of it.
//
// U-1 (adversarial pass, F-A2) found the same mistake one level down, in
// this file's own scan rather than in `main.js`: the original regex found
// an `invoke(...)` call's argument and classified it in the same step, so
// an argument shape it could not parse — a template literal, an aliased
// binding through another name — produced no match *at all*, and the call
// was invisible rather than reported. And the deferral check named four
// constructs by hand (`setTimeout`, `.then(`, `queueMicrotask`,
// `requestAnimationFrame`) and missed three that do the identical thing
// (`.catch`, `.finally`, `setInterval`) — the third round in a row an
// enumeration here covered half its sites.
//
// Both are fixed the same way: **find first, classify second.**
// `invokeSites` finds every textual `invoke(` occurrence with no filter on
// what follows it. `classifyInvokeArg` then reads the first argument and
// returns `literal` (a command name), `computed` (a bare identifier — the
// `set_rerank_model`/`set_chat_model` loop's `command` variable) or
// `unclassified` — and an unclassified shape is itself reported, by name,
// whether or not the call also turns out to be gated, because a shape this
// scan cannot read is a shape it cannot vouch for. `aliasSites` catches the
// one shape that never appears as `invoke(` at all — `invoke` assigned to
// another binding — by failing loud at the assignment, since the test
// cannot follow the binding to wherever it is later called.
//
// The deferral check no longer names a construct either.
// `nestedDeferralSpans` finds every function/arrow boundary *inside* a
// gate's own body — not the gate's own immediate function argument, which
// `gateBodyRange` excludes, but anything nested one level deeper — and any
// `invoke(` inside one of those spans counts as ungated, because the
// callback holding it runs after its own turn on the event loop, by which
// point the gate has already reopened. One rule instead of a list is what
// makes `setInterval`/`.catch`/`.finally` fall out of it for free, along
// with whatever the next round reaches for.
//
// The trap this has to keep clearing: `main.js` has one legitimate
// deferral inside a gated *handler* — `refreshSettings().then((settings)
// => { ... })`, inside `#embed`'s `onProgress` "ended" callback — but that
// callback is assigned *before* the handler's own `withSearchGated(...)`
// call, not textually inside it, so `nestedDeferralSpans` never looks at
// it at all. Checked as a standalone run against the real, unmodified
// `main.js` before this was wired into the test below (see the report).
//
// What it still cannot see: a block comment (`main.js` has none — every
// comment in it is `//`) would not be skipped the way a line comment is,
// and a template literal's `${...}` containing an unbalanced brace or
// paren could desync `matchBrace`/`matchParen` — both skip a string or
// template's contents whole, by matching quote to quote, so this only
// bites if the imbalance is inside the interpolation's own code, which
// nothing in `main.js` today does.
//
// The allowlist itself is the other route review found: `OUT_OF_SCOPE_
// COMMANDS` cannot be made un-editable — an allowlist has to stay
// editable — but the most likely way to silence this floor test is a
// one-line addition there instead of gating a real mutation. `no
// out-of-scope command name looks like a config mutation`, below, narrows
// that: every command this file actually mutates config through is named
// `set_*` or `forget_*`, so a name shaped like one of those appearing in
// the allowlist is the shape of the mistake to catch. This does not close
// the route, only narrows it.
const mainJsPath = fileURLToPath(new URL("./main.js", import.meta.url));

// Skips a string, a template literal (honouring `\` escapes; a template's
// `${...}` is skipped along with the rest of it, quote to quote, not
// parsed), or a `//` line comment starting at `src[i]`. Returns the index
// to resume from, or `null` if `i` does not start one of those — the one
// piece of "is this really code" every scanner below shares, so a stray
// paren, brace or comma inside a string or a comment cannot desync any of
// them the same way it could when each had its own copy of this check.
const skipNonCode = (src, i) => {
  const c = src[i];
  if (c === '"' || c === "'" || c === "`") {
    let j = i + 1;
    while (j < src.length && src[j] !== c) {
      if (src[j] === "\\") {
        j += 1;
      }
      j += 1;
    }
    return j + 1;
  }
  if (c === "/" && src[i + 1] === "/") {
    let j = i;
    while (j < src.length && src[j] !== "\n") {
      j += 1;
    }
    return j;
  }
  return null;
};

// Returns the source index one past the `)` that closes the `(` at
// `openIndex`, skip-aware throughout.
const matchParen = (src, openIndex) => {
  let depth = 0;
  for (let i = openIndex; i < src.length; i += 1) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip - 1;
      continue;
    }
    const c = src[i];
    if (c === "(") {
      depth += 1;
    } else if (c === ")") {
      depth -= 1;
      if (depth === 0) {
        return i + 1;
      }
    }
  }
  throw new Error(`unbalanced parens from source offset ${openIndex}`);
};

// `matchParen`'s sibling for `{`/`}`, used to find where a function or
// arrow body — the gate's own, or one nested inside it — actually ends.
const matchBrace = (src, openIndex) => {
  let depth = 0;
  for (let i = openIndex; i < src.length; i += 1) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip - 1;
      continue;
    }
    const c = src[i];
    if (c === "{") {
      depth += 1;
    } else if (c === "}") {
      depth -= 1;
      if (depth === 0) {
        return i + 1;
      }
    }
  }
  throw new Error(`unbalanced braces from source offset ${openIndex}`);
};

// Every `[start, end)` span textually inside a `withSearchGated(...)` call —
// the one decision point §3.1 asks for, and the only thing that counts as
// "gated" for this test.
const gatedRanges = (src) => {
  const ranges = [];
  const re = /withSearchGated\(/g;
  let m;
  while ((m = re.exec(src))) {
    const openParen = m.index + "withSearchGated".length;
    ranges.push([m.index, matchParen(src, openParen)]);
  }
  return ranges;
};

// The gate's own body: every call site in this file is
// `withSearchGated(async (…) => { … })`, and this finds that function
// argument's own `{ … }` — the first real `=>` after `gateStart`, then the
// first real `{` after that. `null` if the argument is not this shape, in
// which case the caller falls back to treating the whole gated range as
// depth 0, same as before this file could tell nested boundaries apart.
const gateBodyRange = (src, gateStart, gateEnd) => {
  let i = gateStart;
  let arrow = -1;
  while (i < gateEnd - 1) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip;
      continue;
    }
    if (src[i] === "=" && src[i + 1] === ">") {
      arrow = i;
      break;
    }
    i += 1;
  }
  if (arrow === -1) {
    return null;
  }
  i = arrow + 2;
  while (i < gateEnd) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip;
      continue;
    }
    if (src[i] === "{") {
      return [i + 1, matchBrace(src, i) - 1];
    }
    if (!/\s/.test(src[i])) {
      return null;
    }
    i += 1;
  }
  return null;
};

// Where a concise-body arrow's own expression ends — `() => invoke(...)`,
// with no braces, is exactly shapes 3-5 of U-1's five. The body ends at the
// first `,`/`;` reached while this arrow's own paren/bracket/brace nesting
// is back to zero, or at a closing bracket that would take it negative —
// which belongs to whatever encloses the arrow, not to the arrow itself.
const conciseArrowBodyEnd = (src, start, hardLimit) => {
  let depth = 0;
  let i = start;
  while (i < hardLimit) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip;
      continue;
    }
    const c = src[i];
    if (c === "(" || c === "[" || c === "{") {
      depth += 1;
      i += 1;
      continue;
    }
    if (c === ")" || c === "]" || c === "}") {
      if (depth === 0) {
        return i;
      }
      depth -= 1;
      i += 1;
      continue;
    }
    if ((c === "," || c === ";") && depth === 0) {
      return i;
    }
    i += 1;
  }
  return hardLimit;
};

// Review, V-3: the fixed control-flow keywords `IDENT(...) {` also matches
// but is never a function boundary — `if`/`for`/`while`/`switch` guard a
// block that runs now, and `catch` (this very file's own gated handlers'
// `try { … } catch (error) { … }`) would otherwise turn every one of them
// into a false deferral. `do`/`with` are here for the same reason even
// though neither takes a parenthesised head the way this check looks for.
const DEFERRAL_CONTROL_KEYWORDS = new Set(["if", "for", "while", "switch", "catch", "do", "with"]);

// U-1 (adversarial pass, F-A2): every `[start, end)` span, inside a gate's
// own body, that is itself inside a *further* function or arrow boundary —
// construct-agnostic on purpose, so `setTimeout`, `setInterval`, `.then`,
// `.catch`, `.finally`, `queueMicrotask`, `requestAnimationFrame` and
// whatever the next round reaches for all fall out of one rule: is there a
// function boundary between here and the gate's own body, not "does this
// text match one of these names". `start`/`end` bound the gate's own
// immediate body (`gateBodyRange`'s job to find), so the gate's own
// function argument is never itself counted as nested.
//
// Review, V-3: object method shorthand (`{ async later() { … } }`) and
// class methods (`class K { async go() { … } }`) are a third way to write
// a function, and neither the `function` keyword nor `=>` matches them —
// the deferral axis had the same "unrecognised shape vanishes" defect the
// argument axis's `unclassified` branch was built to end, just not yet
// applied here. Unlike deferral *constructs*, which are an open library
// set, the ways to write a function in JS are a closed grammar: any
// `IDENT(params) {` not one of `DEFERRAL_CONTROL_KEYWORDS` is a boundary,
// covering method shorthand, class methods, getters/setters, generators,
// `static`/`async` methods and constructors alike without naming any of
// them individually.
const nestedDeferralSpans = (src, start, end) => {
  const spans = [];
  let i = start;
  while (i < end) {
    const skip = skipNonCode(src, i);
    if (skip !== null) {
      i = skip;
      continue;
    }
    if (
      src.startsWith("function", i) &&
      !/[\w$]/.test(src[i - 1] ?? "") &&
      !/[\w$]/.test(src[i + 8] ?? "")
    ) {
      const brace = src.indexOf("{", i);
      if (brace === -1 || brace >= end) {
        i += 8;
        continue;
      }
      const close = matchBrace(src, brace);
      spans.push([i, close]);
      i = close;
      continue;
    }
    if (src[i] === "=" && src[i + 1] === ">") {
      let j = i + 2;
      while (/\s/.test(src[j])) {
        j += 1;
      }
      if (src[j] === "{") {
        const close = matchBrace(src, j);
        spans.push([i, close]);
        i = close;
      } else {
        const close = conciseArrowBodyEnd(src, j, end);
        spans.push([i, close]);
        i = close;
      }
      continue;
    }
    if (/[a-zA-Z_$]/.test(src[i]) && !/[\w$]/.test(src[i - 1] ?? "")) {
      const idMatch = /^[a-zA-Z_$][\w$]*/.exec(src.slice(i, i + 200));
      const name = idMatch[0];
      let j = i + name.length;
      while (/\s/.test(src[j])) {
        j += 1;
      }
      if (src[j] === "(" && !DEFERRAL_CONTROL_KEYWORDS.has(name)) {
        const afterParen = matchParen(src, j);
        let k = afterParen;
        while (/\s/.test(src[k])) {
          k += 1;
        }
        if (src[k] === "{") {
          const close = matchBrace(src, k);
          spans.push([i, close]);
          i = close;
          continue;
        }
      }
    }
    i += 1;
  }
  return spans;
};

// Every textual `invoke(` occurrence, found with no filter at all on what
// follows it — classification is a separate step below, deliberately, so a
// shape that step cannot read still shows up here instead of vanishing.
const invokeSites = (src) => {
  const re = /\binvoke\(/g;
  const sites = [];
  let m;
  while ((m = re.exec(src))) {
    const openParen = m.index + m[0].length - 1;
    const closeParen = matchParen(src, openParen);
    sites.push({ index: m.index, argsText: src.slice(openParen + 1, closeParen - 1) });
  }
  return sites;
};

// Reads one call's leading argument. `literal` is a command name this test
// can look up in `OUT_OF_SCOPE_COMMANDS`; `computed` is a bare identifier —
// the `set_rerank_model`/`set_chat_model` loop's `command` — that this test
// cannot name but can still confirm is gated. Anything else (a template
// literal, a member expression, a nested call) is `unclassified`, and U-1's
// shape 1, `` invoke(`set_${role}_model`, …) ``, is exactly this: it is
// neither of the first two, and used to produce no match at all rather
// than reaching this branch.
const classifyInvokeArg = (argsText) => {
  const literal = /^\s*"([a-zA-Z0-9_]+)"\s*(?:,|$)/.exec(argsText);
  if (literal) {
    return { kind: "literal", name: literal[1] };
  }
  const identifier = /^\s*([a-zA-Z_$][\w$]*)\s*(?:,|$)/.exec(argsText);
  if (identifier) {
    return { kind: "computed", name: identifier[1] };
  }
  return { kind: "unclassified", name: argsText.trim().slice(0, 60) };
};

// U-1's shape 2: `invoke` itself assigned to another binding, then called
// through that binding — a call this file's own scan never sees as
// `invoke(`, so the only place it can be caught is the assignment. `[=:]`
// immediately before the bare word is what this looks for; the one
// legitimate reference in `main.js`, `const { invoke, Channel } =
// window.__TAURI__.core;`, is a destructuring pattern with `{` before
// `invoke`, not `=`/`:`, so it does not match.
const aliasSites = (src) => {
  const re = /[=:]\s*invoke\b(?!\()/g;
  const sites = [];
  let m;
  while ((m = re.exec(src))) {
    sites.push(m.index);
  }
  return sites;
};

// §6, out of scope: these mutate the index or read status, not the search
// configuration or the privacy promise — plus the two commands the barrier
// is itself built around, neither a config mutation of its own.
const OUT_OF_SCOPE_COMMANDS = new Set([
  "open_index",
  "add_watched_folder",
  "start_walk_job",
  "cancel_job",
  "job_status",
  "skips",
  "provider_models",
  "model_settings",
  "search",
]);

test("every config-mutating invoke() in main.js is inside withSearchGated(...), and not deferred out of it", () => {
  const src = readFileSync(mainJsPath, "utf8");
  const gates = gatedRanges(src);
  assert.ok(gates.length > 0, "premise: withSearchGated(...) exists in main.js at all");

  const ungated = [];
  const unclassified = [];

  for (const site of invokeSites(src)) {
    const cls = classifyInvokeArg(site.argsText);
    const label =
      cls.kind === "literal"
        ? cls.name
        : cls.kind === "computed"
          ? `<computed: ${cls.name}>`
          : `<unclassified: ${cls.name}>`;

    if (cls.kind === "literal" && OUT_OF_SCOPE_COMMANDS.has(cls.name)) {
      continue;
    }

    const gate = gates.find(([start, end]) => site.index > start && site.index < end);
    if (!gate) {
      // Never reached the barrier lexically at all — the base violation.
      ungated.push(label);
    } else {
      const body = gateBodyRange(src, gate[0], gate[1]);
      const deferred = body ? nestedDeferralSpans(src, body[0], body[1]) : [];
      // Inside the gate textually, but also inside a further function/arrow
      // boundary — the gate has already reopened by the time this call
      // actually runs, so this counts as ungated too.
      if (deferred.some(([ds, de]) => site.index > ds && site.index < de)) {
        ungated.push(label);
      }
    }
    if (cls.kind === "unclassified") {
      unclassified.push(label);
    }
  }

  assert.deepEqual(
    ungated,
    [],
    "a config-mutating invoke() is reachable outside the search barrier — either never inside " +
      "withSearchGated(...) at all, or inside it only lexically because it runs from a function " +
      "or arrow nested inside the gate's own body after the gate has already reopened — every " +
      "one of these can change what leaves the machine or clobber pending state, and none of " +
      "them is in §6's out-of-scope list",
  );
  assert.deepEqual(
    unclassified,
    [],
    "an invoke() call's argument could not be classified as a command name or a computed " +
      "identifier — this scan cannot vouch for what it targets, gated or not, and a shape it " +
      "cannot read must fail loud rather than pass in silence",
  );
  assert.deepEqual(
    aliasSites(src),
    [],
    "invoke was assigned to another binding — this scan cannot follow a call made through that " +
      "binding, so it fails at the assignment instead of missing the call entirely",
  );
});

// A2 (post-review addition): the allowlist above is the one part of this
// floor test that can be silenced quietly — an unbalanced paren in a
// comment throws, and a balanced mention of `withSearchGated(` in a comment
// merely yields an empty (harmless) range, but adding a name to
// `OUT_OF_SCOPE_COMMANDS` is a legitimate-looking one-line diff that turns
// a real config mutation invisible to every check above. This does not
// close that route — it cannot, an allowlist has to stay editable — but it
// narrows the most likely form of it: every command this file actually
// mutates config through is named `set_*` or `forget_*` (`set_key`,
// `set_search_arms`, `set_embedding_model`, `set_rerank_model`,
// `set_chat_model`, `forget_key`), so a name shaped like one of those
// showing up in the allowlist is the shape of the mistake to catch, not a
// coincidence.
test("no out-of-scope command name looks like a config mutation", () => {
  for (const name of OUT_OF_SCOPE_COMMANDS) {
    assert.ok(
      !name.startsWith("set_") && !name.startsWith("forget_"),
      `"${name}" is in the out-of-scope allowlist but is shaped like a config mutation — the ` +
        `most likely way to silence the floor test above is adding a new set_*/forget_* command ` +
        `here instead of gating it`,
    );
  }
});

// F9: `#search-submit` is enabled in the markup before this round, and the
// first draw that would say otherwise is a network round trip away. Between
// load and that draw, a search is pressable with no disclosure on screen —
// `bootPending` is what lets a test hold that window open long enough to
// look at it. §3.3.
test("search stays closed until the first settings read lands, at load (F9, §3.3)", async () => {
  const w = bootPending();
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    true,
    "search was pressable before any settings read had landed",
  );

  w.modelSettings.resolve({
    key: { kind: "absent" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 0,
      totalChunks: 0,
      failedChunks: 0,
      embeddedChunksEverywhere: 0,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  await w.importPromise;
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    false,
    "the first settings read landed but the gate stayed shut",
  );
});

// F9, the sequence Codex named: no key, then a key saved. The backend
// commits the key the instant `set_key` resolves — a search submitted right
// then would genuinely leave the machine — but `#disclosure` does not catch
// up until the trailing `refreshSettings()` inside the same handler draws.
// The gate has to stay shut across that whole span, not just across the
// `invoke` itself, or it reopens on the strength of a promise the screen is
// still making the old way.
test("a saved key does not reopen search until the disclosure has caught up with it (F9)", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "absent" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 0,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 0,
        embeddingModel: null,
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
    set_key: () => ({ balance: { kind: "notStated" } }),
  });

  assert.equal(w.el("search-submit").disabled, false, "premise: submit starts enabled, key absent");
  assert.match(
    w.el("disclosure").textContent,
    /Nothing leaves this machine/,
    "premise: the disclosure promises nothing leaves, with no key saved",
  );

  const read = w.hold("model_settings");
  w.el("key").value = "sk-example-live-key";
  const submit = w.el("key-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.el("search-submit").disabled,
    true,
    "search stayed pressable between the key being saved on the core side and the disclosure " +
      "catching up with it — the exact window F9 names",
  );
  assert.match(
    w.el("disclosure").textContent,
    /Nothing leaves this machine/,
    "premise: the disclosure is still stale — this is why the gate, not the sentence, has to " +
      "be what stands between a press and the provider here",
  );

  read.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 0,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 0,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  await submit;
  await settleEverything();

  assert.equal(w.el("search-submit").disabled, false, "the gate never reopened once the redraw landed");
});

// F2, with the shape the brief asked for specifically: a stale
// `model_settings` response drawn *in between* two overlapping arm writes.
// Without this, a test that only holds one write open passes against the
// round-2 code that already fails in production — round 2's guard protected
// the saved booleans, not the `disabled` attribute a redraw could still
// clear.
test("a stale settings redraw landing between two overlapping arm writes does not re-enable them (F2)", async () => {
  const w = await boot({
    model_settings: () => ({
      key: { kind: "present" },
      platform: "mac",
      index: {
        kind: "read",
        activeSpace: 1,
        embeddedChunks: 8,
        totalChunks: 9,
        failedChunks: 0,
        embeddedChunksEverywhere: 8,
        embeddingModel: "vendor/m",
        rerankModel: null,
        chatModel: null,
        searchTextArm: true,
        searchContentArm: true,
      },
    }),
  });

  assert.equal(w.el("arm-text").disabled, false, "premise: clickable at boot");
  assert.equal(w.el("arm-content").disabled, false, "premise: clickable at boot");

  // Issued from `forget`, held open — the "stale" read, exactly the source
  // the existing arm-write tests already use for the same reason.
  const staleRead = w.hold("model_settings");
  await w.press("forget");

  // Write A: uncheck the text arm.
  const writeA = w.hold("set_search_arms");
  w.el("arm-text").checked = false;
  const changeA = w.el("arm-text").listeners.get("change")({});
  await settleEverything();

  assert.equal(w.el("arm-text").disabled, true, "premise: write A disabled the text arm");
  assert.equal(w.el("arm-content").disabled, true, "premise: write A disabled the content arm too");

  // The stale redraw lands mid-flight — round 2's bug: `drawArmState` wrote
  // `toggleState`'s `disabled` unconditionally, clearing both.
  staleRead.resolve({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: "vendor/m",
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  await settleEverything();

  assert.equal(
    w.el("arm-text").disabled,
    true,
    "a stale settings redraw re-enabled a control write A's own pending save still owns",
  );
  assert.equal(w.el("arm-content").disabled, true, "same bug, the other checkbox");

  // Write B starts while A is still pending — the round-2 bug is exactly
  // what let this happen for real: the stale redraw's `disabled = false`
  // let a person click a checkbox a pending write should still have held
  // shut.
  const writeB = w.hold("set_search_arms");
  w.el("arm-content").checked = false;
  const changeB = w.el("arm-content").listeners.get("change")({});
  await settleEverything();

  // A settles first — must not re-open anything while B is still out (§3.2).
  writeA.resolve(null);
  await changeA;
  await settleEverything();
  assert.equal(
    w.el("arm-text").disabled,
    true,
    "the first write to settle re-opened the gate while a second write was still out",
  );
  assert.equal(w.el("arm-content").disabled, true, "same, the other checkbox");

  // B settles — now everything may re-open.
  writeB.resolve(null);
  await changeB;
  await settleEverything();
  assert.equal(w.el("arm-text").disabled, false);
  assert.equal(w.el("arm-content").disabled, false);
});

// Adversarial pass, F-A3/U-2: `withSearchGated` used to increment
// `pendingConfigWrites` and call `syncSearchGate()` *before* its own `try`.
// `syncSearchGate` dereferences `el("search-submit")`; a throw there left the
// increment applied and skipped the `finally`, so the counter never came back
// down — search stayed shut for the rest of the session, even for a later
// write that had nothing wrong with it. Not reachable through `boot()`'s own
// `el`, which never throws, so this window builds its own `getElementById`
// that throws for `search-submit` only while a flag is up, mirroring the
// adversarial harness's shape (report, finding F-A3, probe P9).
test("a throw inside syncSearchGate's own call does not leak the gate shut for later writes", async () => {
  const elements = new Map();
  const realEl = (id) => {
    if (!elements.has(id)) {
      elements.set(id, makeElement());
    }
    return elements.get(id);
  };
  let throwing = false;
  const el = (id) => {
    if (id === "search-submit" && throwing) {
      throw new Error("element vanished");
    }
    return realEl(id);
  };

  const settings = () => ({
    key: { kind: "present" },
    platform: "mac",
    index: {
      kind: "read",
      activeSpace: 1,
      embeddedChunks: 8,
      totalChunks: 9,
      failedChunks: 0,
      embeddedChunksEverywhere: 8,
      embeddingModel: null,
      rerankModel: null,
      chatModel: null,
      searchTextArm: true,
      searchContentArm: true,
    },
  });
  const invoke = (command) => {
    if (command === "open_index") return Promise.resolve({ path: "/tmp/index", schemaVersion: 1 });
    if (command === "job_status") return Promise.resolve({ running: false });
    if (command === "model_settings") return Promise.resolve(settings());
    if (command === "provider_models") {
      return Promise.resolve({ entries: [], unreadable: 0, unreadableRecords: [] });
    }
    if (command === "forget_key") return Promise.resolve({ kind: "removed" });
    return Promise.resolve(null);
  };
  class Channel {
    constructor() {
      this.onmessage = null;
    }
  }

  globalThis.window = {
    __TAURI__: { core: { invoke, Channel }, dialog: { open: async () => null } },
  };
  globalThis.document = { getElementById: el, createElement: () => makeElement() };

  bootCount += 1;
  await import(`./main.js?window=${bootCount}`);
  await settleEverything();

  assert.equal(realEl("search-submit").disabled, false, "premise: submit starts enabled");

  // One write whose own `syncSearchGate()` throws at the increment.
  throwing = true;
  const clickPromise = realEl("forget").listeners.get("click")({});
  const caught = await clickPromise.then(
    () => null,
    (error) => error,
  );
  assert.match(String(caught), /element vanished/, "premise: the handler threw, same as the harness");
  throwing = false;
  await settleEverything();

  // A later, entirely healthy write must still be able to shut the gate and
  // reopen it — if the earlier throw leaked the counter, this one settling
  // will find it already shut and unable to ever come back.
  realEl("forget").listeners.get("click")({});
  await settleEverything();
  assert.equal(
    realEl("search-submit").disabled,
    false,
    "a throw inside an earlier syncSearchGate() call leaked the counter and shut the gate for " +
      "the rest of the session",
  );
});

// F3, adapted for U-3 (adversarial pass, F-A6). This test used to hold two
// searches open at once to pin their render order — exactly the capability
// U-3 removes: a submit while one is in flight is now refused before it
// ever reaches `search()` (see the U-3 tests below). What F3's guard still
// protects, on the one search that is now ever in flight: the refused
// submit must draw nothing, and the search that actually ran must draw its
// own real answer once it settles — not left blank, not overwritten.
test("a submit refused while a search is in flight draws nothing until that search settles (F3, U-3)", async () => {
  const w = await boot();

  const first = w.hold("search");
  w.el("query").value = "first query";
  const submitA = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });

  // Refused: `first` is still out. No second `invoke("search")`, so nothing
  // here has an answer to draw from.
  w.el("query").value = "second query";
  const submitB = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await submitB;
  await settleEverything();
  assert.equal(
    w.el("text-arm-state").textContent,
    "",
    "a refused submit drew something before the one search in flight had answered",
  );

  first.resolve({
    hits: [{ relativePath: "a.txt", text: "a" }],
    text: { kind: "answered", matched: 5 },
    content: { kind: "answered", matched: 7, embedded: 10, total: 10, reachable: 10 },
  });
  await submitA;
  await settleEverything();

  assert.equal(w.el("text-arm-state").textContent, "Search by text returned 5.");
  assert.equal(w.el("results").options.length, 1);
  assert.equal(w.el("results").options[0].options[1].textContent, "a");
});

// Review, V-1: `searchDrawn = issue` used to be assigned *before* the
// render, so a render that itself throws lands in this same handler's own
// `catch` with `issue <= searchDrawn` already true (they are equal) — and
// the failure is swallowed instead of reported. Not reachable from the
// product today (`bridge.rs` always serialises real `hits`), which is why
// this needs a malformed answer to reach at all, the same category as U-2.
// Pinned rather than only reasoned about, per this branch's own rule that a
// test which reads code proves its shape, not its premise.
test("a render that throws after a successful answer still reports the failure (V-1)", async () => {
  const w = await boot({
    search: () => ({
      hits: null,
      text: { kind: "answered", matched: 0 },
      content: { kind: "answered", matched: 0, embedded: 0, total: 0, reachable: 0 },
    }),
  });

  w.el("query").value = "malformed";
  await w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.el("results").options.length,
    1,
    "a render failure left nothing on screen — searchDrawn advanced before the render, so the " +
      "throw looked like a stale answer to the catch block and was swallowed",
  );
  assert.match(w.el("results").options[0].textContent, /search failed/);
});

// Adversarial pass, F-A6/U-3: nothing stopped a second submit while a search
// was already in flight, so two concurrent searches sent two paid
// `POST /embeddings` requests (report, finding F-A6) — money spent on an
// answer the window discards by design, since only the newest may render.
// The controller's ruling: coalesce. One query field, one button; a second
// submit must not reach the provider while the first is still out.
const emptyAnswer = () => ({
  hits: [],
  text: { kind: "answered", matched: 0 },
  content: { kind: "answered", matched: 0, embedded: 0, total: 0, reachable: 0 },
});

test('a submit while a search is in flight does not send a second invoke("search") (U-3)', async () => {
  const w = await boot();

  const first = w.hold("search");
  w.el("query").value = "first query";
  const submitA = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.calls.filter((c) => c === "search").length,
    1,
    "premise: the first submit reached the provider",
  );

  w.el("query").value = "second query";
  const submitB = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.calls.filter((c) => c === "search").length,
    1,
    "a second submit sent a second paid provider request while the first was still in flight",
  );

  first.resolve(emptyAnswer());
  await Promise.all([submitA, submitB]);
  await settleEverything();
});

// The success path must reopen the gate — otherwise the first search's own
// coalescing would refuse every later submit for the rest of the session.
test("a settled, successful search reopens the gate for the next submit (U-3)", async () => {
  const w = await boot();

  const first = w.hold("search");
  w.el("query").value = "first query";
  const submitA = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  first.resolve(emptyAnswer());
  await submitA;
  await settleEverything();

  const second = w.hold("search");
  w.el("query").value = "second query";
  const submitB = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.calls.filter((c) => c === "search").length,
    2,
    "a successful search left the gate shut, refusing the very next submit",
  );
  second.resolve(emptyAnswer());
  await submitB;
  await settleEverything();
});

// The failure path — round 3's own bug (F3) lived exactly here, in a
// handler that reopened correctly on success and not on rejection.
test("a settled, failed search reopens the gate for the next submit (U-3)", async () => {
  const w = await boot();

  const first = w.hold("search");
  w.el("query").value = "first query";
  const submitA = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  first.reject(new Error("the index could not be reached"));
  await submitA;
  await settleEverything();

  const second = w.hold("search");
  w.el("query").value = "second query";
  const submitB = w.el("search-form").listeners.get("submit")({ preventDefault: () => {} });
  await settleEverything();

  assert.equal(
    w.calls.filter((c) => c === "search").length,
    2,
    "a failed search left the gate shut, refusing the very next submit",
  );
  second.resolve(emptyAnswer());
  await submitB;
  await settleEverything();
});
