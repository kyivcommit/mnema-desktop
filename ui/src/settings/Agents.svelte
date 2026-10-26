<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import {
    agentAccessEnabled, setAgentAccess, agentLog, agentClients, applyAgentClients,
    type AgentLogEntry, type Change, type ClientId, type ClientState,
  } from '../lib/ipc';

  // The «Agents» section: the access switch, the four agents it connects, and
  // what agents have asked. The section is torn down by every nav click, so the
  // poll below lives exactly as long as it is visible — and the state read on
  // mount lives no longer than that either.

  const POLL_MS = 2000;

  const OPS = ['search', 'passage', 'status', 'rescan'] as const;
  // An op this window does not know (a newer app, an older window) shows as the app sent it.
  const opLabel = (op: string) => (OPS as readonly string[]).includes(op) ? t(`agents_op_${op}` as Key) : op;

  // The four agents, in the order `agent_clients.rs` answers them.
  const CLIENTS: readonly ClientId[] = ['claude', 'codex', 'pi', 'opencode'];
  // Product names, untranslated: the same four words in both languages.
  const CLIENT_NAME: Record<ClientId, Key> = {
    claude: 'agents_client_claude', codex: 'agents_client_codex',
    pi: 'agents_client_pi', opencode: 'agents_client_opencode',
  };

  let on = $state(false);
  let log = $state<AgentLogEntry[]>([]);
  let failure = $state<string | null>(null);
  let busy = $state(false);

  // What each CLI said about the `mnema` server, or null until the first answer
  // arrives. Read on mount and after every Apply, never derived from what was
  // asked for: one client's refusal says nothing about the other three.
  let states = $state<ClientState[] | null>(null);
  // The person's own pick, per client, while they have one. Cleared when a
  // re-read answers, so a box goes back to the record that refused it instead of
  // staying where the person put it.
  let choice = $state<Partial<Record<ClientId, boolean>> | null>(null);
  // One sentence per client `apply_agent_clients` answered with an error, and
  // whether that client was being connected or disconnected.
  let failures = $state<{ id: ClientId; connecting: boolean; error: string }[]>([]);
  let applying = $state(false);
  // An answer this window did not get at all, verbatim — from the read and from
  // the press, kept apart because they name different things.
  let readFailure = $state<string | null>(null);
  let applyRejected = $state<string | null>(null);

  const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

  // The socket's real state, never the value we last asked for: a refused start
  // leaves it closed, and the switch must say so.
  async function readEnabled() {
    try { on = await agentAccessEnabled(); } catch (err) { failure = message(err); }
  }

  async function readClients() {
    try {
      states = await agentClients();
      readFailure = null;
      // An answer is a new state, and the person's pick was made against the
      // old one: a box this read now disables must not keep a tick it can no
      // longer justify. A FAILED read confirms nothing and leaves the pick
      // alone — that is the one case where the boxes are still the person's.
      choice = null;
    } catch (err) {
      // The previous answer, if any, stays on screen: it was true a moment ago.
      readFailure = message(err);
    }
  }

  async function toggle(e: Event) {
    const box = e.currentTarget as HTMLInputElement;
    const want = box.checked;
    failure = null;
    busy = true;
    try { await setAgentAccess(want); } catch (err) { failure = message(err); }
    await readEnabled();
    busy = false;
    // `on` may not have changed (a refused start), so Svelte would leave the
    // box the person just ticked; put it back to what the socket says.
    box.checked = on;
    if (on) void readClients();
  }

  async function pollLog() {
    try { log = await agentLog(); } catch { /* keep the last list */ }
  }

  onMount(() => {
    void readEnabled().then(() => { if (on) void readClients(); });
  });

  // Only while access is on: closing the window hides it, it does not unmount
  // the section, and an off switch has nothing to log.
  $effect(() => {
    if (!on) return;
    void pollLog();
    const timer = setInterval(() => void pollLog(), POLL_MS);
    return () => clearInterval(timer);
  });

  // Ticked means the CLI holds a record of `mnema` at all — the spec's own
  // "connected" — not that the record names this build: an outdated path is
  // ticked, and Apply is what rewrites it.
  const connected = (state: ClientState | undefined) =>
    state !== undefined && state.link !== null && state.link.kind !== 'absent';

  const rows = $derived.by(() => {
    void $locale;
    return CLIENTS.map((id) => {
      const state = states?.find((one) => one.id === id);
      const checked = choice?.[id] ?? connected(state);
      // Nothing is offered until the first answer arrives, and a client the
      // shell could not even look for is not a client this window may touch.
      const disabled = applying || state === undefined || !state.installed || state.error !== null;
      return {
        id,
        name: t(CLIENT_NAME[id]),
        state,
        checked,
        disabled,
        // The one fact Apply is sent: a box the person moved, and which way.
        // A disabled row is never a change — its state is unknown, and "unknown"
        // must not reach the backend as "disconnect it". A ticked stale record
        // is a change with nothing moved: only an add rewrites it.
        change: !disabled
          && (checked !== connected(state) || (checked && state.link?.kind === 'stale')),
        note: state === undefined || state.error !== null ? null
          : !state.installed ? t('agents_not_installed')
            : state.link?.kind === 'stale' ? t('agents_stale')
              : null,
      };
    });
  });

  // Apply is owed only when some box carries a change — see `change` above.
  const canApply = $derived(states !== null && !applying && rows.some((row) => row.change));

  const failureOf = $derived(new Map(failures.map((one) => [one.id, one])));
  const failureText = $derived.by(() => {
    void $locale;
    return new Map(failures.map((one) => [one.id, t(
      one.connecting ? 'agents_apply_failed' : 'agents_disconnect_failed',
      { name: t(CLIENT_NAME[one.id]) },
    )]));
  });

  function pick(id: ClientId, checked: boolean) {
    choice = { ...(choice ?? {}), [id]: checked };
  }

  async function runApply() {
    if (!canApply) return;
    const changes: Change[] = rows
      .filter((row) => row.change)
      .map((row) => ({ id: row.id, connect: row.checked }));
    applying = true;
    failures = [];
    applyRejected = null;
    // Every id an answer can carry is one this request named — `apply` reads,
    // runs and answers only listed clients — so this map is total by
    // construction; the default covers a reply the contract does not allow.
    const connectOf = new Map(changes.map((change) => [change.id, change.connect]));
    try {
      const applied = await applyAgentClients(changes);
      failures = applied.flatMap((one) => one.error === null ? []
        : [{ id: one.id, connecting: connectOf.get(one.id) ?? false, error: one.error }]);
    } catch (err) {
      // Set HERE, not after the re-read: the press's own sentence must not wait
      // behind four CLI reads (up to about twenty seconds) or go unseen if one
      // never settles. A command that never answered changed nothing this
      // window knows of; the re-read below still draws the record.
      applyRejected = message(err);
    }
    await readClients();
    applying = false;
  }

  const accessLabel = $derived.by(() => { void $locale; return t('agents_access_label'); });
  const clientsHeading = $derived.by(() => { void $locale; return t('agents_clients_heading'); });
  const applyLabel = $derived.by(() => { void $locale; return t('agents_apply'); });
  const detailsLabel = $derived.by(() => { void $locale; return t('agents_details'); });
  const readFailedLabel = $derived.by(() => { void $locale; return t('agents_read_failed'); });
  const applyRejectedLabel = $derived.by(() => { void $locale; return t('agents_apply_rejected'); });
  const checkingLabel = $derived.by(() => { void $locale; return t('agents_checking'); });
