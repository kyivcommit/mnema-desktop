<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { ask, chatTools, continueHistory, historyEntry, newChat, openSettings, modelSettings, providerStatus, type ProviderChoice, type ProviderStatus, type RoleStatuses } from '../lib/ipc';
  import { checkQuery, roleReady, roleServed, heatAfter, stateFromAnswer, turnsAfter, type Heat, type LauncherState, type Turn } from './state';
  import Toolbar from './Toolbar.svelte';
  import SearchLine from './SearchLine.svelte';
  import Cards from './Cards.svelte';

  let query = $state('');
  let echo = $state('');
  // `$state.raw`, here and for `previous`: `Cards` finds a turn by the identity of its
  // answer, and a deep proxy of the same object is not `===` to it.
  let launcherState = $state.raw<LauncherState>({ kind: 'idle' });
  // The answer a hot launcher was showing when the current submit started. Any
  // error puts it back (a too-long line, a failed ask): the error is
  // told in the search line, and the person keeps what they were reading. Null
  // when there was none, and for every submit that did not start from a hot
  // launcher's answer.
  let previous = $state.raw<LauncherState | null>(null);
  // Recents: bumped whenever the list of questions may have changed (a recorded
  // answer, a gone entry); the tree re-reads the list on each change.
  let historyGen = $state(0);
  // The row of the question on screen, when it was opened from Recents. Null for
  // a question asked from the search line.
  let historyId = $state<number | null>(null);
  // What the cards draw. The search line always gets `launcherState` itself.
  const cardsState = $derived(
    launcherState.kind === 'error' && previous
      ? previous
      : launcherState,
  );
  // The conversation on screen, oldest first (the backend keeps the same session).
  // `$state.raw`: `Cards` finds a turn by the identity of its answer.
  let turns = $state.raw<Turn[]>([]);
  // The «Tool support» switch (ruling R4): off keeps the launcher at one turn
  // and hides «New chat». Read like the provider status, on mount and on focus.
  let chatOn = $state(false);
  let status = $state<ProviderStatus | null>(null);
  // Which provider the status is about: the two are written together.
  let statusProvider = $state<ProviderChoice>('openRouter');
  // Cold: the search column alone. The first answer with something to show
  // makes it hot and opens both side panels; after that the person's choice
  // stands across questions.
  let heat = $state<Heat>('cold');
  let left = $state(false);
  let right = $state(false);

  // What is on screen, not what was asked for: cold shows no side panel
  // whatever the switches say, and the switches themselves are left alone.
  const showLeft = $derived(heat === 'hot' && left);
  const showRight = $derived(heat === 'hot' && right);
  const cols = $derived(`${showLeft ? 'l' : ''}s${showRight ? 'r' : ''}`);

  // Under the Mnema provider the backend keeps no conversation, whatever the switch says.
  const dialogue = $derived(chatOn && statusProvider !== 'mnema');
  const newChatLabel = $derived.by(() => { void $locale; return t('new_chat'); });

  // Read on mount and again whenever the window gains focus: the key and the
  // model are changed in another window, and the launcher is hidden, not
  // closed, so nothing else would tell it. Non-fatal: on failure the cloud keeps
  // its last state — log, do not swallow.
  // A later call supersedes an earlier one still in flight: a slow probe that
  // lands after a newer answer must not put the old state back on the cloud.
  let providerGen = 0;
  function refreshProvider() {
    const gen = ++providerGen;
    Promise.all([providerStatus(), modelSettings().catch(() => null)])
      .then(([backend, settings]) => {
        if (gen !== providerGen) return;
        const roles = settings?.roles;
        // The backend's `ok` is a probe of the provider; whether the ROLE is set up
        // is `roleReady`, the same rule the settings tab dots use. A failed
        // settings read is not ready.
        const effective = (role: keyof RoleStatuses): ProviderStatus => {
          const probe = backend[role];
          if (probe.kind !== 'ok' || (settings && roleReady(settings, role))) return probe;
          const missing = !settings || !roles?.[role] ? 'role'
            : !roleServed(settings, role) ? (roles[role] === 'openRouter' ? 'key' : 'localModels')
              : role === 'chat' ? 'chatModel' : 'embeddingModel';
          return { kind: 'notConfigured', missing };
        };
        const s: RoleStatuses = { embed: effective('embed'), chat: effective('chat') };
        // Answers need the chat role and search the embed role: the cloud shows
        // the first of the two that is not ok.
        const role: keyof RoleStatuses = s.chat.kind !== 'ok' || s.embed.kind === 'ok' ? 'chat' : 'embed';
        status = s[role];
        statusProvider = roles?.[role] ?? 'openRouter';
      })
      .catch((e) => console.error('provider_status failed', e));
  }

  function refreshChatTools() {
    chatTools().then((on) => { chatOn = on === true; }).catch((e) => console.error('chat_tools_enabled failed', e));
  }
  function refreshAll() { refreshProvider(); refreshChatTools(); }

  // Every call that changes the backend session (ask, new chat, continue a
  // stored one) goes through this chain, so they arrive in the order the person
  // acted whatever order their own answers take.
  let sessionOps: Promise<unknown> = Promise.resolve();
  function inSession<T>(call: () => Promise<T>): Promise<T> {
    const run = sessionOps.then(call, call);
    sessionOps = run.catch(() => {});
    return run;
  }

  async function startNewChat() {
    if (launcherState.kind === 'inFlight') return;
    // The backend first: a screen that says "new" while the session goes on would lie.
    try { await inSession(newChat); } catch (e) { console.error('new_chat failed', e); return; }
    screenGen += 1; // an open that is still reading does not draw over the empty screen
    // Only the conversation clears; heat and the panels stay as the person left them.
    turns = [];
    launcherState = { kind: 'idle' };
    previous = null;
    historyId = null;
    echo = '';
  }

  onMount(() => {
    refreshAll();
  });

  // The owner validates and calls ask — the whole machine goes through
  // state.ts. A rejected ask becomes a visible error, never a silent reset:
  // an eaten error is easy to miss.
  // Bumped by every submit and every open: a Recents read that started before
  // the screen changed hands does not draw over what is on it now.
  let screenGen = 0;

  async function runSearch(raw: string) {
    if (launcherState.kind === 'inFlight') return; // one ask at a time
    // Enter on an empty line does nothing, in every state (owner, 2026-10-08).
    if (raw.trim() === '') return;
    screenGen += 1;
    // What is on screen, not what the machine holds: after an error the
    // machine says `error` while the restored answer is still showing, and a
    // second error must restore that same answer again.
    const shown = cardsState;
    previous = heat === 'hot' && (shown.kind === 'generated' || shown.kind === 'citationsOnly') ? shown : null;
    const shownEcho = echo;
    const check = checkQuery(raw);
    // A rejected line keeps the echo: the answer it belongs to stays drawn.
    if (!check.ok) { launcherState = { kind: 'error', reason: check.reason }; return; }
    echo = '';
    if (!dialogue) turns = []; // one turn at a time: the answer on screen is replaced
    launcherState = { kind: 'inFlight', query: check.query };
    try {
      const answer = await inSession(() => ask(check.query));
      launcherState = stateFromAnswer(check.query, answer);
      turns = turnsAfter(turns, check.query, answer);
      // Whatever answer lands, it is not an opened question, refusals included.
      historyId = null;
      // The backend records a generated or citations-only answer inside `ask`, so
      // the list has a new row. A refusal records nothing, so it does not reread it.
      if (answer.kind === 'generated' || answer.kind === 'citationsOnly') historyGen += 1;
      // Only the cold-to-hot step opens the panels; a hot launcher keeps
      // whichever the person switched off.
      const warmed = heat === 'cold' && heatAfter(heat, launcherState) === 'hot';
      if (warmed) { heat = 'hot'; left = right = true; }
      // §7: line clears on ready — but only if it still holds the submitted
      // query. A draft typed while the ask was in flight is kept, not wiped
      // (Codex #3). D171: an answer the provider failed to give keeps the
      // query too, so asking again is one Enter away — the remedy for a lost
      // network or a silent model, and harmless for a refused key.
      const failed = answer.kind === 'citationsOnly' && answer.why.kind !== 'notAsked';
      if (query === raw && !failed) query = '';
      // The provider just failed this ask: the cloud asks again rather than
      // keep showing a cached "ok" beside the failure (D171 review, finding 2;
      // the backend dropped that cache for a network failure).
      if (failed) refreshProvider();
      // §7: the query echoes as a chat bubble. The bubble itself is drawn by
      // `Answer` inside the centre card (Task 8b) — the launcher used to draw a
      // second one of its own here, and in state B both were on screen at once.
      echo = check.query;
    } catch (e) {
      console.error('ask failed', e); // query stays in the line for a retry
      launcherState = { kind: 'error', reason: 'askFailed' };
      if (previous) echo = shownEcho; // the restored answer's own question
    }
  }

  // Recents: a question asked before, drawn from its stored snapshot. No `ask` and
  // no network, and the search line is left alone. Same one-at-a-time rule as a
  // search: a click while an ask is in flight does nothing.
  async function openHistory(id: number) {
    if (launcherState.kind === 'inFlight') return;
    // Kept as `runSearch` keeps it: an error puts back what was on screen.
    const shown = cardsState;
    previous = heat === 'hot' && (shown.kind === 'generated' || shown.kind === 'citationsOnly') ? shown : null;
    const screenAt = ++screenGen;
    // A search or another open that began while the entry was read owns the
    // screen now; the entry is dropped.
    const superseded = () => screenAt !== screenGen;
    try {
      const opened = await historyEntry(id);
      if (superseded()) return;
      if (opened.kind === 'gone') { historyGen += 1; return; }
      if (opened.kind === 'unreadable') { launcherState = { kind: 'error', reason: 'historyUnreadable' }; return; }
      // The backend continues this conversation only if the screen is still to
      // draw it: the check runs when the call's turn in the chain comes.
      if (dialogue) {
        let skipped = false;
        await inSession(() => { if (superseded()) { skipped = true; return Promise.resolve(false); } return continueHistory(id); });
        if (skipped || superseded()) return;
      }
      const last = opened.turns[opened.turns.length - 1];
      launcherState = stateFromAnswer(last.query, last.answer);
      turns = opened.turns.reduce<Turn[]>((acc, t) => turnsAfter(acc, t.query, t.answer), []);
      const warmed = heat === 'cold' && heatAfter(heat, launcherState) === 'hot';
      if (warmed) { heat = 'hot'; left = right = true; }
      echo = last.query;
      historyId = id;
    } catch (e) {
      if (superseded()) return;
      console.error('history_entry failed', e);
      launcherState = { kind: 'error', reason: 'historyFailed' };
    }
  }

  // ⌘, on macOS, Ctrl+, elsewhere. The launcher's own menu is the app menu
  // now (§1: the policy is `Regular`), and it opens its own settings section;
  // this key is the launcher's shortcut to the same place.
  function onKeydown(event: KeyboardEvent) {
    if (event.key === ',' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      openSettings().catch((e) => console.error('open_settings failed', e));
    }
  }
</script>

<svelte:window onfocus={refreshAll} onkeydown={onKeydown} />

<main class="panels" data-cols={cols}>
  <!-- D155: the search panel is the drag handle. "deep" drags from any
       click inside it except the input, and the toolbar buttons — Tauri's
       own drag script skips clickable tags. The other panels select text. -->
  <Cards state={cardsState} query={echo} {left} {right} {heat} {historyId} {historyGen} onOpenHistory={openHistory} {turns}>
    {#snippet search()}
    <div class="searchbar" data-tauri-drag-region="deep">
      <div class="sb-row">
        <SearchLine bind:query state={launcherState} onSubmit={runSearch} />
        {#if dialogue}
          <button type="button" class="new-chat" data-testid="new-chat" disabled={launcherState.kind === 'inFlight'} onclick={startNewChat}>{newChatLabel}</button>
        {/if}
      </div>
      <div class="sb-tools">
        <Toolbar {heat} bind:left bind:right {status} provider={statusProvider} />
      </div>
    </div>
    {/snippet}
  </Cards>
</main>
