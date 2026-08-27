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
  import { onMount, untrack } from 'svelte';
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

  // Ruling M holds: this card is not keyed and does not refetch on a state
  // change — `state` changes twice per question and a remount would snap shut
  // every folder the person opened, in the card whose whole purpose is browsing
  // folder neighbours.
  //
  // 🔴 Owner review on PR #24, P3, is the thing that ruling never provided. A
  // mount-time snapshot goes stale: §7.3 keeps the window alive across a hide,
  // so one launcher outlives many index changes and keeps listing rows that are
  // no longer there. The trigger below is the window regaining focus, and it is
  // chosen rather than invented — `Launcher.svelte:65` already treats window
  // BLUR as "the person has left" and hides the launcher on it, so focus is the
  // same signal in reverse: the launcher is in front of the person again, which
  // is the moment its listing is about to be read. It costs nothing while the
  // window is hidden and it is not tied to the answer state, so the toggles,
  // which are component state, are untouched by it.
  //
  // ⚠️ Unverified on the real window: nobody has run this application yet, so
  // that the webview receives a DOM focus event when Tauri shows the launcher
  // is an inference from the blur handler that is already relied on, not a
  // measurement.
  //
  // `loading` is a plain `let`, not `$state`: nothing renders from it. It stops
  // two listings being on the wire at once, where the loser lands last and puts
  // an older index on screen than the one the card already had.
  let loading = false;
  function load() {
    if (loading) return;
    loading = true;
    listTree()
      .then((l) => { listing = l; failed = false; })
      .catch((e) => {
        // Non-fatal, but never silent: an unreadable listing must say so
        // rather than look like an empty index (Ruling N).
        console.error('list_tree failed', e);
        // 🔴 What disappears: only a card with NOTHING to show says so. A
        // refresh that fails must not take a listing that works off the screen
        // and leave the person with a message, on an event they did not cause.
        if (listing === null) failed = true;
      })
      .finally(() => { loading = false; });
  }

  onMount(load);

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

  // 🔴 Owner review on PR #24, P4. The invariant: when the source card shows a
  // passage, this card shows which row it came from. Two states broke it, and
  // `openByDefault` above cannot reach either — it is consulted only where
  // `toggled` has no entry, and only by the Files tab.
  //
  // `toggled` winning over `openByDefault` is Ruling M and it stands: folders
  // must not snap shut on every question. But "this citation is now selected"
  // is a DIFFERENT event from "an answer arrived" — it is the person's own
  // click on a citation, and its entire purpose is to be shown where the
  // passage came from. So a NEW selection clears the hand-toggle on the folders
  // along its own path, and on no others: a folder opened or shut somewhere
  // else is none of this selection's business, and clearing those would be
  // Ruling M's defect with an extra step.
  //
  // Deliberately NOT undone: a person who shuts the folder of the passage
  // already on screen keeps it shut. They acted on a row they could see, and no
  // new event has happened since.
  //
  // The second half is the Recents tab, where marking a row is not enough
  // because the selected document may have no row there at all (recents is a
  // short list of what was indexed last). When the tab on screen cannot show
  // the selection and the other one can, the card shows the one that can. A tab
  // the person picks while the selection stands is left alone — this runs on a
  // CHANGE of selection only.
  //
  // The stamp is what makes "a change" precise: it holds the selected document
  // and the folders on its way, so it also fires when the listing arrives after
  // the selection did. Everything it writes is read inside `untrack`, so this
  // effect depends on the selection and the listing, never on what it sets.
  let lastSelection = '';
  $effect(() => {
    const id = selectedId;
    const l = listing;
    const onTheWay = openByDefault;
    const stamp = JSON.stringify([id, [...onTheWay].sort()]);
    if (stamp === lastSelection) return;
    lastSelection = stamp;
    if (id === null || l === null) return;

    untrack(() => {
      // Only the folders explicitly SHUT by hand on this selection's path, and
      // they are set open rather than forgotten. Both halves are load-bearing:
      //
      // - a folder the person OPENED by hand is not touched at all. Deleting
      //   its entry was measured and it takes Ruling M's defect back by the
      //   long way round: with the entry gone the folder is open only while
      //   the selection is, so it snapped shut the moment the next answer was
      //   a refusal — a folder the person opened, closing itself on an event
      //   they did not cause.
      // - the shut one is set to `true`, not deleted, for the same reason in
      //   reverse: `true` is what "this folder is open" is written as, and it
      //   survives the selection going away the way a hand-open does.
      //
      // A folder with no entry keeps having none: it is open because the
      // selection is on its path, and it shuts again when that stops being
      // true. That is the default, and it is not a person's decision to keep.
      const next = { ...toggled };
      let changed = false;
      for (const k of onTheWay) {
        if (next[k] === false) { next[k] = true; changed = true; }
      }
      if (changed) toggled = next;

      const inRecents = l.recents.some((r) => r.documentId === id);
      const inFiles = l.roots.some((r) => r.files.some((f) => f.documentId === id));
      if (tab === 'recents' && !inRecents && inFiles) tab = 'files';
    });
  });
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

<svelte:window onfocus={load} />

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
               action of any kind. A row, rendered as one.
               P4: and it carries the mark, which it never did — selecting a
               citation while this tab was showing left no current row
               anywhere, so the source card was reading out a passage the tree
               could not place. -->
          <span data-testid={`tree-recent-${recent.documentId}`}
            aria-current={recent.documentId === selectedId ? 'true' : undefined}
            >{recent.relativePath}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