</script>

<label class="agents-switch">
  <input type="checkbox" data-testid="agents-access" checked={on} disabled={busy} onchange={toggle} /> {accessLabel}
</label>
{#if failure}
  <p role="alert" data-testid="agents-failure">{#key $locale}{t('agents_access_failed')}{/key} {failure}</p>
{/if}

{#if on}
  <div role="group" aria-labelledby="agents-clients-heading">
    <h3 id="agents-clients-heading">{clientsHeading}</h3>
    <!-- Before the first answer an empty box would read as "not connected"
         (owner, live stand 2026-10-26): say it is being checked instead. -->
    {#if states === null && readFailure === null}
      <p class="fc" data-testid="agents-checking">{checkingLabel}</p>
    {:else}
    {#each rows as row (row.id)}
      <label class="agents-switch">
        <input
          type="checkbox"
          data-testid={`agents-client-${row.id}`}
          checked={row.checked}
          disabled={row.disabled}
          onchange={(e) => pick(row.id, e.currentTarget.checked)}
        /> {row.name}
      </label>
      {#if row.state?.error}
        <details data-testid={`agents-error-${row.id}`}>
          <summary>{detailsLabel}</summary>
          <p>{row.state.error}</p>
        </details>
      {:else if row.note}
        <p class="fc" data-testid={`agents-note-${row.id}`}>{row.note}</p>
      {/if}
      {#if failureOf.has(row.id)}
        <p role="alert" data-testid={`agents-failed-${row.id}`}>{failureText.get(row.id)}</p>
        <details data-testid={`agents-failed-details-${row.id}`}>
          <summary>{detailsLabel}</summary>
          <p>{failureOf.get(row.id)?.error}</p>
        </details>
      {/if}
    {/each}
    <button type="button" data-testid="agents-apply" disabled={!canApply} onclick={runApply}>{applyLabel}</button>
    {/if}
  </div>
  {#if applyRejected}
    <p role="alert" data-testid="agents-apply-rejected">{applyRejectedLabel} {applyRejected}</p>
  {/if}
  {#if readFailure}
    <p role="alert" data-testid="agents-read-failed">{readFailedLabel} {readFailure}</p>
  {/if}
{/if}

<h3>{#key $locale}{t('agents_log_heading')}{/key}</h3>
{#if log.length === 0}
  <p data-testid="agents-log-empty">{#key $locale}{t('agents_log_empty')}{/key}</p>
{:else}
  <ul data-testid="agents-log">
    {#each log as entry, i (i)}
      <li>
        <time datetime={entry.at}>{new Date(entry.at).toLocaleString($locale)}</time>
        <span>{#key $locale}{opLabel(entry.op)}{/key}</span>
        {#if entry.query}<span>{entry.query}</span>{/if}
        {#if entry.passages != null}<span>{#key $locale}{t('agents_log_passages', { n: entry.passages })}{/key}</span>{/if}
        {#if entry.client}<span>{#key $locale}{t('agents_log_client', { client: entry.client })}{/key}</span>{/if}
      </li>
    {/each}
  </ul>
{/if}
