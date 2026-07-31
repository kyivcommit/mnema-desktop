// The webview half of the walking skeleton. It draws what the core reports and
// decides nothing: no state here outlives a reload, because the core owns it.

const { invoke, Channel } = window.__TAURI__.core;

const el = (id) => document.getElementById(id);

// Opening the index is the first thing that happens, and its failure is
// something the user has to be able to read — which is why the window opens
// before the database does.
try {
  const info = await invoke("open_index");
  el("index-status").textContent = `index ready at ${info.path} (schema v${info.schemaVersion})`;
} catch (error) {
  el("index-status").textContent = `the index could not be opened: ${error}`;
}

el("search-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = el("query").value;
  try {
    // `search` replaced `lexical_search`: the same lexical arm, but each hit
    // is now a citation — text, where it came from — rather than a bare
    // chunk id with nowhere for the window to take it.
    const hits = await invoke("search", { query });
    el("search-status").textContent = hits.length
      ? `${hits.length} hit(s): ${hits.map((hit) => hit.relativePath ?? "(no path)").join(", ")}`
      : "no matches";
  } catch (error) {
    el("search-status").textContent = `search failed: ${error}`;
  }
});

const setRunning = (running) => {
  el("start").disabled = running;
  el("cancel").disabled = !running;
};

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
// fixes: a job over 40 quarter-seconds cannot do that, a job over an empty
// folder finishes in less than one IPC round trip.
const followUntilIdle = async () => {
  while ((await invoke("job_status")).running) {
    await new Promise((wake) => setTimeout(wake, 500));
  }
  setRunning(false);
  if (!endingDescribed) {
    el("job-status").textContent = "the job has finished";
  }
};

// Never leaves the buttons disabled. If the core cannot be reached, Start is
// enabled and a click is answered either by starting or by "a job is already
// running" — both recoverable, unlike a window with nothing left to press.
const follow = () =>
  followUntilIdle().catch((error) => {
    setRunning(false);
    el("job-status").textContent = `lost track of the job: ${error}`;
  });

try {
  const { running } = await invoke("job_status");
  setRunning(running);
  if (running) {
    el("job-status").textContent = "a job started before this page loaded is still running";
    follow();
  }
} catch (error) {
  setRunning(false);
  el("job-status").textContent = `${error}`;
}

el("start").addEventListener("click", async () => {
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
      const eta = secondsLeft === null ? "estimating…" : `${secondsLeft}s left`;
      el("job-status").textContent = `${done} of ${total}, ${skipped} skipped — ${eta}`;
      return;
    }

    // The core says how the job ended; the page does not decide. Cancelling is
    // a request, and between the request and the stop there is real work still
    // finishing — a page that printed "cancelled" on the click would be stating
    // as fact something it had not been told.
    //
    // This arrives however the job ended, a panic included, which is what keeps
    // Start from being disabled forever.
    const { reason, done, total } = data;
    el("bar").max = total;
    el("bar").value = done;
    el("job-status").textContent =
      {
        completed: `finished ${total} of ${total}`,
        cancelled: `stopped after ${done} of ${total}`,
        failed: `failed after ${done} of ${total}`,
        // A reason this page does not know is still an ending. Rendering the
        // literal `undefined` would be the page inventing a word.
      }[reason] ?? `ended (${reason}) after ${done} of ${total}`;
    endingDescribed = true;
    setRunning(false);
  };

  try {
    endingDescribed = false;
    await invoke("start_probe_job", { onProgress });
    setRunning(true);
    // Even here, where this page owns the channel. `setRunning(true)` runs after
    // the await, so an ending that arrived first has already been overwritten by
    // the line above, and only the core can put it right.
    follow();
  } catch (error) {
    // Refused — most likely because a job is already running. Nothing started,
    // so the buttons must not move.
    el("job-status").textContent = `${error}`;
  }
});

el("cancel").addEventListener("click", async () => {
  // Only the button is disabled here. Start comes back when the job says it has
  // ended, not when the user asks it to.
  el("cancel").disabled = true;
  el("job-status").textContent = "stopping…";
  await invoke("cancel_job");
});
