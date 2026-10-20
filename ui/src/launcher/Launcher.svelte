<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { ask, chatTools, continueHistory, historyEntry, newChat, openSettings, modelSettings, providerStatus, type ProviderChoice, type ProviderStatus, type RoleStatuses } from '../lib/ipc';
  import { checkQuery, firstCard, roleReady, roleServed, stateFromAnswer, turnsAfter, type LauncherState, type Turn } from './state';
  import Toolbar from './Toolbar.svelte';
  import Transcript from './Transcript.svelte';
  import Composer from './Composer.svelte';
  import Tree from './Tree.svelte';
  import type { TreePick, TreeSelection } from './Tree.svelte';
  import FilePane from './FilePane.svelte';
  import Recents from './Recents.svelte';
  import Shell from './Shell.svelte';
  import { loadLayout, isOverlay, type PanelState } from './layout';
  import type { AskCitation, DocumentFocus, DocumentLocation, Hit } from '../lib/ipc';

  let query = $state('');
  // `$state.raw`, here and for `previous`: the transcript finds a turn by the
  // identity of its answer, and a deep proxy of the same object is not `===` to it.
  let launcherState = $state.raw<LauncherState>({ kind: 'idle' });
  // The answer a launcher was showing when the current submit started. Any
  // error puts it back (a too-long line, a failed ask): the error is told in
  // the composer, and the person keeps what they were reading. Null when there
  // was none.
  let previous = $state.raw<LauncherState | null>(null);
  // Recents: bumped whenever the list of questions may have changed (a recorded
  // answer, a gone entry); the sidebar re-reads the list on each change.
  let historyGen = $state(0);
  // The conversation on screen: the row an opened entry names, or the row `ask`
  // reports it wrote. Null for a refusal and for a fresh chat.
  let historyId = $state<number | null>(null);
  // §2/§3: the file the right pane shows. `rootId`/`relativePath` may be null
  // (a citation with no place on disk); the tree marks a row only when both
  // name one. `openFocus` is one citation's passage — the pane scrolls to it
  // and drops it through `onFocusHandled`, so it is scrolled to once.
  let openFile = $state<DocumentLocation | null>(null);
  let openFocus = $state<DocumentFocus | null>(null);
  // The tree's mark is the (root, path) pair, never `documentId` (spec §3):
  // one document can sit at several paths, and a key on the id would mark
  // every row that names it.
  let selected = $state<TreeSelection | null>(null);

  // A click in the tree opens the file at its start: no highlight, no scroll.
  function openAtStart(pick: TreePick) {
    openFile = { documentId: pick.documentId, rootId: pick.rootId, relativePath: pick.relativePath };
    openFocus = null;
    selected = { rootId: pick.rootId, relativePath: pick.relativePath };
  }

  // A citation — the answer's first one, or the card the person pressed —
  // opens the whole file with its passage marked (§3). The right area opens
  // itself on a CLICK; a TREE the person shut stays shut, so `panels.tree` is
  // untouched. An ANSWER switches the file inside the area without opening it
  // when opening would paint an overlay over the answer it came with (§2): a
  // 480px pane plus its scrim over the transcript and the composer hides the
  // thing the person just read, so there the person opens it.
  function showCitation(citation: AskCitation | Hit | null, fromAnswer = false) {
    if (citation === null) return;
    openFile = { documentId: citation.documentId, rootId: citation.rootId, relativePath: citation.relativePath };
    openFocus = { chunkId: citation.chunkId, ord: citation.ord, text: citation.text };
    // A citation with no place on disk has no row to move to, and the mark it
    // would replace with nothing is left where it is.
    if (citation.rootId !== null && citation.relativePath !== null) {
      selected = { rootId: citation.rootId, relativePath: citation.relativePath };
    }
    // `isOverlay` answers about an area that is OPEN, so the question is asked
    // of the layout as if it were.
    if (!fromAnswer || !isOverlay({ ...panels, right: true }, window.innerWidth)) {
      panels = { ...panels, right: true };
    }
  }

  // What the transcript draws. The composer always gets `launcherState` itself.
  const shownState = $derived(
    launcherState.kind === 'error' && previous
      ? previous
      : launcherState,
  );
  // The conversation on screen, oldest first (the backend keeps the same session).
  let turns = $state.raw<Turn[]>([]);
  // The «Tool support» switch (ruling R4): off keeps the launcher at one turn
  // and hides «New chat». Read like the provider status, on mount and on focus.
  let chatOn = $state(false);
  let status = $state<ProviderStatus | null>(null);
  // Which provider the status is about: the two are written together.
  let statusProvider = $state<ProviderChoice>('openRouter');
  // §2: the three toggles and the three widths, remembered across runs. Shell
  // clamps them to the window and owns the splitters.
  let panels = $state<PanelState>(loadLayout());

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
    // Only the conversation clears; the panels stay as the person left them.
    turns = [];
    launcherState = { kind: 'idle' };
    previous = null;
    historyId = null;
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
    const shown = shownState;
    previous = shown.kind === 'generated' || shown.kind === 'citationsOnly' ? shown : null;
    const check = checkQuery(raw);
    // A rejected line keeps the answer it belongs to on screen.
    if (!check.ok) { launcherState = { kind: 'error', reason: check.reason }; return; }
    if (!dialogue) turns = []; // one turn at a time: the answer on screen is replaced
    launcherState = { kind: 'inFlight', query: check.query };
    try {
      const reply = await inSession(() => ask(check.query));
      const answer = reply.answer;
      launcherState = stateFromAnswer(check.query, answer);
      turns = turnsAfter(turns, check.query, answer);
      // §3, "when the answer arrived": an answer with citations opens the right
      // panel on the FIRST one — the smallest `anchor`, not the model's own
      // list order. No citations: the panel keeps exactly what it was showing.
      const first = firstCard(answer);
      if (first !== null) showCitation(first, true);
      // The row this answer was written to, straight from the backend: a refusal
      // (or a write that failed) reports none, and then nothing is marked.
      historyId = reply.historyId;
      // The backend records a generated or citations-only answer inside `ask`, so
      // the list may have a new row. A refusal records nothing, so it does not
      // reread it.
      if (answer.kind === 'generated' || answer.kind === 'citationsOnly') historyGen += 1;
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
    } catch (e) {
      console.error('ask failed', e); // query stays in the line for a retry
      launcherState = { kind: 'error', reason: 'askFailed' };
    }
  }

  // Recents: a question asked before, drawn from its stored snapshot. No `ask` and
  // no network, and the composer is left alone. Same one-at-a-time rule as a
  // search: a click while an ask is in flight does nothing.
  async function openHistory(id: number) {
    if (launcherState.kind === 'inFlight') return;
    // Kept as `runSearch` keeps it: an error puts back what was on screen.
    const shown = shownState;
    previous = shown.kind === 'generated' || shown.kind === 'citationsOnly' ? shown : null;
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

<Shell bind:panels fileOpen={openFile !== null}>
  {#snippet transcript()}
    <Transcript {turns} state={shownState} onSelected={showCitation} />
    <Composer bind:value={query} state={launcherState} onSubmit={runSearch} />
  {/snippet}
  {#snippet recents()}
    <Recents activeId={historyId} gen={historyGen} onOpen={openHistory}
      onDelete={(id) => { if (id === historyId) historyId = null; }}
      error={launcherState.kind === 'error' && (launcherState.reason === 'historyUnreadable' || launcherState.reason === 'historyFailed') ? launcherState.reason : null} />
  {/snippet}
  {#snippet tree()}
    <Tree {selected} onPick={openAtStart} />
  {/snippet}
  {#snippet file()}
    {#if openFile !== null}
      <FilePane location={openFile} focus={openFocus} onFocusHandled={() => (openFocus = null)} />
    {/if}
  {/snippet}
  {#snippet tools()}
    {#if dialogue}
      <button type="button" class="new-chat" data-testid="new-chat" disabled={launcherState.kind === 'inFlight'} onclick={startNewChat}>{newChatLabel}</button>
    {/if}
    <Toolbar {status} provider={statusProvider} />
  {/snippet}
</Shell>
