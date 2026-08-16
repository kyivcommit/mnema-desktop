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
      content: { kind: "answered", matched: 7, embedded: 10, total: 10 },
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
