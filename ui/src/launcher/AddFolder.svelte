<script lang="ts">
  import { locale, t } from '../i18n';
  import { phaseLabel } from '../settings/jobs';
  import type { AddFolderController } from './addFolder';
  import { ringFraction } from './scanRing';

  // A view of the launcher's one add-folder controller (`addFolder.ts`): the
  // Documents pane and the header each draw one, never both at once.
  let { folders }: { folders: AddFolderController } = $props();

  const fraction = $derived($folders.scan === null ? null : ringFraction($folders.scan.snapshot));
  const label = $derived.by(() => { void $locale; return t('tree_add_folder'); });
  const title = $derived.by(() => {
    void $locale;
    const s = $folders.scan?.snapshot;
    if (fraction === null || s?.kind !== 'running') return label;
    const p = s.phase;
    if (p.kind !== 'reading' && p.kind !== 'embedding') return label;
    const l = phaseLabel(p);
    // A phase whose total is still unknown claims no count.
    const counts = p.counts.total === 0 ? '' : ` ${p.counts.done}/${p.counts.total}`;
    return `${label} — ${t(l.key, l.params)}${counts}`;
  });
</script>

<button type="button" class="tb add-folder" data-testid="tree-add-folder"
  aria-label={title} title={title} onclick={() => folders.press()}>
  {#if fraction !== null}
    <svg class="scan-ring" class:spin={fraction === 'spin'} data-testid="tree-add-folder-ring"
      data-fraction={String(fraction)} viewBox="0 0 26 26" aria-hidden="true">
      <circle cx="13" cy="13" r="11.5" pathLength="1"
        stroke-dasharray={fraction === 'spin' ? '0.25 1' : `${fraction} 1`} />
    </svg>
  {/if}
  <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor"
    stroke-width="1.4" stroke-linecap="round" aria-hidden="true"><path d="M8 3.5v9M3.5 8h9"/></svg>
</button>
{#if $folders.error !== null}
  <p class="tree-note" role="alert" data-testid="tree-add-folder-error">{$folders.error}</p>
{/if}
