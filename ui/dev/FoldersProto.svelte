<script lang="ts">
  // Prototype only (owner, 2026-11-09): the Folders section as one tree panel,
  // with the add/remove/mask controls as an icon bar at the panel's foot.
  // Local state, no IPC — the real surface stays `src/settings/Folders.svelte`.
  // `ignored` — a
  // `.gitignore` in the folder names it; `failed` — the last scan could not read
  // the file, with the reason. Formats no extractor reads and built-in junk
  // files and folders (node_modules, .DS_Store) are not listed at all.
  type Node = {
    name: string; children: Node[] | null; // null = a file
    ignored?: boolean; failed?: string;
  };
  type Root = { id: number; path: string; docs: number; children: Node[] };
  type Flags = Omit<Node, 'name' | 'children'>;

  const d = (name: string, children: Node[], f: Flags = {}): Node => ({ name, children, ...f });
  const f = (name: string, fl: Flags = {}): Node => ({ name, children: null, ...fl });
  let nextId = 3;
  let roots: Root[] = $state([
    { id: 1, path: '/home/u/docs', docs: 8, children: [
      d('archive', [d('2019', [f('report.pdf')]), d('2020', [f('summary.docx', { failed: 'Could not extract text: the file is damaged' }), f('summary.docx.tmp')])]),
      d('drafts', [f('chapter-1.md'), f('chapter-2.md')]),
      d('notes', [d('meetings', [f('2026-10-01.md')]), f('todo.txt'), f('debug.log')]),
      d('out', [f('export.pdf'), f('export.html')], { ignored: true }),
      f('.gitignore'),
      f('book.epub'),
      f('contract.bak', { ignored: true }),
      f('contract.pdf'),
    ] },
    { id: 2, path: '/home/u/papers', docs: 2, children: [
      d('arxiv', [f('2401.01234.pdf')]), d('scans', [f('vol1.pdf', { failed: 'The PDF is password-protected' }), f('vol2.pdf')]),
    ] },
  ]);
  let open = $state(new Set<string>(['1:']));
  let excluded = $state(new Set<string>(['1:drafts']));
  // Ticked although `.gitignore` names it: the override layer the walk lacks today.
  let included = $state(new Set<string>());

  // Mask = a glob on the file's own name, caseless (`rules.rs` `MaskLayer::matches`).
  const maskOf = (name: string) =>
    masks.find((m) => new RegExp(`^${m.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.')}$`, 'i').test(name));

  type State = { box: 'on' | 'off' | 'dim' | 'none'; tag?: string };
  function stateOf(k: string, node: Node, parentOff: boolean): State {
    const mask = node.children === null ? maskOf(node.name) : undefined;
    if (mask) return { box: 'none', tag: `mask ${mask}` };
    if (parentOff) return { box: 'dim' };
    if (node.ignored) return included.has(k) ? { box: 'on', tag: 'gitignore · included' } : { box: 'off', tag: 'gitignore' };
    return excluded.has(k) ? { box: 'off' } : { box: 'on' };
  }
  // Mixed: the folder is on, but something below that could be ticked is not.
  // Masked rows cannot be ticked, so they never make it mixed.
  function anyOff(id: number, parent: string, nodes: Node[]): boolean {
    return nodes.some((c) => {
      const p = join(parent, c.name);
      const st = stateOf(key(id, p), c, false);
      return st.box === 'off' || (st.box === 'on' && !!c.children && anyOff(id, p, c.children));
    });
  }
  // A click on a mixed box ticks everything below it, the launcher tree's rule.
  function tickAll(id: number, parent: string, nodes: Node[]) {
    const ex = new Set(excluded), inc = new Set(included);
    const walk = (p: string, ns: Node[]) => ns.forEach((c) => {
      const k = key(id, join(p, c.name));
      ex.delete(k);
      if (c.ignored) inc.add(k);
      if (c.children) walk(join(p, c.name), c.children);
    });
    walk(parent, nodes);
    excluded = ex; included = inc;
  }
  // Ticking something under an unticked folder: every unticked folder on the way
  // turns on, and the exclusion moves down onto the siblings left behind. The
  // folder then reads as mixed.
  function enableUnder(id: number, path: string) {
    const ex = new Set(excluded), inc = new Set(included);
    let nodes = roots.find((r) => r.id === id)!.children;
    let p = '';
    let pushing = false;
    for (const part of path.split('/')) {
      const node = nodes.find((n) => n.name === part)!;
      if (pushing) for (const s of nodes) if (s !== node && !s.ignored) ex.add(key(id, join(p, s.name)));
      const k = key(id, join(p, part));
      if (node.ignored ? !inc.has(k) : ex.has(k)) { pushing = true; node.ignored ? inc.add(k) : ex.delete(k); }
      p = join(p, part);
      nodes = node.children ?? [];
    }
    excluded = ex; included = inc;
  }
  const failedIn = (nodes: Node[]): number =>
    nodes.reduce((n, c) => n + (c.failed ? 1 : 0) + (c.children ? failedIn(c.children) : 0), 0);
  function flip(id: number, path: string, node: Node) {
    const k = key(id, path);
    const which = node.ignored ? included : excluded;
    const s = new Set(which); s.has(k) ? s.delete(k) : s.add(k);
    node.ignored ? (included = s) : (excluded = s);
    collapseUp(id, path);
  }
  // The last ticked child unticked: the folder itself goes off, and the
  // per-child exclusions fold back into one rule on the folder. Walks upward,
  // so a folder emptied this way can empty its own parent too.
  function collapseUp(id: number, path: string) {
    const parts = path.split('/');
    for (let i = parts.length - 1; i >= 1; i--) {
      const dir = parts.slice(0, i).join('/');
      let nodes = roots.find((r) => r.id === id)!.children;
      let node: Node | undefined;
      for (const part of dir.split('/')) { node = nodes.find((n) => n.name === part); nodes = node?.children ?? []; }
      if (!node || node.ignored) return;
      const tickable = nodes.filter((c) => stateOf(key(id, join(dir, c.name)), c, false).box !== 'none');
      if (!tickable.length || tickable.some((c) => stateOf(key(id, join(dir, c.name)), c, false).box === 'on')) return;
      const ex = new Set(excluded);
      ex.add(key(id, dir));
      for (const c of tickable) ex.delete(key(id, join(dir, c.name)));
      excluded = ex;
    }
  }
  let selected: number | null = $state(null);
  let masks = $state(['*.tmp']);
  let maskSelected: string | null = $state(null);
  let maskOpen = $state(false);
  let maskDraft = $state('');
  let maskInput: HTMLInputElement | undefined = $state();

  const key = (id: number, path: string) => `${id}:${path}`;
  const join = (p: string, name: string) => (p ? `${p}/${name}` : name);
  function toggle(k: string) { const s = new Set(open); s.has(k) ? s.delete(k) : s.add(k); open = s; }
  function addFolder() {
    const id = nextId++;
    roots = [...roots, { id, path: `/home/u/new-${id}`, docs: 0, children: [d('sub', [f('a.pdf')])] }];
    selected = id;
  }
  function removeSelected() {
    if (selected === null) return;
    roots = roots.filter((r) => r.id !== selected);
    selected = null;
  }
  async function toggleMask() {
    maskOpen = !maskOpen;
    if (maskOpen) { await Promise.resolve(); maskInput?.focus(); }
  }
  function addMask(e: SubmitEvent) {
    e.preventDefault();
    const p = maskDraft.trim();
    if (p && !masks.includes(p)) masks = [...masks, p];
    maskSelected = p || null;
    maskDraft = '';
    maskOpen = false;
  }
  function removeMask() {
    masks = masks.filter((m) => m !== maskSelected);
    maskSelected = null;
  }
