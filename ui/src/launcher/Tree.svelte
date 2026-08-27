<script module lang="ts">
  import type { TreeFile } from '../lib/ipc';

  export type TreeNode =
    | { kind: 'folder'; name: string; path: string; children: TreeNode[] }
    | { kind: 'file'; name: string; path: string; documentId: string };

  // Decision 6: the folder tree is a split of `relativePath` on `/`, and
  // nothing else — the backend hands over flat paths per root. Pure and
  // exported so the depth cases (no slash at all, two levels deep, both mixed
  // in one root) can be pinned without a render; a render can only show one
  // depth at a time, and only through the collapse rules.
  export function buildFolderTree(files: TreeFile[]): TreeNode[] {
    const top: TreeNode[] = [];
    const folders = new Map<string, Extract<TreeNode, { kind: 'folder' }>>();

    for (const file of files) {
      const parts = file.relativePath.split('/');
      const name = parts.pop()!; // split never returns an empty array
      let siblings = top;
      let path = '';
      for (const part of parts) {
        path = path ? `${path}/${part}` : part;
        let folder = folders.get(path);
        if (!folder) {
          folder = { kind: 'folder', name: part, path, children: [] };
          folders.set(path, folder);
          siblings.push(folder);
        }
        siblings = folder.children;
      }
      siblings.push({ kind: 'file', name, path: file.relativePath, documentId: file.documentId });
    }
    return top;
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { listTree } from '../lib/ipc';
  import type { AskCitation, Hit, TreeListing } from '../lib/ipc';

  // Ruling O: Task 8b holds the selection as exactly this union, so pinning
  // the narrower type here would force a signature change one task later.
  // Both members carry `documentId`, which is all this card reads.
  let { selected }: { selected: AskCitation | Hit | null } = $props();

  let listing = $state<TreeListing | null>(null);
  let failed = $state(false);
  let tab = $state<'files' | 'recents'>('files');
  // Folders the person opened or shut by hand, keyed per root: two roots may
  // hold the same folder path and must not share one open flag.
  let toggled = $state<Record<string, boolean>>({});

  // Ruling M: on mount, and only on mount. Task 8b deliberately does not key
  // this card — `state` changes twice per question, and a remount would
  // refetch and snap shut every folder the person opened, in the card whose
  // whole purpose is browsing folder neighbours.
  onMount(() => {
    listTree()
      .then((l) => (listing = l))
      .catch((e) => {
        // Non-fatal, but never silent: an unreadable listing must say so
        // rather than look like an empty index (Ruling N).
        console.error('list_tree failed', e);
        failed = true;
      });
  });

  const filesLabel = $derived.by(() => { void $locale; return t('tree_tab_files'); });
  const recentsLabel = $derived.by(() => { void $locale; return t('tree_tab_recents'); });
  const emptyLabel = $derived.by(() => { void $locale; return t('tree_empty'); });
  const failedLabel = $derived.by(() => { void $locale; return t('tree_failed'); });

  // Ruling P: selection is by documentId, never by relativePath — two roots
  // can hold the same relative path under different documents.
  const selectedId = $derived(selected?.documentId ?? null);

  const roots = $derived(
    (listing?.roots ?? []).map((root) => ({ root, nodes: buildFolderTree(root.files) })),
  );
  const isEmpty = $derived(
    listing !== null && listing.roots.length === 0 && listing.recents.length === 0,
  );

  // NUL as the separator because no path component can contain one, so no two
  // distinct (root, folder) pairs can collide on one key. It must be written as
  // the escape `\0`: a raw NUL byte here makes the whole file binary — grep and
  // rg go silent on it and `git diff` prints "Binary files differ", hiding the
  // component from every review.
  function key(rootId: number, path: string) {
    return `${rootId}\0${path}`;
  }

  // Folders start expanded along the path to the selected file and collapsed
  // everywhere else. Derived from `selected`, so a new citation opens its
  // folder without a refetch; `toggled` still wins, so an opened folder the
  // person is reading does not shut itself on the next answer.
  const openByDefault = $derived.by(() => {
    const open = new Set<string>();
    if (selectedId === null || listing === null) return open;
    for (const root of listing.roots) {
      for (const file of root.files) {
        if (file.documentId !== selectedId) continue;
        const parts = file.relativePath.split('/');
        parts.pop();
        let path = '';
        for (const part of parts) {
          path = path ? `${path}/${part}` : part;
          open.add(key(root.rootId, path));
        }
      }
    }
    return open;
  });

  function isOpen(rootId: number, path: string) {
    const k = key(rootId, path);
    return k in toggled ? toggled[k] : openByDefault.has(k);
  }

  function toggle(rootId: number, path: string) {
    const k = key(rootId, path);
    toggled = { ...toggled, [k]: !isOpen(rootId, path) };
  }
</script>

{#snippet branch(nodes: TreeNode[], rootId: number)}
  <ul>
    {#each nodes as node (node.path)}
      <li>
        {#if node.kind === 'folder'}
          <button
            type="button"
            data-testid={`tree-folder-${node.path}`}
            aria-expanded={isOpen(rootId, node.path)}
            onclick={() => toggle(rootId, node.path)}>{node.name}</button>
          {#if isOpen(rootId, node.path)}
            {@render branch(node.children, rootId)}
          {/if}
        {:else}
          <!-- 🔴 Owner review on PR #24, P5. This row was a `<button>` with no
               `onclick` and a `role="treeitem"` with no enclosing tree, group
               or keyboard model: click, Enter and Space all did nothing, and a
               keyboard reached it only to find that out. It is a row, so it is
               rendered as one.

               The other option — wire the promised action — is not reachable
               from this card: opening a document needs a command that takes a
               `documentId` and the bridge has none (`lib/ipc.ts:81-107`;
               `source_around` needs a chunk, which only a citation carries).
               A row that says "activate me" and cannot is the finding itself.

               `aria-current` stays: it is a global attribute, valid on any
               element, and it is what says which row the source card is
               showing. `aria-selected` goes with the role — it is defined only
               inside a listbox/grid/tree, and on a bare row it states a
               membership that no longer exists. -->
          <span
            data-testid={`tree-file-${node.documentId}`}
            aria-current={node.documentId === selectedId ? 'true' : undefined}
            >{node.name}</span>
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<div class="tree" data-testid="tree-body">
  <div class="tabs">
    <button
      type="button"
      data-testid="tree-tab-files"
      aria-pressed={tab === 'files'}
      onclick={() => (tab = 'files')}>{filesLabel}</button>
    <button
      type="button"
      data-testid="tree-tab-recents"
      aria-pressed={tab === 'recents'}
      onclick={() => (tab = 'recents')}>{recentsLabel}</button>
  </div>

  {#if failed}
    <p data-testid="tree-failed">{failedLabel}</p>
  {:else if isEmpty}
    <p data-testid="tree-empty">{emptyLabel}</p>
  {:else if tab === 'files'}
    {#each roots as { root, nodes } (root.rootId)}
      <section data-testid={`tree-root-${root.rootId}`}>
        <h4>{root.name}</h4>
        {@render branch(nodes, root.rootId)}
      </section>
    {/each}
  {:else}
    <ul>
      {#each listing?.recents ?? [] as recent (recent.documentId)}
        <li>
          <!-- P5, the same finding one tab over: a focusable button with no
               action of any kind. A row, rendered as one. -->
          <span data-testid={`tree-recent-${recent.documentId}`}
            >{recent.relativePath}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
