// The webview half of the walking skeleton. It draws what the core reports
// and decides nothing: no state here outlives a reload, because the core
// owns it. `render.js` is where every sentence is actually built — pure
// functions, importable and tested (`render.test.js`) without a browser —
// and everything below is DOM: elements, listeners, `invoke`.

import { endingSentence, searchResultItems } from "./render.js";

const { invoke, Channel } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;

const el = (id) => document.getElementById(id);
const results = el("results");

// Opening the index is the first thing that happens, and its failure is
// something the user has to be able to read — which is why the window opens
// before the database does.
try {
  const info = await invoke("open_index");
  el("index-status").textContent = `index ready at ${info.path} (schema v${info.schemaVersion})`;
} catch (error) {
  el("index-status").textContent = `the index could not be opened: ${error}`;
}

// `null` until `pick` answers with a real one. Kept apart from `jobRunning`
// below because the two gate "Index it" for different reasons — one because
// nothing has been chosen yet, the other because something is already
// running — and conflating them would make a reload (which loses this, but
// not necessarily a running job) look identical to "no folder chosen" for
// the wrong reason.
let watchedRootId = null;
let jobRunning = false;

const syncButtons = () => {
  el("walk").disabled = jobRunning || watchedRootId === null;
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
    el("job-status").textContent =
      "the job is no longer running, but this page has no channel to it and does not " +
      "know how it ended — whether it finished cleanly, or something was left unreconciled";
  }
};

// Never leaves the buttons disabled. If the core cannot be reached, Cancel is
// simply left disabled (nothing is known to be running) rather than the page
// having nothing left to press.
const follow = () =>
  followUntilIdle().catch((error) => {
    jobRunning = false;
    syncButtons();
    el("job-status").textContent = `lost track of the job: ${error}`;
  });

try {
  const { running } = await invoke("job_status");
  jobRunning = running;
  syncButtons();
  if (running) {
    el("job-status").textContent = "a job started before this page loaded is still running";
    follow();
  }
} catch (error) {
  jobRunning = false;
  syncButtons();
  el("job-status").textContent = `${error}`;
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
    // "Index it" from being disabled forever.
    const ending = data;
    el("bar").max = ending.total;
    el("bar").value = ending.done;

    el("job-status").textContent = endingSentence(ending);
    endingDescribed = true;
    jobRunning = false;
    syncButtons();

    if (watchedRootId !== null) {
      renderSkips(watchedRootId);
    }
  };

  if (watchedRootId === null) {
    el("job-status").textContent = "choose a folder above before indexing";
    return;
  }

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
    // so the buttons must not move.
    el("job-status").textContent = `${error}`;
  }
});

el("cancel").addEventListener("click", async () => {
  // Only the button is disabled here. "Index it" comes back when the job says
  // it has ended, not when the user asks it to.
  el("cancel").disabled = true;
  el("job-status").textContent = "stopping…";
  try {
    await invoke("cancel_job");
  } catch (error) {
    // The request never reached the core, so whatever is running is still
    // running. Left disabled forever, and silent about it, "stopping…"
    // would read as true when it is not — this is the honest alternative:
    // the job is presumably still going, and the button is worth pressing
    // again.
    el("job-status").textContent = `could not ask the job to stop: ${error}`;
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
// the list is made of, this only turns that into elements.
async function search(query) {
  const hits = await invoke("search", { query });
  results.replaceChildren(...searchResultItems(hits).map((item) => {
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
}

el("search-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const query = el("query").value;
  try {
    await search(query);
  } catch (error) {
    const li = document.createElement("li");
    li.textContent = `search failed: ${error}`;
    results.replaceChildren(li);
  }
});