</script>

{#snippet chevron()}
  <svg class="chev" viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4l4 4-4 4"/></svg>
{/snippet}
{#snippet folderIcon(isOpen: boolean)}
  <svg class="ico" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true">
    {#if isOpen}<path d="M2 12.5V4.5a1 1 0 0 1 1-1h3l1.5 1.5H12a1 1 0 0 1 1 1V7"/><path d="M2 12.5l1.8-5h10.7l-1.8 5z"/>
    {:else}<path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>{/if}
  </svg>
{/snippet}

{#snippet fileIcon()}
  <svg class="ico" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true"><path d="M4 2h5.5L12.5 5v8.5a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5v-11A.5.5 0 0 1 4 2z"/><path d="M9.5 2v3h3M5.8 8h4.4M5.8 10.5h4.4"/></svg>
{/snippet}

{#snippet branch(id: number, parent: string, nodes: Node[], parentOff: boolean)}
  <ul>
    {#each nodes as node (node.name)}
      {@const path = join(parent, node.name)}
      {@const k = key(id, path)}
      {@const st = stateOf(k, node, parentOff)}
      {@const isDir = node.children !== null}
      {@const expandable = isDir && node.children!.length > 0}
      {@const mixed = st.box === 'on' && isDir && anyOff(id, path, node.children!)}
      <li>
        <div class="folder-row" class:off={st.box === 'off' || st.box === 'dim'} class:never={st.box === 'none'} class:file={!isDir}>
          <button type="button" aria-expanded={expandable ? open.has(k) : undefined}
            class:leaf={!expandable}
            onclick={() => expandable && toggle(k)}
          >{@render chevron()}<span class="check-slot"></span>{#if isDir}{@render folderIcon(open.has(k))}{:else}{@render fileIcon()}{/if}<span class="label">{node.name}</span><span class="tags">{#if st.tag}<span class="tag" class:git={node.ignored}>{st.tag}</span>{/if}{#if node.failed}<span class="tag err" title={node.failed}>scan failed</span>{:else if isDir && !open.has(k) && failedIn(node.children!)}<span class="tag err" title="Files inside could not be read">{failedIn(node.children!)} failed</span>{/if}</span></button>
          {#if st.box !== 'none'}
            <input type="checkbox" class="search-check" aria-label={`Index ${node.name}`}
              checked={st.box === 'on' && !mixed} indeterminate={mixed}
              onchange={() => (st.box === 'dim' ? enableUnder(id, path) : mixed ? tickAll(id, path, node.children!) : flip(id, path, node))} />
          {/if}
        </div>
        {#if expandable && open.has(k)}{@render branch(id, path, node.children!, st.box !== 'on')}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<section class="proto">
  <h2>Folders</h2>

  <div class="box">
    <div class="tree">
      {#each roots as root (root.id)}
        {@const k = key(root.id, '')}
        <section>
        <ul>
          <li>
            <div class="folder-row root" class:selected={selected === root.id}>
              <button type="button" aria-expanded={open.has(k)}
                onclick={() => { selected = root.id; toggle(k); }}
              >{@render chevron()}{@render folderIcon(open.has(k))}<span class="label mono">{root.path}</span><span class="tags">{#if !open.has(k) && failedIn(root.children)}<span class="tag err">{failedIn(root.children)} failed</span>{/if}<span class="count">{root.docs} docs</span></span></button>
            </div>
          </li>
        </ul>
        {#if open.has(k)}{@render branch(root.id, '', root.children, false)}{/if}
        </section>
      {/each}
    </div>

    <div class="bar">
      <button type="button" title="Add a folder" aria-label="Add a folder" onclick={addFolder}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M8 3v10M3 8h10"/></svg>
      </button>
      <span class="sep"></span>
      <button type="button" title="Remove the selected folder" aria-label="Remove the selected folder"
        disabled={selected === null} onclick={removeSelected}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M3 8h10"/></svg>
      </button>
    </div>
  </div>

  <p class="hint">Untick a folder or file to keep it out of the index. What <span class="mono">.gitignore</span> names starts unticked; tick it to index it anyway.</p>

  <h3>File masks</h3>
  <div class="box">
    <ul class="masks">
      {#each masks as m (m)}
        <li><button type="button" class="mask" class:selected={maskSelected === m}
          onclick={() => (maskSelected = m)}><span class="mono">{m}</span></button></li>
      {:else}
        <li class="empty">No masks.</li>
      {/each}
    </ul>

    {#if maskOpen}
      <form class="mask-row" onsubmit={addMask}>
        <input bind:this={maskInput} bind:value={maskDraft} placeholder="e.g. *.log"
          onkeydown={(e) => e.key === 'Escape' && (maskOpen = false)} />
        <button type="submit" disabled={!maskDraft.trim()}>Add</button>
      </form>
    {/if}

    <div class="bar">
      <button type="button" title="Add a file mask" aria-label="Add a file mask" aria-pressed={maskOpen} onclick={toggleMask}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M8 3v10M3 8h10"/></svg>
      </button>
      <span class="sep"></span>
      <button type="button" title="Remove the selected mask" aria-label="Remove the selected mask"
        disabled={maskSelected === null} onclick={removeMask}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M3 8h10"/></svg>
      </button>
    </div>
  </div>
  <p class="hint">Masks apply in every folder and match a file's name, e.g. <span class="mono">*.log</span>.</p>
</section>

<style>
  .proto { max-width: 560px; margin: 28px 32px; color: var(--ink); font-family: var(--sans); }
  h2 { font-family: var(--serif); font-size: 20px; font-weight: 600; margin: 0 0 14px; }
  h3 { font: 500 11px/1 var(--mono); letter-spacing: 0.12em; text-transform: uppercase; color: var(--ink-soft); margin: 22px 0 10px; }
  .mono { font-family: var(--mono); }
  .hint { color: var(--ink-soft); font-size: 12.5px; margin: 8px 2px 0; }

  .box { border: 1px solid var(--line); border-radius: 12px; background: var(--surface-2); overflow: hidden; }
  .tree { font-size: 13px; padding: 6px; height: 340px; overflow: auto; }
  .tree ul { list-style: none; margin: 0; padding: 0; }
  .tree ul ul, .tree section > ul + ul { padding-left: 18px; }
  .folder-row { position: relative; }
  .folder-row > button {
    display: flex; align-items: center; gap: 4px; width: 100%; padding: 4px 7px;
    border: 1px solid transparent; border-radius: 6px; background: transparent;
    color: var(--ink); font: 500 13px var(--sans); text-align: left; cursor: default;
  }
  .folder-row > button:hover { background: var(--surface-3); }
  .folder-row.selected > button { background: var(--select); color: var(--on-select); }
  .folder-row.selected .chev, .folder-row.selected .ico, .folder-row.selected .count { color: var(--on-select); }
  .label { white-space: nowrap; }
  .tags { margin-left: auto; padding-left: 16px; display: flex; align-items: center; gap: 8px; }
  .count { font-size: 12px; font-weight: 400; color: var(--ink-soft); }
  .tag { font-size: 11.5px; font-weight: 400; font-style: normal; font-family: var(--sans); color: var(--ink-soft); }
  .tag.err { color: var(--err); }
  .tag.err::before { content: '⚠ '; }
  .tag.git { padding: 0 5px; border: 1px solid var(--line-strong); border-radius: 4px; font-family: var(--mono); font-size: 10.5px; }
  .file > button { font-weight: 400; }
  .chev { flex: none; color: var(--ink-soft); transition: transform 0.12s; }
  [aria-expanded="true"] > .chev { transform: rotate(90deg); }
  button.leaf > .chev { visibility: hidden; }
  .check-slot { flex: none; width: 16px; }
  .ico { flex: none; color: var(--ink-soft); margin-right: 2px; }
  .off > button .label { font-family: var(--serif); font-style: italic; font-weight: 400; color: var(--ink-soft); }
  .never > button .label { color: var(--ink-soft); font-weight: 400; }
  .never .check-slot { visibility: hidden; }
  .search-check {
    position: absolute; left: 23px; top: 50%; transform: translateY(-50%);
    appearance: none; width: 16px; height: 16px; margin: 0; border-radius: 4px;
    display: grid; place-items: center; cursor: pointer; color: var(--ink-soft);
    border: 1px solid var(--ink-soft); background: transparent;
  }
  .search-check:checked, .search-check:indeterminate { border-color: transparent; }
  .search-check:checked::after { content: '✓'; font-size: 13px; line-height: 1; }
  .search-check:indeterminate::after { content: '–'; font-size: 13px; line-height: 1; }
  .search-check:disabled { opacity: 0.4; cursor: default; }
  .search-check:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }

  .mask-row { display: flex; gap: 6px; padding: 8px; border-top: 1px solid var(--line); }
  .mask-row input {
    flex: 1; min-width: 0; padding: 5px 8px; border: 1px solid var(--line-strong); border-radius: 6px;
    background: var(--surface); color: var(--ink); font: 13px var(--mono);
  }
  .mask-row button { padding: 4px 12px; border: 1px solid var(--line-strong); border-radius: 6px; background: transparent; color: var(--ink); font: 500 13px var(--sans); }
  .mask-row button:disabled { color: var(--ink-faint); }

  .bar { display: flex; align-items: center; height: 30px; border-top: 1px solid var(--line); padding: 0 4px; }
  .bar button {
    display: grid; place-items: center; width: 28px; height: 22px; border: 0; border-radius: 5px;
    background: transparent; color: var(--ink); cursor: default;
  }
  .bar button:hover:not(:disabled), .bar button[aria-pressed="true"] { background: var(--surface-3); }
  .bar button:disabled { color: var(--ink-faint); }
  .bar .sep { width: 1px; height: 14px; background: var(--line-strong); margin: 0 3px; }

  .masks { list-style: none; margin: 0; padding: 6px; min-height: 64px; max-height: 180px; overflow: auto; }
  .mask {
    display: block; width: 100%; padding: 4px 9px; border: 0; border-radius: 6px;
    background: transparent; color: var(--ink); font-size: 13px; text-align: left; cursor: default;
  }
  .mask:hover { background: var(--surface-3); }
  .mask.selected { background: var(--select); color: var(--on-select); }
  .masks .empty { padding: 4px 9px; color: var(--ink-soft); font-size: 13px; }
</style>
