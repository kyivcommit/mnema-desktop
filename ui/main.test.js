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

  // The ending arrives first — before the press's own `invoke` has come back.
  await w.send(channel, ending({ reason: "completed", done: 0, total: 0, refused: 0 }));
  assert.match(w.status(), /nothing was waiting/, "the run's own sentence is not on screen");

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
