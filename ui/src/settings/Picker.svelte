<script lang="ts">
  import type { Snippet } from 'svelte';
  import { tick } from 'svelte';
  import { matches } from './picker';

  type Option = { id: string; label: string; aside?: string; group?: string; disabled?: boolean };
  let {
    options, value, onchange, searchable = false, placeholder, disabled = false, labelledby, marker,
  }: {
    options: Option[]; value: string | null; onchange: (id: string) => void;
    searchable?: boolean; placeholder: string; disabled?: boolean; labelledby: string;
    marker?: Snippet<[Option]>;
  } = $props();

  const uid = `picker-${Math.random().toString(36).slice(2, 8)}`;
  let open = $state(false);
  let query = $state('');
  let active = $state<string | null>(null);
  let root: HTMLElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();

  const selected = $derived(options.find((o) => o.id === value));
  const visible = $derived(options.filter((o) => matches(query, `${o.label} ${o.aside ?? ''}`)));
  const enabled = $derived(visible.filter((o) => !o.disabled));
  // Consecutive options of one group share a section; ungrouped ones stay flat.
  const sections = $derived(
    visible.reduce<{ group?: string; items: Option[] }[]>((acc, o) => {
      const last = acc[acc.length - 1];
      if (last && last.group === o.group) last.items.push(o);
      else acc.push({ group: o.group, items: [o] });
      return acc;
    }, []),
  );
  const domId = (id: string) => `${uid}-${id}`;

  async function show() {
    if (disabled) return;
    open = true;
    query = '';
    active = (selected && !selected.disabled ? selected : options.find((o) => !o.disabled))?.id ?? null;
    await tick();
    if (searchable) input?.focus();
  }
  function close() {
    open = false;
    button?.focus();
  }
  function choose(o: Option) {
    if (o.disabled) return;
    onchange(o.id);
    close();
  }
  function move(to: number) {
    if (enabled.length) active = enabled[(to + enabled.length) % enabled.length].id;
  }
  function onkeydown(e: KeyboardEvent) {
    const i = enabled.findIndex((o) => o.id === active);
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); void show(); }
      return;
    }
    if (e.key === 'ArrowDown') move(i + 1);
    else if (e.key === 'ArrowUp') move(i < 0 ? 0 : i - 1);
    else if (e.key === 'Home') move(0);
    else if (e.key === 'End') move(enabled.length - 1);
    else if (e.key === 'Enter') { const o = enabled[i]; if (o) choose(o); }
    else if (e.key === 'Escape') close();
    else return;
    e.preventDefault();
  }
  function outside(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
  // Typing can filter the active option out of the list; keep it on a visible one.
  $effect(() => {
    if (open && !enabled.some((o) => o.id === active)) active = enabled[0]?.id ?? null;
  });
</script>

<svelte:window onmousedown={outside} />

<div class="picker" bind:this={root} {onkeydown} role="presentation">
  <button
    type="button" role="combobox" class="picker-button" bind:this={button}
    aria-haspopup="listbox" aria-expanded={open} aria-labelledby={labelledby}
    aria-controls={open ? `${uid}-list` : undefined}
    aria-activedescendant={open && !searchable && active ? domId(active) : undefined}
    {disabled} onclick={() => (open ? close() : show())}
  >
    {#if selected}
      {#if marker}{@render marker(selected)}{/if}
      <span class="picker-label">{selected.label}</span>
      {#if selected.aside}<span class="picker-aside">{selected.aside}</span>{/if}
    {:else}
      <span class="picker-placeholder">{placeholder}</span>
    {/if}
  </button>
  {#if open}
    <div class="picker-pop">
      {#if searchable}
        <input
          type="search" class="picker-search" bind:this={input} bind:value={query}
          aria-labelledby={labelledby} aria-controls="{uid}-list"
          aria-activedescendant={active ? domId(active) : undefined}
        />
      {/if}
      <div role="listbox" id="{uid}-list" aria-labelledby={labelledby}>
        {#each sections as s}
          {#if s.group !== undefined}
            <div role="group" aria-label={s.group}>
              <div class="picker-group" aria-hidden="true">{s.group}</div>
              {@render rows(s.items)}
            </div>
          {:else}
            {@render rows(s.items)}
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

{#snippet rows(items: Option[])}
  {#each items as o (o.id)}
    <!-- Keyboard goes through the combobox; the option itself is a pointer target. -->
    <div
      role="option" id={domId(o.id)} class="picker-option" class:active={o.id === active}
      aria-selected={o.id === value} aria-disabled={o.disabled || undefined}
      tabindex="-1" onclick={() => choose(o)} onkeydown={() => {}}
    >
      {#if marker}{@render marker(o)}{/if}
      <span class="picker-label">{o.label}</span>
      {#if o.aside}<span class="picker-aside">{o.aside}</span>{/if}
    </div>
  {/each}
{/snippet}

<style>
  .picker { position: relative; }
  .picker-button, .picker-option { display: flex; align-items: center; gap: 8px; }
  .picker-button {
    width: 100%; height: 37px; padding: 0 13px; text-align: left;
    border: 1px solid var(--line-strong); border-radius: 9px; background: var(--surface);
    font: inherit; font-size: 13.5px; color: var(--ink);
  }
  .picker-placeholder { color: var(--ink-faint); }
  .picker-aside { margin-left: auto; color: var(--ink-soft); font-size: 12px; }
  .picker-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .picker-pop {
    position: absolute; z-index: 10; left: 0; right: 0; margin-top: 4px; max-height: 320px; overflow-y: auto;
    border: 1px solid var(--line-strong); border-radius: 9px; background: var(--surface); box-shadow: var(--shadow);
  }
  .picker-search { box-sizing: border-box; width: 100%; padding: 8px 13px; border: 0; border-bottom: 1px solid var(--line); background: var(--surface); font: inherit; color: var(--ink); }
  .picker-group { padding: 6px 13px 2px; font-size: 11.5px; color: var(--ink-soft); }
  .picker-option { padding: 7px 13px; cursor: pointer; font-size: 13.5px; }
  .picker-option.active { background: var(--surface-2); }
  .picker-option[aria-disabled='true'] { color: var(--ink-faint); cursor: default; }
</style>
