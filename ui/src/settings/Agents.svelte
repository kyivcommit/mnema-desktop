<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import {
    agentAccessEnabled, setAgentAccess, agentLog, agentCommandPath, type AgentLogEntry,
  } from '../lib/ipc';

  // The «Agents» section: the access switch, the line that sets an agent up, and
  // what agents have asked. The section is torn down by every nav click, so the
  // poll below lives exactly as long as it is visible.

  const POLL_MS = 2000;

  let on = $state(false);
  let path = $state('');
  let log = $state<AgentLogEntry[]>([]);
  let failure = $state<string | null>(null);

  // The socket's real state, never the value we last asked for: a refused start
  // leaves it closed, and the switch must say so.
  async function readEnabled() {
    try { on = await agentAccessEnabled(); } catch { on = false; }
  }

  async function toggle(e: Event) {
    const box = e.currentTarget as HTMLInputElement;
    const want = box.checked;
    failure = null;
    try { await setAgentAccess(want); } catch (err) { failure = String(err); }
    await readEnabled();
    // `on` may not have changed (a refused start), so Svelte would leave the
    // box the person just ticked; put it back to what the socket says.
    box.checked = on;
  }

  async function pollLog() {
    try { log = await agentLog(); } catch { /* keep the last list */ }
  }

  onMount(() => {
    void readEnabled();
    agentCommandPath().then((p) => (path = p), () => {});
    void pollLog();
    const timer = setInterval(() => void pollLog(), POLL_MS);
    return () => clearInterval(timer);
  });

  const accessLabel = $derived.by(() => { void $locale; return t('agents_access_label'); });
  const cmd = $derived(`claude mcp add mnema -- "${path}" mcp`);
  const json = $derived(JSON.stringify({ mcpServers: { mnema: { command: path, args: ['mcp'] } } }));

  const copy = (text: string) => navigator.clipboard.writeText(text).catch(() => {});
</script>

<label class="agents-switch">
  <input type="checkbox" data-testid="agents-access" checked={on} onchange={toggle} /> {accessLabel}
</label>
{#if failure}
  <p role="alert" data-testid="agents-failure">{#key $locale}{t('agents_access_failed')}{/key} {failure}</p>
{/if}

{#if on}
  <div role="group" aria-label={$locale && t('agents_setup_cmd')}>
    <p>{#key $locale}{t('agents_setup_cmd')}{/key}</p>
    <code data-testid="agents-cmd">{cmd}</code>
    <button type="button" data-testid="agents-copy-cmd" onclick={() => copy(cmd)}>{#key $locale}{t('agents_copy')}{/key}</button>
    <p>{#key $locale}{t('agents_setup_json')}{/key}</p>
    <code data-testid="agents-json">{json}</code>
    <button type="button" data-testid="agents-copy-json" onclick={() => copy(json)}>{#key $locale}{t('agents_copy')}{/key}</button>
  </div>
{/if}

<h3>{#key $locale}{t('agents_log_heading')}{/key}</h3>
{#if log.length === 0}
  <p data-testid="agents-log-empty">{#key $locale}{t('agents_log_empty')}{/key}</p>
{:else}
  <ul data-testid="agents-log">
    {#each log as entry, i (i)}
      <li>
        <time datetime={entry.at}>{new Date(entry.at).toLocaleString()}</time>
        <span>{entry.op}</span>
        {#if entry.query}<span>{entry.query}</span>{/if}
        {#if entry.passages != null}<span>{#key $locale}{t('agents_log_passages', { n: entry.passages })}{/key}</span>{/if}
        {#if entry.client}<span>{#key $locale}{t('agents_log_client', { client: entry.client })}{/key}</span>{/if}
      </li>
    {/each}
  </ul>
{/if}
