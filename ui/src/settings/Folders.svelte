<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import {
    listTree, addWatchedFolder, removeWatchedFolder, confirmWatchedFolder,
    listSubfolders, listExclusions, rootMark, setTick, previewTick,
    type StoredExclusion, type Subfolder, type SubfolderListing, type SubfolderState,
    type ScanSnapshot, type TreeListing, type TreeRoot,
  } from '../lib/ipc';
  import type { JobController } from './jobs';
  import { masksVersion } from './masksVersion';
  import { dialogFocus, focusCancel } from './dialog';
  import { unapplied, foldersWaiting, applyAction } from './unapplied';

  // §9.2, Tasks 7 and 8 — the folder surface: add, list, remove. Scanning is
  // not this component's any more (Task 8): the single Scan control
  // lives in the Scanning section (`Scanning.svelte`), and adding a folder here
  // starts nothing at all — the note below the list says so and names the next
  // step, because excluding subfolders (PR 8) is a configuration move a person
  // may still want to make before the first scan reads this folder.
  //
  // Fixture question: `TreeRoot` (`ipc.ts`) carries `files: TreeFile[]` and no
  // flag for whether a walk has ever run. A folder just added and a folder
  // walked to genuine completion with nothing in it are the SAME value on
  // this wire — so this screen never says a folder is "empty"; it states the
  // count `indexed_documents` gives, which is true of both states, and stops
  // there. Telling the two apart belongs to the task that runs the walk.

  // Still required, even with the scan button gone: this component listens for
  // a job's ending through the SAME controller `Settings.svelte` owns, to
  // re-read a panel a scan elsewhere may have moved underneath it — see
  // `reread`/`withdrawQuestions` below. A folder list that quietly dropped that
  // listener when nobody passed one is the shape a stale panel goes unnoticed
  // in.
  let { jobs }: { jobs: JobController } = $props();
  // Read once, on purpose: the controller's identity never changes for the life
  // of the window, so `$jobState` below is ordinary auto-subscription — the
  // shape `Scanning.svelte:40` already uses. It is a SECOND reader of the same
  // store beside the `onMount` subscription further down, and the two answer
  // different questions: that one acts on a change (withdraw, re-read), this
  // one draws the state a run is in right now.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  let roots = $state<TreeRoot[]>([]);
  // Set on a rejected `list_tree`, at mount or on a later re-read. Held apart
  // from `actionError` (Models.svelte's own precedent) because this one means
  // the list on screen cannot be trusted at all, not that one action failed.
  let loadError = $state<string | null>(null);
  // Set on a rejected add/remove. The backend's own sentence, shown verbatim
  // beside no lead-in of this component's own — §10: a rejection arrives as
  // a sentence, never a kind, and no re-read is implied by it.
  let actionError = $state<string | null>(null);
  // 🔴 PR 8a, fix round 5. Set when a press was about a folder the fresh
  // listing no longer puts at this id — see `namesFolder`. Component-level and
  // not on the panel, because the panel is precisely what goes: `refresh` drops
  // it, so a note written into it would be dropped with it and the press would
  // read as a press that did nothing.
  //
  // A boolean and not the sentence: a `t()` frozen here would keep its
  // language through a switch. Cleared at the four
  // functions a press into this component can start — `askExclude`,
  // `askInclude`, `addFolder`, `askRemove` — for the same rule as
  // `actionError`: what the last press led to stands until the next one. Not
  // at `exclude`/`include`, `answer` or `confirmRemove` themselves: those run
  // only once one of the four above has already cleared it for this press.
  let rootChanged = $state(false);

  // 🔴 Task 8. NOT `rootChanged`: that flag answers a different question — "the
  // id/path a press was about has moved underneath it" — and never becomes
  // `true` on an ordinary successful add, so it cannot be reused here without
  // making an unrelated banner appear or disappear by accident. This one
  // answers "the last press was an add that succeeded", for the owner's own
  // ruling: adding a folder starts no scan, and this is the sentence that says
  // what to do instead. Cleared at the two presses that change the list —
  // `addFolder` and `askRemove` — for the same reason `rootChanged` is
  // cleared at ITS four: what the last press led to stands until the next one.
  // 🔴 Task 9: `askRemove` and not the confirmation behind it. The press is
  // what makes this note stale — a person who has pressed "Remove" has moved
  // on from the add whatever they answer next — and a note cleared only on the
  // answer would stand over an open removal question about the folder it names.
  // 🔴 Fix round 1, Minor 2: TWO sites, not `rootChanged`'s four — that flag
  // also clears at `askExclude`/`askInclude`, which do not touch this list at
  // all, so citing "the same place" overstated where this one lives.
  let folderAdded = $state(false);

  // ── Task 9: "Remove" asks before it removes ─────────────────────────────
  //
  // 🔴 The question lives HERE and not in `Pending`, and the reason is where the
  // press is: `Pending` is a PANEL's question and exists only while a row is
  // expanded, while this press is on the row itself and is available whether the
  // row is open or shut. A question stored on the panel would be dropped by the
  // collapse that a person expanding another folder makes, and dropped in
  // silence.
  //
  // At most one on screen — a second "Remove" replaces the first, for
  // `Pending`'s own reason: two questions about two folders, each with a
  // Confirm, is a screen on which the wrong one can be answered.
  //
  // `files` is `root.files.length` read ONCE, at the click, from the row the
  // person saw, and `path` likewise — the same freezing `Pending` does with its
  // two numbers, for the same reason: a re-read landing underneath must not
  // renumber or rename a sentence somebody is part way through reading. It is
  // also what is SENT: see `confirmRemove`.
  let removeQuestion = $state<{ rootId: number; path: string; files: number } | null>(null);
  // The root whose removal is in flight, or `null`. Its row says so in place of
  // its buttons and every other row's "Remove" is disabled meanwhile: a
  // second press during the wait sends `remove_watched_folder` twice for one
  // folder, and the second one races the re-read behind the first.
  let removing = $state<number | null>(null);
  // The root the `−` acts on, by id AND path: `watched_root.id` is a rowid
  // alias SQLite hands out again, so an id alone could select another folder.
  let chosen = $state<{ rootId: number; path: string } | null>(null);
  // Whether a root holds any rule of the person's own whose folder is there,
  // or a top-level row `.gitignore` names, as last read for that path. Kept after its panel is shut, so a collapsed
  // root still shows `–` (spec, owner's ruling 5). Only this window writes
  // rules, and it re-reads the panel when it does, so the record is current
  // for as long as nothing else can move it.
  let marks = $state<Record<number, { path: string; some: boolean }>>({});

  // Whether a job holds the slot, read LIVE from the store rather than captured
  // at the press. `remove_watched_folder` is refused while a job is running
  // (`bridge.rs:97-180`), so the press is taken off the screen before it is
  // made — and a run that starts after the question was opened has to reach the
  // Confirm button already drawn, which a value captured at the press could not
  // do.
  const scanRunning = $derived($jobState.scan.snapshot.kind === 'running');

  // ── PR 8a, Task 5: what an expanded folder holds ──────────────────────────
  //
  // One expansion per watched root, present exactly while that row is open —
  // collapsing DELETES the entry rather than hiding it, because a kept listing
  // is a claim about a moment that has passed and the disk can change between
  // two expands.
  //
  // `tree` mirrors the open state instead of being a flat cache keyed by path:
  // a child is fetched only while it is open, so re-reading after an action is
  // one walk of the same shape and a subfolder that stopped existing — or
  // stopped being expandable, because a rule now names it — simply does not
  // come back.
  type SubTree = { listing: SubfolderListing; children: Record<string, SubTree> };
  type Panel = {
    // 🔴 PR 8a, fix round 4, item 1: the absolute path this panel was built
    // for, recorded so that `refresh` can tell "the same root" apart from "the
    // same id". The id alone cannot answer that question — see the prune in
    // `refresh` for why, and for what it costs when it is asked of the id.
    // Written once, at `toggleRoot`, from the very row the person pressed; it
    // is never patched, because a panel whose path has changed is not a panel
    // to correct, it is one to drop.
    rootPath: string;
    tree: SubTree | null;
    rules: StoredExclusion[] | null;
    // A rejected `list_subfolders`/`list_exclusions`. Held apart from the two
    // banners above for the reason those two are held apart from each other:
    // this one means the SUBFOLDERS of one row cannot be trusted, while the
    // list of watched folders is fine and no add or remove has failed.
    loadError: string | null;
    // A rejected exclude/include, shown verbatim (§10).
    actionError: string | null;
    // PR 8a, Task 6: the question now in front of the person for this row, or
    // `null`. At most one per row — a second press replaces the first, so no
    // two questions about the same folder can be on screen disagreeing.
    pending: Pending | null;
  };

  // 🔴 Numbers, not a sentence: a sentence frozen at the moment of the click keeps its language through a
  // switch. And numbers READ ONCE, from one reply — re-deriving them per render
  // would let the question a person is answering renumber itself underneath
  // them.
  // 🔴 PR 8a, fix round 5. `of` is carried because the wait is now reached by
  // BOTH presses and the two are waiting for different things: the exclude
  // press is finding out what its rule removes, the include press is finding
  // out whether this row still names the folder the panel was opened for. One
  // sentence over both would be false of one of them, and the false half is the
  // include one — telling a person their removal is being costed when nothing
  // of the sort is being counted.
  type Pending =
    // `all` waits on the same identity check as `include` and reads its words.
    | { kind: 'checking'; path: string; of: 'exclude' | 'include' | 'all' }
    | { kind: 'exclude'; path: string; paths: number }
    // `heldBelow` is carried, not looked up when the question is drawn, for
    // the same reason `paths`/`documents` are: the question states what was
    // true when it was asked. It answers whether removing THIS rule still
    // leaves rules of the person's own further down the same path. See
    // `heldBelow` for why the sentence needs it.
    //
    // Only tree rows ask now (the rule list left the tree, owner 2026-11-12):
    // a row IS on disk, and an `excluded` row has no rule above it (the
    // listing reports an ancestor first), so neither "gone" nor "held from
    // above" can be the answer and neither is carried.
    //
    // `pushDown` (folders tree PR 2): the row sits under an excluded folder, so
    // ticking it takes that folder's rule away and rules out the siblings
    // instead — the sentence has to say new files there are indexed later.
    | {
        kind: 'include'; path: string;
        heldBelow: boolean; pushDown: boolean;
        // Folders tree PR 3: the row is one `.gitignore` holds, so the answer
        // stores an include and the sentence says so.
        gitignored: boolean;
      }
    // A mixed box was clicked: every rule strictly under `path` goes. `rules`
    // is how many, and `gitignored` how many rows `.gitignore` holds start
    // being indexed — both from `preview_tick`, the shell's own plan of the
    // press, frozen at the press like every other number here.
    | { kind: 'all'; path: string; rules: number; gitignored: number };

  let panels = $state<Record<number, Panel>>({});

  // Not `$state`: nothing renders from it. One counter per root, bumped by
  // every read AND by every collapse, so a listing still on the wire when the
  // row is shut — or when a newer read has started — is dropped instead of
  // being drawn over whatever the person is looking at now.
  const generations: Record<number, number> = {};

  // 🔴 PR 8a, fix round 4, item 2. The same shape one level up: `generations`
  // guards ONE panel's read, and there was nothing guarding the read of the
  // LIST. `refresh` is reachable from four places — `onMount`, an add, a
  // remove, and a job ending — so two `list_tree` calls can be on the wire at
  // once, and the one asked for first is not the one that has to answer first.
  // The older answer used to land behind the newer one and both replace `roots`
  // with a list that has been superseded AND prune the panels against it.
  //
  // One counter for the whole list rather than one per call site, for the
  // reason `generations` is one per root: what matters is whether this is still
  // the newest question asked, and the four callers are asking the same one.
  // Not `$state`: nothing renders from it.
  let refreshes = 0;

  // Task 6's own counter, and NOT `generations`. The rule, stated once rather
  // than as a list of sites that would drift from the code under it: it is
  // bumped wherever the answer to a question already in flight stops being
  // wanted, so a `list_tree` reply landing after that raises nothing. Held
  // apart from `generations` because that one is ALSO bumped by an ordinary
  // re-read, and a re-read finishing is not a reason to discard a question the
  // person is in the middle of reading.
  const asks: Record<number, number> = {};

  function ask(rootId: number): number {
    const n = (asks[rootId] ?? 0) + 1;
    asks[rootId] = n;
    return n;
  }

  function message(e: unknown) {
    return e instanceof Error ? e.message : String(e);
  }

  // Whether `.gitignore` holds a row of this level or under it — as if the
  // person had unticked it (owner's ruling 2026-11-12). The shell walks the
  // subtree for `gitignoredBelow`, so a collapsed folder knows it too (owner,
  // 2026-11-13: the tick must not change when the folder is opened).
  function holdsGitignored(entries: Subfolder[]): boolean {
    return entries.some((e) => e.state.kind === 'gitIgnored' || e.state.kind === 'gitIgnoredOpen' || e.gitignoredBelow);
  }

  function patch(rootId: number, fields: Partial<Panel>) {
    const panel = panels[rootId];
    if (panel === undefined) return; // the row was shut while this was running
    const next = { ...panel, ...fields };
    panels = { ...panels, [rootId]: next };
    if ((fields.rules || fields.tree) && next.rules) {
      // A row `.gitignore` names counts as unticked (owner's ruling
      // 2026-11-12), on any loaded level, as for a folder's mixed box.
      const some = next.rules.some((r) => r.existsOnDisk)
        || (next.tree !== null && holdsGitignored(next.tree.listing.entries));
      marks = { ...marks, [rootId]: { path: panel.rootPath, some } };
    }
  }

  // The mark of a shut root, asked of the shell. A panel read that starts while
  // this is on the wire bumps `generations` and answers itself, so this reply
  // is dropped; a failure leaves no mark rather than a `✓` that could be false.
  async function readMark(root: TreeRoot) {
    const generation = generations[root.rootId] ?? 0;
    let some: boolean;
    try {
      some = await rootMark(root.rootId);
    } catch {
      return;
    }
    if ((generations[root.rootId] ?? 0) !== generation) return;
    if (!roots.some((r) => r.rootId === root.rootId && r.absolutePath === root.absolutePath)) return;
    marks = { ...marks, [root.rootId]: { path: root.absolutePath, some } };
  }

  // A click on a root selects it for `−` and opens it; it never shuts it.
  function selectRoot(root: TreeRoot) {
    chosen = { rootId: root.rootId, path: root.absolutePath };
    if (panels[root.rootId] === undefined) toggleRoot(root);
  }

  function openPathsOf(node: SubTree | null): Set<string> {
    const paths = new Set<string>();
    const visit = (n: SubTree) => {
      for (const [path, child] of Object.entries(n.children)) {
        paths.add(path);
        visit(child);
      }
    };
    if (node !== null) visit(node);
    return paths;
  }

  async function fetchTree(rootId: number, path: string, want: Set<string>): Promise<SubTree> {
    const listing = await listSubfolders(rootId, path);
    const children: Record<string, SubTree> = {};
    for (const entry of listing.entries) {
      // `describe` decides what may be opened, and it is the same call the row
      // itself renders from — one classifier, so a subtree cannot appear under
      // a row that offers no control to open it.
      if (!want.has(entry.relativePath) || !describe(entry.state).expandable) continue;
      children[entry.relativePath] = await fetchTree(rootId, entry.relativePath, want);
    }
    return { listing, children };
  }

  // The disk and the stored rules, read together: the rule list is not
  // derivable from the listing (a rule may name a folder several levels down,
  // or one that is no longer there at all), and the listing is not derivable
  // from the rules.
  async function read(rootId: number, want: Set<string>) {
    const generation = (generations[rootId] ?? 0) + 1;
    generations[rootId] = generation;
    try {
      const [tree, rules] = await Promise.all([
        fetchTree(rootId, '', want),
        listExclusions(rootId),
      ]);
      if (generations[rootId] !== generation) return;
      patch(rootId, { tree, rules, loadError: null });
    } catch (e) {
      if (generations[rootId] !== generation) return;
      // Both cleared: half a listing beside a sentence saying it could not be
      // read is a screen that contradicts itself.
      patch(rootId, { tree: null, rules: null, loadError: message(e) });
    }
  }

  // Takes the ROW and not its id (fix round 4, item 1): the panel has to record
  // which folder it was opened for, and the only place that knows it without a
  // second lookup is the row the person pressed. A `roots.find(...)` here would
  // be the same fact read a second time, out of a list a refresh can replace
  // between the render and the click.
  function toggleRoot(root: TreeRoot) {
    const rootId = root.rootId;
    if (panels[rootId] !== undefined) {
      generations[rootId] = (generations[rootId] ?? 0) + 1;
      ask(rootId); // a question the person shut the row on is not asked again
      const next = { ...panels };
      delete next[rootId];
      panels = next;
      return;
    }
    ask(rootId);
    panels = {
      ...panels,
      [rootId]: {
        rootPath: root.absolutePath,
        tree: null, rules: null, loadError: null, actionError: null,
        pending: null,
      },
    };
    void read(rootId, new Set());
  }

  // Shutting a subfolder drops its subtree here and now, without a read: the
  // person asked for less on screen, and nothing about the folders that remain
  // has changed. Opening one re-reads the whole panel — the row that was
  // pressed is not the only one whose state a rule added elsewhere can change.
  function toggleSubfolder(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined || panel.tree === null) return;
    const paths = openPathsOf(panel.tree);
    if (paths.has(path)) {
      patch(rootId, { tree: prune(panel.tree, path) });
      return;
    }
    void read(rootId, new Set([...paths, path]));
  }

  function prune(node: SubTree, path: string): SubTree {
    const children: Record<string, SubTree> = {};
    for (const [key, child] of Object.entries(node.children)) {
      if (key === path) continue;
      children[key] = prune(child, path);
    }
    return { listing: node.listing, children };
  }

  // ── PR 8a, Task 6: what an exclusion costs, said before it is stored ──────
  //
  // 🔴 Mirrored from what an exclusion rule actually covers: a path lies under
  // a prefix only across a separator. `drop2/y.md` is a SIBLING of `drop`, not
  // a child — `anchored_pattern` produces `!/drop`, which does not match it —
  // so a count written with `startsWith(prefix)` alone would tell a person that
  // `drop2` disappears too. It is the one state in the fixture list that
  // separates the two forms.
  //
  // 🔴 Review round 1, M2. The sentence above used to say "mirrored from
  // `walk.rs`'s own `under`", and that was the wrong Rust function: it is
  // called from two places (`walk.rs:696`, `walk.rs:768`) and both pass a
  // FROZEN prefix; it never sees an exclusion rule. What decides whether a
  // RULE covers a path is
  // `mnema-walk/src/rules.rs:522`'s `anchored_pattern` — `!/{escaped}` — read
  // by `ignore`'s gitignore line parser. This function mirrors THAT, and the
  // two Rust functions agree today because both are right, not because one
  // derives from the other.
  //
  // What makes it drift: a change to `anchored_pattern` or to how that pattern
  // is matched changes what actually disappears, while this count keeps
  // answering by the old rule — so a person is told a number for a deletion
  // that will not happen, or is told nothing about one that will. Under D29 the
  // second direction leaves text in the index and sends it to a third-party
  // provider.
  //
  // What would catch it is a PAIR, neither half sufficient alone:
  // `Folders.test.ts`'s `a sibling whose name merely starts with the prefix is
  // not counted` on this side, and
  // `a_user_prefix_does_not_remove_a_sibling_whose_name_starts_with_it`
  // (`crates/mnema-walk/tests/rules.rs`) on Rust's — which review round 1 had
  // to write, because no Rust fixture paired a prefix with such a sibling. The real fix, still open, is
  // `list_tree` carrying the count so there is one rule and no copy.
  function under(relativePath: string, prefix: string): boolean {
    return relativePath.startsWith(`${prefix}/`);
  }

  // What a rule on `prefix` takes away: the path itself as well as everything
  // under it. `!/a/b.pdf` names the FILE too (folders tree PR 2, open question
  // 1), so a count over `under` alone answers 0 for a file and the exclusion
  // would skip its question.
  function covers(relativePath: string, prefix: string): boolean {
    return relativePath === prefix || under(relativePath, prefix);
  }

  // 🔴 Fix round 1, I2. Whether a rule of the person's own remains STRICTLY
  // under `prefix` — the fact two of this screen's sentences were written
  // without and are false in a state this very screen can reach.
  //
  // The state is storable and nothing refuses it: `exclude_subfolder` checks an
  // unknown root, a blank path, the validator and the built-in layers and
  // nothing else (`bridge.rs:450-483` — there is no ancestor guard), and
  // `add_path_exclusion` is `INSERT … ON CONFLICT DO NOTHING`
  // (`write.rs:604-612`), so a rule on `Archive/Held` and a rule on `Archive`
  // are both stored. `subfolder_state` then still reports `Archive` as
  // `Excluded` (`tree.rs:817-848` asks about an ancestor, and `Archive` has
  // none), so this screen offers to remove it — and "anything at this path is
  // indexed again" was contradicted by the rule list two lines below it.
  // Probed rather than reasoned: both orders of the two calls answer `Ok` and
  // `list_exclusions` comes back with both prefixes, which
  // `the_outermost_rule_is_the_one_a_held_subfolder_names` and
  // `a_rule_stored_under_one_that_arrives_later_is_kept_and_still_names_the_outermost`
  // (`src-tauri/tests/commands.rs`) now pin.
  //
  // 🔴 And the rule this condition names is one the TREE never shows. A folder
  // held by two rules reports the OUTERMOST (`subfolder_state`'s own choice),
  // so the row for `Archive/Held` says "held by your rule on Archive" and its
  // own rule appears nowhere above. It appears in the RULE LIST of this same
  // panel, which is `list_exclusions`' whole answer for the root — which is
  // what makes "your other rules further down this path" something a person
  // can go and find rather than a sentence about nothing they can see.
  // `Folders.test.ts`'s "the pair reads as one screen" reads both halves.
  //
  // That is an invariant rather than a coincidence, and it is why the sentence
  // needs no path in it: this function and the rule list are handed the SAME
  // `panel.rules`, so a `true` here cannot name a rule the panel is not also
  // drawing a row for.
  //
  // Through `under`, so the boundary rule is the one the cost count already
  // uses: a rule on `Archive2` is a sibling of `Archive`, not a rule under it,
  // and `anchored_pattern` agrees. Strict, so the rule being removed is never
  // its own answer.
  //
  // `null` is a panel whose read failed, which draws neither a rule row nor a
  // subfolder row and cannot reach either caller. It answers `false`, and that
  // is the direction to answer it in: `false` selects the UNCONDITIONAL
  // sentence, which claims more comes back than does. Under D29 an
  // over-statement of what reaches the provider is the safe way round.
  //
  // 🔴 Fix round 2, A2: `existsOnDisk`, and it is the same direction again. The
  // softened sentence promises an EXCEPTION, and without this filter the rule
  // backing that promise could name a path with nothing on disk. At the next
  // scan such a rule excludes nothing, so
  // the exception understates what reaches the provider — the one direction
  // D29 does not allow, and the reason this is filtered rather than left with a
  // written excuse. Dropping the exception selects the unconditional sentence,
  // which is not merely the safe side here but exactly true: everything present
  // at this path really is indexed again.
  //
  // Per rule, never over the list as a whole: a rule whose folder is gone must
  // not veto a sibling that is doing its job, which
  // `a rule below whose folder is gone does not cancel one whose folder is there`
  // pins against an `every`-shaped reading of the same fact.
  function heldBelow(rules: StoredExclusion[] | null, prefix: string): boolean {
    return rules !== null
      && rules.some((rule) => rule.existsOnDisk && under(rule.prefix, prefix));
  }


  // This root's indexed paths under `prefix`, from ONE `list_tree` reply: a
  // relative path means nothing outside the root it belongs to.
  function costOf(listing: TreeListing, rootId: number, prefix: string) {
    const root = listing.roots.find((r) => r.rootId === rootId);
    const paths = root?.files.filter((f) => covers(f.relativePath, prefix)).length ?? 0;
    return { paths };
  }

  // ── PR 8a, fix round 5: an answer about a root is about THAT root ─────────
  //
  // 🔴 `watched_root.id` is `INTEGER PRIMARY KEY` with no `AUTOINCREMENT`
  // (`schema.sql:11-15`), so it is a rowid alias and SQLite hands a deleted id
  // out again in the ordinary case: remove the folder added last, add another,
  // and the new one gets the old id. Round 4 wrote the consequence into
  // `refresh` — a panel is kept only while the id AND the path still agree —
  // and it is written HERE now, once, as the one function all three sites call
  // rather than as three copies of one comparison. What this closes is exactly
  // two sites drifting apart: round 4's comparison went into `refresh` while
  // `askExclude` went on matching a fresh listing on the id alone (so the
  // question quoted another folder's file and document counts under this
  // folder's row) and `askInclude` read no listing at all (so removing a rule
  // could take the protection off a folder the person never pointed at — under
  // D29, that folder's text on its way to the provider at the next scan).
  //
  // `===` against a value that is `undefined` when the id has gone, so ONE
  // comparison answers both cases — the folder has left, and the id now names a
  // different folder — and neither can be fixed without the other.
  //
  // The listing is the CALLER's and is never read again here: what makes this
  // an answer rather than a guess is that the reply deciding the identity is
  // the same reply the numbers are counted from.
  //
  // The limit, stated rather than left silent: this settles the identity at the
  // moment the question is raised, and a person then sits in front of that
  // question for as long as they like. A remove-and-add landing in THAT gap is
  // caught only if a `refresh` happens to run before the answer — every
  // `refresh` prunes the panel and takes the question with it — and not
  // otherwise. Closing that gap costs a `list_tree` on every confirmation,
  // which is a decision about the whole action flow rather than about this
  // rule, and it is not taken here.
  function namesFolder(listing: TreeListing, rootId: number, rootPath: string): boolean {
    return listing.roots.find((root) => root.rootId === rootId)?.absolutePath === rootPath;
  }

  // 🔴 What both presses do when the id and the path disagree, in ONE place
  // because there is one answer and neither press may invent its own.
  //
  // Nothing is stored and the panel is not corrected: `Panel.rootPath` already
  // says a panel whose folder has changed is not a panel to correct, it is one
  // to drop. It is not dropped here either — `refresh` owns that, applying
  // `namesFolder` to every panel at once, and re-reading the list is also what
  // puts the folder that IS at this id on screen. A bare refusal leaving the
  // stale row standing would answer the next press the same way for ever.
  //
  // The question goes synchronously all the same, ahead of that re-read: the
  // re-read is slow and can fail, and the `checking` sentence must not stand
  // over either.
  //
  // No `ask` bump here, deliberately, and this is not the missing half of one:
  // the only holder of this root's current ask generation is the caller, which
  // returns on the next line, and the prune inside `refresh` bumps the counter
  // itself as it drops the panel. A second bump would be a guard whose mutant
  // dies from its neighbour.
  function abandonChangedFolder(rootId: number) {
    patch(rootId, { pending: null });
    rootChanged = true;
    reread(false); // no ending, so nothing to withdraw
  }

  // 🔴 The re-read is not a nicety: a count taken from the listing this window
  // already holds is a number about a moment that has passed — a job may have
  // added or removed paths since the row was drawn.
  async function askExclude(rootId: number, path: string, isDir = true) {
    // Captured before the await, and only the fields that may be: `rootPath` is
    // written once at `toggleRoot` and never patched, so it cannot have moved
    // underneath. A row shut and reopened under the same id during the await is
    // a different panel, and `toggleRoot` bumps `asks` twice getting there, so
    // the generation check below has already returned by then.
    const panel = panels[rootId];
    if (panel === undefined) return;
    const generation = ask(rootId);
    rootChanged = false;
    abandonRemoveQuestion(); // a press elsewhere in this list: see the function
    patch(rootId, {
      actionError: null,
      pending: { kind: 'checking', path, of: 'exclude' },
    });
    let listing: TreeListing;
    try {
      listing = await listTree();
    } catch (e) {
      if (asks[rootId] !== generation) return;
      // The rule is NOT stored, and no loss sentence is shown: this window
      // could not find out what the loss is, and a confirmation over a number
      // it could not read would be worse than none. §10 — what crossed is a
      // sentence, so the sentence is what appears.
      patch(rootId, { pending: null, actionError: message(e) });
      return;
    }
    if (asks[rootId] !== generation) return;
    // 🔴 Before `costOf` and not after it: that function keys on the id alone,
    // and a `relativePath` means nothing outside the root it belongs to. With
    // the id handed to another folder, both numbers are that folder's and the
    // path stored on a confirmation would be this folder's — see `namesFolder`.
    if (!namesFolder(listing, rootId, panel.rootPath)) {
      abandonChangedFolder(rootId);
      return;
    }
    const cost = costOf(listing, rootId, path);
    // No question over nothing: a confirmation a person can always click
    // through is training for the one that matters. Nor over one file (owner,
    // 2026-11-12): ticking it back and scanning puts it back.
    if (cost.paths === 0 || !isDir) {
      patch(rootId, { pending: null });
      await exclude(rootId, path);
      return;
    }
    patch(rootId, { pending: { kind: 'exclude', path, ...cost } });
  }

  // 🔴 A re-read now, and deliberately still no count. Fix round 5 gave this
  // function the `list_tree` it never had, read for ONE fact and no other: that
  // this id still names the folder this panel was opened for. Without it this
  // was the worse half of round 4's own defect — the exclude question merely
  // quoted another folder's numbers, while this one went straight to
  // `include_subfolder` with an id that could belong to a folder the person
  // never pointed at, taking a rule off it. Over-warning is a nuisance;
  // silently removing protection is the direction D29 does not allow.
  //
  // The count stays absent for the reason it always was: this window does not
  // know what is on disk under a folder the walk has been pruning, so a number
  // here would be invented; what IS known is the consequence, and that is what
  // the sentence states.
  //
  async function askInclude(
    rootId: number, path: string, pushDown = false, gitignored = false, isDir = true,
  ) {
    // Captured before the await for `askExclude`'s reason, and `rules` is read
    // from that same capture on purpose: the question states what was true when
    // it was ASKED.
    const panel = panels[rootId];
    if (panel === undefined) return;
    const generation = ask(rootId);
    rootChanged = false;
    abandonRemoveQuestion(); // a press elsewhere in this list: see the function
    patch(rootId, {
      actionError: null,
      pending: { kind: 'checking', path, of: 'include' },
    });
    let listing: TreeListing;
    try {
      listing = await listTree();
    } catch (e) {
      if (asks[rootId] !== generation) return;
      // The rule is NOT removed and no question is raised: this window could
      // not find out whether the id still names this folder, and under D29 a
      // removal taken on an answer it never read is the one direction that
      // cannot be undone — the folder's text is at the provider after the next
      // scan. §10 — what crossed is a sentence, so the sentence is what appears.
      patch(rootId, { pending: null, actionError: message(e) });
      return;
    }
    if (asks[rootId] !== generation) return;
    if (!namesFolder(listing, rootId, panel.rootPath)) {
      abandonChangedFolder(rootId);
      return;
    }
    // Owner, 2026-11-12: a file the person unticked names exactly what comes
    // back, so no question. Not a push-down (it changes the folders above) and
    // not a `.gitignore` row (the person never chose to leave it out).
    if (!isDir && !pushDown && !gitignored) {
      patch(rootId, { pending: null });
      await include(rootId, path, 'on');
      return;
    }
    patch(rootId, {
      // From the capture two lines up, not from `panels[rootId]` here: the
      // question states what was true when it was asked.
      pending: {
        kind: 'include', path,
        heldBelow: heldBelow(panel.rules, path),
        pushDown,
        gitignored,
      },
    });
  }

  // Folders tree PR 2: the mixed box. The same identity check as `askInclude`
  // — under D29 taking rules away on an id that may name another folder is the
  // direction that cannot be undone — and the count of rules that go, read
  // from the same capture.
  async function askAll(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const generation = ask(rootId);
    rootChanged = false;
    abandonRemoveQuestion();
    patch(rootId, {
      actionError: null,
      pending: { kind: 'checking', path, of: 'all' },
    });
    let listing: TreeListing;
    try {
      listing = await listTree();
    } catch (e) {
      if (asks[rootId] !== generation) return;
      patch(rootId, { pending: null, actionError: message(e) });
      return;
    }
    if (asks[rootId] !== generation) return;
    if (!namesFolder(listing, rootId, panel.rootPath)) {
      abandonChangedFolder(rootId);
      return;
    }
    // Folders tree PR 3: the shell plans the press and writes nothing — which
    // rows `.gitignore` holds is the walk's own layer's answer, never this
    // window's (D202). A failure stores nothing and says why.
    let change;
    try {
      change = await previewTick(rootId, path, 'all');
    } catch (e) {
      if (asks[rootId] !== generation) return;
      patch(rootId, { pending: null, actionError: message(e) });
      return;
    }
    if (asks[rootId] !== generation) return;
    patch(rootId, {
      pending: { kind: 'all', path, rules: change.removed.length, gitignored: change.included.length },
    });
  }

  // What is stored is the path the QUESTION carries, and it is read from the
  // question rather than from anything on screen: the listing under an open
  // question can be redrawn by a re-read, and the sentence the person read
  // named one folder.
  //
  // 🔴 Exhaustive over the three kinds with no default arm, and `checking` is
  // why. This is NOT a second guard over the markup's decision not to draw a
  // control in that state — an `if (kind === 'exclude') … else …` would route
  // `checking` into `include`, which under D29 takes a person's exclusion rule
  // away and sends that folder's text to the provider. The two answers differ,
  // so both are written; `describe`'s default arm explains the case where they
  // do not.
  async function answer(rootId: number) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const pending = panel.pending;
    if (pending === null) return;
    switch (pending.kind) {
      case 'checking':
        return; // no answer has been offered yet; there is nothing to store
      case 'exclude':
        ask(rootId);
        patch(rootId, { pending: null });
        await exclude(rootId, pending.path);
        return;
      case 'include':
        ask(rootId);
        patch(rootId, { pending: null });
        await include(rootId, pending.path, 'on');
        return;
      case 'all':
        ask(rootId);
        patch(rootId, { pending: null });
        await include(rootId, pending.path, 'all');
        return;
    }
  }

  function dismiss(rootId: number) {
    if (panels[rootId] === undefined) return;
    ask(rootId);
    patch(rootId, { pending: null });
  }

  async function exclude(rootId: number, path: string) {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const want = openPathsOf(panel.tree);
    patch(rootId, { actionError: null });
    try {
      // One `set_tick`: a fold-back, if the folder empties, lands in the same
      // transaction (`ticks.rs`).
      await setTick(rootId, path, 'off');
      unapplied.set(true);
    } catch (e) {
      patch(rootId, { actionError: message(e) });
    }
    // Unconditional, and outside the `try`: `exclude_subfolder` can refuse a
    // path this listing showed (`Error::AlreadyPrunedByBuiltIn`), and what the
    // rows must say next is decided by re-reading the state — never by parsing
    // a rejection, which crosses the IPC as a sentence and nothing else.
    await read(rootId, want);
  }

  async function include(rootId: number, path: string, tick: 'on' | 'all') {
    const panel = panels[rootId];
    if (panel === undefined) return;
    const want = openPathsOf(panel.tree);
    patch(rootId, { actionError: null });
    try {
      // One `set_tick`: a push-down is decided and written by the shell in one
      // transaction, never as several calls from here (spec C).
      await setTick(rootId, path, tick);
      unapplied.set(true);
    } catch (e) {
      patch(rootId, { actionError: message(e) });
    }
    await read(rootId, want);
  }

  async function refresh() {
    const generation = (refreshes += 1);
    let listing: TreeListing;
    try {
      listing = await listTree();
    } catch (e) {
      // 🔴 A stale REJECTION is stale too, and it is the half that a guard
      // written for the resolved path alone would leave standing. All four
      // callers turn a rejection from here into `loadError`, so an older
      // failure landing behind a newer success would print "the list of folders
      // could not be read" over a list that has just been read successfully —
      // the exact state `loadError = null` below exists to prevent. Swallowed
      // here rather than at the four call sites, because it is one fact about
      // one reply and four copies of it would be four places to forget.
      if (refreshes !== generation) return;
      throw e;
    }
    // 🔴 PR 8a, fix round 4, item 2. Nothing below this line may run for an
    // answer that has been overtaken: not `roots`, not `panels`, not
    // `loadError`. See `refreshes` for what overlaps and why.
    if (refreshes !== generation) return;
    roots = listing.roots;
    // 🔴 Fix round 1, I1. The removal question is held to the SAME identity
    // rule as the panels below, against the same listing and through the same
    // `namesFolder`, because it is drawn under a row and can outlive it. It is
    // withdrawn only by `readSeq` growth, so a plain re-read — a mount, an add,
    // a removal's own answer, or any ending that moves no reading counter —
    // leaves it standing by design; and the identity underneath it can move
    // between two of them. `watched_root.id` is a rowid alias (see the panel
    // prune below), so a remove-and-add elsewhere hands this id to another
    // folder.
    //
    // Both cases the one comparison answers are real and neither is quiet: with
    // the id still present and naming something else, a question saying
    // "Remove folder /synthetic/root — 3 files" is drawn directly beneath a row
    // that now reads another path, and its Confirm can only earn a refusal
    // (`bridge.rs` compares the frozen path). With the root gone from the
    // listing, NO row renders the question at all, so the press disappears with
    // no word — the falsehood `withdrawQuestions` exists to prevent.
    //
    // The note and not `rootChanged`: this one names the folder the question
    // was about, which is the fact a person needs to press again, and
    // `rootChanged` is a boolean that cannot carry it.
    if (removeQuestion !== null
        && !namesFolder(listing, removeQuestion.rootId, removeQuestion.path)) {
      removeQuestion = null;
    }
    // An expansion belongs to a row, and outlives it for no longer than the
    // row itself: `remove_watched_folder` deletes a database row whose id
    // SQLite may hand out again, and an expansion left behind under that id
    // would draw one folder's subfolders under another folder's path.
    //
    // 🔴 PR 8a, fix round 4, item 1. That sentence was already written here,
    // and it covered ONE of the hazard's two cases — "the id is gone". The
    // other case is the one rowid reuse actually produces: the id is STILL
    // THERE and now names a different folder. `watched_root.id` is
    // `INTEGER PRIMARY KEY` with no `AUTOINCREMENT` (`schema.sql:11-15`), so it
    // is a rowid alias and SQLite hands a deleted id out again — remove the
    // most recently added root, add another, and the new one gets the old id.
    // That is the ordinary case, not an exotic one. Kept across it, the panel
    // draws the OLD folder's subfolders under the NEW folder's path, and its
    // controls then exclude or include on the new root carrying a
    // `relativePath` from the old one — under D29, a rule the person never
    // asked for, or a folder's text sent to the provider.
    //
    // This is the class this branch has already paid for three times: a seam
    // covering half of its own sites, written by somebody who was thinking
    // about the hazard and named it in the comment above.
    //
    // No new identifier scheme, deliberately: the id stays the key, and what is
    // compared is the path the listing gives against the path the panel was
    // built for (`Panel.rootPath`).
    //
    // 🔴 PR 8a, fix round 5: through `namesFolder` and not through a comparison
    // written here. Round 4 left this comparison at one site while the two
    // questions asked the id alone; one function is what stops the next site
    // from being a third copy that can drift.
    const kept: Record<number, Panel> = {};
    for (const [key, panel] of Object.entries(panels)) {
      const rootId = Number(key);
      if (namesFolder(listing, rootId, panel.rootPath)) {
        kept[rootId] = panel;
        continue;
      }
      // The panel goes and the two counters that outlive it are bumped with it,
      // exactly as a collapse does in `toggleRoot`: a `list_subfolders` still on
      // the wire for the folder that has left must not be drawn into a panel
      // opened later under the reused id, and a question asked about that folder
      // must raise nothing when its reply lands.
      //
      // ⚠️ Stated rather than claimed: neither bump has a reachable mutant
      // today, so no test here pins them and none pretends to. Every way back
      // into this id runs through `toggleRoot`, which bumps both itself, and
      // `read` bumps `generations` at the start of every read — so a stale reply
      // is refused by a neighbouring defence on every path measured. They are
      // written because the ownership rule is "the panel's counters die with the
      // panel", and a rule kept only where it is currently observable is a rule
      // the next call site will not inherit.
      generations[rootId] = (generations[rootId] ?? 0) + 1;
      ask(rootId);
    }
    if (Object.keys(kept).length !== Object.keys(panels).length) panels = kept;
    loadError = null; // a successful read is proof the earlier one is stale
    if (chosen !== null && !namesFolder(listing, chosen.rootId, chosen.path)) chosen = null;
    // D202: a folder added from the launcher arrives here unconfirmed. Nothing
    // chosen yet — open it, so its ticks and Start are what the window shows.
    if (chosen === null) {
      const waiting = listing.roots.find((r) => !r.confirmed);
      if (waiting !== undefined) selectRoot(waiting);
    }
    // Owner, 2026-11-13: every root's ✓ or – is there when the window opens,
    // not only after its panel has been read once. After `selectRoot`, and not
    // for an open panel: its own read gives the mark, and would drop this
    // reply after the shell had walked the whole root for nothing.
    for (const root of listing.roots) {
      if (panels[root.rootId] === undefined) void readMark(root);
    }
  }

  // ── PR 8a, Task 8. Live run, finding 1: what a finished scan leaves behind ─
  //
  // `refresh` re-reads the ROW — the roots and their counts — and nothing under
  // it. An expanded panel is read from two other places, the disk
  // (`list_subfolders`) and the stored rules (`list_exclusions`), and a scan
  // ending is exactly the moment both can have moved: measured, `archive` was
  // renamed to `Archive` between two scans, the walk indexed the folder again
  // because the rule still names the old spelling byte for byte, and the row
  // went on reading "archive — excluded by your rule" while that folder's text
  // was on its way to the model provider (D29). Collapsing and re-expanding by
  // hand was the only thing that corrected it.
  //
  // This is PR 7's finding 2 one level deeper, and the asymmetry argument
  // written at the subscription below is the same one: a re-read that finds the
  // listing unchanged rewrites it invisibly, a missed one leaves a falsehood a
  // person can act on.
  //
  // 🔴 EVERY open panel, each with the subfolders that panel currently has
  // open, so an ending that arrives while three roots are expanded does not
  // shut two of them. Started together rather than one after another: the roots
  // are independent — `read` checks THIS root's generation and `patch` writes
  // into THIS root's panel — so the order between them is not observable, and
  // awaiting one would hold the others stale for the length of its call.
  //
  // The limit, stated rather than left silent: `want` is taken from the tree
  // the panel HOLDS, so an expand still on the wire when the ending lands is
  // discarded with every other in-flight read and that subfolder does not open.
  // `exclude` and `include` compute `want` the same way and have the same
  // window; the alternative is a second record of what is open, and
  // `panel.tree` already answers that question.
  //
  // 🔴 PR 8a, Task 8 fix round 2, and the finding still stands even though the
  // shape it was written against is gone. The RE-READ below is unconditional —
  // any phase of a scan moves `list_exclusions`' `existsOnDisk` and
  // `list_subfolders` (D29 sends the same rename window through either). The
  // WITHDRAWAL is narrower, because it is spelled "indexing has finished and
  // this panel was read again": what makes a pending question wrong is a
  // READING pass having run, which is what moved the two numbers `Pending`
  // freezes. An embedding pass changes no rule and no file count for the
  // folder the question is about, so a still-open press has nothing
  // invalidated to withdraw it over. Reviewed and reproduced: raising an
  // exclude question while the embedding ran, then letting it end, used to
  // discard the press and print "indexing has finished" when none had.
  //
  // 🔴 That distinction IS drawn, and the fact it is drawn on is
  // `ScanState.readSeq` — how many reading passes have ENDED in this process
  // (`ipc.ts:734-745`). The chained pass the finding was first expressed in
  // terms of no longer exists; the counter is the durable form of the same
  // fact, and it is the only field that tells apart the two endings a person
  // sees as one. See the subscription below for where it is compared.

  // 🔴 The question goes, and not in silence, when a READING PASS ends.
  // Its two numbers were read from a `list_tree` taken BEFORE that scan,
  // and `Pending` freezes them on purpose: they cannot be corrected in
  // place without renumbering a sentence somebody is part way through
  // reading, and they cannot be left standing, because a reading pass ending
  // is precisely the event that makes them wrong. The include question is no
  // safer — it carries `existsOnDisk`, the one fact the rename that
  // produced this defect invalidated. So the question is withdrawn and the
  // panel says which folder it was about; pressing again asks it afresh
  // against the state that is now on screen.
  //
  // `ask` and not `generations`: this is that counter's own meaning — the
  // answer to a question already in flight has stopped being wanted — so a
  // `checking` reply still on the wire raises nothing when it lands.
  //
  // 🔴 Fix round 1, I1. Its own function, called from `reread` BEFORE any I/O
  // starts, and that placement is the whole finding. It used to sit at the top
  // of `rereadPanels`, which runs inside `refresh().then(…)` — so a rejected
  // `list_tree` at a scan's ending took the withdrawal down with it, and the
  // ending is consumed exactly once (`seen` advances first), so it never came
  // back. The next successful `refresh` — a later ending, an add, a remove,
  // none of which withdraw anything — then cleared
  // `loadError` and redrew the panel WITH the question still standing, stating
  // pre-scan numbers as current, though a scan had happened underneath it.
  //
  // The question is made wrong by the ENDING, not by a successful re-read, so
  // nothing about withdrawing it may depend on a call that can fail. The
  // direction is chosen and it is the safe one: a question withdrawn once too
  // often costs a second press, a question left standing states frozen numbers
  // as current, and under D29 the include question left standing is a person
  // being asked to unprotect a folder on facts a scan has already moved.
  //
  // 🔴 Task 9. BOTH questions, in one function, because one event invalidates
  // both: the removal question's `files` was read from a `list_tree` taken
  // before that reading pass, exactly as the exclude question's two numbers
  // were. Its note is the same sentence about a different subject — the watched
  // folder's own path rather than a subfolder's relative one — and it is drawn
  // under the list, where the question was.
  function withdrawQuestions() {
    for (const [key, panel] of Object.entries(panels)) {
      if (panel.pending === null) continue;
      const rootId = Number(key);
      ask(rootId);
      patch(rootId, { pending: null });
    }
    removeQuestion = null;
  }

  function rereadPanels() {
    for (const [key, panel] of Object.entries(panels)) {
      void read(Number(key), openPathsOf(panel.tree));
    }
  }

  function reread(withdraw: boolean) {
    // Synchronous, and first: see `withdrawQuestions`. It reads no I/O and
    // cannot fail, so there is no path on which a reading pass's ending leaves
    // a pending question standing.
    //
    // The argument stays a boolean, and the fact behind it is decided by the
    // ONE caller that can answer it — the subscription below, which is the only
    // place that holds the last `readSeq` this component acted on. Deciding it
    // here would need that memory in two places.
    if (withdraw) withdrawQuestions();
    // Panels after the roots, never beside them: `refresh` is what deletes the
    // expansion of a root that has gone, and a `list_subfolders` fired for that
    // root would answer with a rejection drawn into a panel about to vanish.
    refresh().then(rereadPanels).catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
  }

  // Folders tree PR 2: a mask stored or removed changes which files the open
  // levels mark `mask …`, so each open panel is read again. The subscription's
  // first call is the current value, not a change, and reads nothing.
  onMount(() => {
    let first = true;
    return masksVersion.subscribe(() => {
      if (first) { first = false; return; }
      for (const id of Object.keys(panels).map(Number)) {
        const panel = panels[id];
        if (panel !== undefined) void read(id, openPathsOf(panel.tree));
      }
    });
  });

  onMount(() => {
    reread(false); // no ending yet, and no panel is open to withdraw
    // Live run, finding 2. Task 7 re-reads after an add and after a remove;
    // the event nobody wired is the one that changes the NUMBER this list shows
    // — a job ending. The row went on stating zero indexed documents while the
    // report under it said four had been added, and the index agreed with the
    // report, not with the row.
    //
    // Task 8 widened what that ending re-reads from the row to the panel under
    // it as well — see `rereadPanels`, which is where the second half of this
    // finding is written up.
    //
    // The RE-READ fires on EVERY ending, and the reason is asymmetry rather
    // than caution: a re-read that finds the same numbers rewrites them
    // invisibly, while a missed one leaves a falsehood a person can act on. A
    // scan is one job over every watched folder and its report names no root,
    // so there is nothing to narrow the re-read BY in any case. Endings are
    // rare: at most a handful per run, never one per progress tick.
    //
    // The WITHDRAWAL is narrower — see `withdrawQuestions`' own comment.
    //
    // Compared by snapshot IDENTITY, not by kind: the controller replaces the
    // whole state on every change, so a progress tick changes the object
    // without ever being an ending, and an ending is written exactly once. The
    // guard is load-bearing in one direction that no kind check would cover:
    // `jobs.ts` writes a NOTE into the same store without touching `scan`, and
    // an ended snapshot already consumed must not be consumed a second time
    // because a sentence beside it changed. Seeded with what the store already
    // holds, so an ending that happened before this component existed is not
    // read as one that happened under it. The case that seeding still answers
    // is the WINDOW opening on a run it never started (`Settings.svelte`'s own
    // note on `jobs.mount`: the tray can start a scan, and the settings window
    // can be opened in the middle of it) — F10 (Task 10e) retired the other
    // one, a nav click back into this section, because this section is no
    // longer unmounted by a nav click at all.
    //
    // 🔴 The WITHDRAWAL is keyed on `ScanState.readSeq` — how many reading
    // passes have ENDED — and NOT on the snapshot becoming `ended`. Two states
    // separate the two rules, and each one costs something real:
    //
    //   • an `embedOnly` run ends like any other and reads no folder, so
    //     nothing it did made a frozen number wrong. Withdrawing there
    //     discards a press and prints "indexing has finished" over a run
    //     that read nothing.
    //   • the embedding phase of a `full` run arrives as `running` with the
    //     counter ALREADY moved. The reading pass has ended by then, so every
    //     number a question froze is stale — and no `ended` snapshot has been
    //     seen yet. This is why the comparison is on the SNAPSHOT'S `readSeq`,
    //     whatever kind carried it.
    //
    // The counter is seeded from the store rather than from zero: a pass that
    // ended before this component existed must not be read as one that ended
    // under a question it never saw. Since F10 (Task 10e) this component is
    // mounted once per WINDOW, so the state that reaches the seeding is the
    // window being opened on a run already going — not a nav click, which no
    // longer rebuilds this section at all.
    //
    // The RE-READ stays on the ending, where it is right: any phase can move
    // what the panels draw, and a re-read that finds the same numbers rewrites
    // them invisibly. Both are reached from one `reread(true)` when the counter
    // moves, in that order — the withdrawal is synchronous and cannot fail, the
    // re-read can (fix round 1, I1).
    let seen: ScanSnapshot = get(jobs.state).scan.snapshot;
    let seenReadSeq = get(jobs.state).scan.readSeq;
    return jobs.state.subscribe(({ scan }) => {
      if (scan.snapshot === seen) return;
      seen = scan.snapshot;
      if (scan.readSeq > seenReadSeq) {
        seenReadSeq = scan.readSeq;
        reread(true);
      } else if (scan.snapshot.kind === 'ended') {
        reread(false);
      }
    });
  });

  // 🔴 Task 9. A press about something else in this list abandons the removal
  // question. The rule is the one
  // `rootChanged` is written under — what the last press led to stands until
  // the next one — and its three sites are the three presses that start
  // something here besides the removal itself: `askExclude`, `askInclude` and
  // `addFolder`. Not `toggleRoot`: opening a row asks nothing and answers
  // nothing, and a question dismissed by a person looking at what is inside the
  // folder they are about to remove would be dismissed for looking.
  function abandonRemoveQuestion() {
    removeQuestion = null;
  }

  // D202: the person's press for the folders added through this window — the
  // one "Apply changes" under the masks since 2026-11-12 (owner). Each waiting
  // folder is confirmed (the backend lets scans read it and starts one); with
  // none waiting the press is a plain full scan. Only over a list that is on
  // screen: `confirm_watched_folder` takes an id alone, and after a failed
  // re-read an id from the last good listing may name another folder.
  async function applyChanges() {
    const waiting = loadError === null ? roots.filter((r) => !r.confirmed) : [];
    if (waiting.length === 0) {
      await jobs.scan('full');
      return;
    }
    actionError = null;
    // The note asked for this press; once it is made the note is stale, even
    // when the scan behind it is refused — the folder is confirmed by then.
    folderAdded = false;
    for (const root of waiting) {
      try {
        await confirmWatchedFolder(root.rootId);
      } catch (e) {
        actionError = e instanceof Error ? e.message : String(e);
      }
    }
    // Re-read on a refusal too: the backend confirms before it starts the scan,
    // so a scan that would not start leaves a confirmed folder, and the row
    // must show it as the backend has it. The button stays disabled until then
    // (`Apply.svelte`), or a second press queues a second scan.
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    applyAction.set(applyChanges);
    return () => applyAction.set(null);
  });

  $effect(() => {
    foldersWaiting.set(loadError === null && roots.some((r) => !r.confirmed));
  });

  async function addFolder() {
    actionError = null;
    rootChanged = false; // what the last press led to stands until the next one
    folderAdded = false; // same rule, same place, for the note below
    abandonRemoveQuestion();
    const selected = await open({ directory: true });
    if (selected === null) return; // cancelled dialog — calls nothing further
    let added: number;
    try {
      added = await addWatchedFolder(selected);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      return;
    }
    // Set on the add's own success, before the re-read below — which can fail
    // on its own account (`loadError`) without un-adding the folder the note is
    // about. §9.2, Task 8 (owner's ruling): adding a folder starts no scan, so
    // this is what stands in its place.
    folderAdded = true;
    // Outside the action's own try: a rejection here is list_tree's own
    // sentence, about the list being unreadable, not about the add having
    // failed — the add already succeeded by this point.
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
      return;
    }
    // The folder just added is the one to set up: selected and open.
    const fresh = roots.find((r) => r.rootId === added);
    if (fresh !== undefined) selectRoot(fresh);
  }

  // 🔴 Task 9. The press asks; it removes nothing. The whole row is captured
  // here — the id, the path this row was DRAWN from, and the count it was
  // showing — and everything after this point reads that capture and never
  // `roots`: the backend deletes the row only while that id still names that
  // path, and a path re-derived from the id at confirm time would agree with it
  // by construction and check nothing.
  //
  // The three banners are cleared HERE, at the press, and deliberately not
  // again in `confirmRemove`: every path into that function runs through this
  // one first. The rule itself is `rootChanged`'s own — what the last press
  // led to stands until the next one.
  function askRemove(root: TreeRoot) {
    actionError = null;
    rootChanged = false;
    folderAdded = false; // the note is about the LAST press, and this one is a remove
    removeQuestion = { rootId: root.rootId, path: root.absolutePath, files: root.files.length };
  }

  function cancelRemove() {
    removeQuestion = null;
  }

  // What is sent is the QUESTION's own id and path, read from the capture and
  // from nothing on screen: a re-read under an open question can rename the row
  // it was asked about, and the sentence the person answered named one folder.
  async function confirmRemove() {
    const question = removeQuestion;
    if (question === null) return;
    // Off the screen before the wait, not after it: a confirmation still drawn
    // while its own removal is in flight is a second press waiting to happen.
    removeQuestion = null;
    removing = question.rootId;
    try {
      await removeWatchedFolder(question.rootId, question.path);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    } finally {
      removing = null;
    }
    // 🔴 Unconditional, and outside the `try` — `exclude`'s own shape, for the
    // same reason and for one more. The reason it shares: what the rows must say
    // next is decided by re-reading the state, never by parsing a rejection,
    // which crosses the IPC as a sentence and nothing else (§10). The reason of
    // its own: the commonest refusal here is `WatchedRootChanged` — the id no
    // longer names that path (`bridge.rs:97-180`) — and that is precisely the
    // state in which the row the person confirmed is not the row on screen. The
    // sentence says the removal did not happen; the re-read is what takes the
    // stale row away.
    //
    // A re-read that is itself refused is `loadError`'s, never `actionError`'s:
    // the removal's own answer already stands, and this failure is about the
    // list being unreadable.
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  const emptyLabel = $derived.by(() => { void $locale; return t('settings_folders_empty'); });
  const addLabel = $derived.by(() => { void $locale; return t('settings_folders_add'); });
  const loadFailedLabel = $derived.by(() => { void $locale; return t('settings_folders_load_failed'); });
  // §9.2, Task 8. Built the same way every other sentence on this screen is —
  // inside the `void $locale` rebuild — so a language switch while it is
  // showing moves it too.
  const addedNoteLabel = $derived.by(() => { void $locale; return t('settings_folders_added_note'); });
  const unconfirmedLabel = $derived.by(() => { void $locale; return t('settings_folders_unconfirmed'); });

  // Rows carry their own count label, not a bare `t()` call in the markup —
  // a call inside `{#each}` reads `get(locale)` non-reactively (`i18n/index.ts:11`)
  // and would not update on a language switch. The whole array is rebuilt
  // under `void $locale` instead, the shape `launcher/Tree.svelte`'s own
  // `historyRows` derived already uses for the same reason.
  //
  // `settings_folders_indexed`, not the shared `indexed_documents` (P2-4
  // review): a bare "0 documents" beside a folder path reads as a claim
  // about the FOLDER, and the ruling means every newly added folder shows zero
  // forever, not transiently — this key names the INDEX as the subject
  // ("Indexed: 0 documents") instead. `removeAriaLabel` carries the same
  // row's path so two "Remove" buttons in a two-folder list stay
  // distinguishable to a screen reader (P2-5); the visible button text is the
  // mockup's own trailing ✕ glyph (Task 6), never a word to keep in step.
  // 🔴 ONE classifier over `SubfolderState`, and no default arm. Every fact a
  // row shows about a state — its sentence, which control it offers, whether it
  // can be opened — is decided here and nowhere else, so no two of them can
  // disagree about the same state. The `never` binding is what makes a seventh
  // variant added to `tree.rs` and mirrored in `ipc.ts` fail `npm run check`
  // instead of rendering as an ordinary excludable folder.
  //
  // **Which states offer a tick** (folders tree PR 2). `open` (ticked) and
  // `excluded` (empty) carry their own rule or none; `excludedByAncestor`
  // carries an EMPTY tick whose press pushes the ancestor's rule down onto the
  // siblings (`set_tick`, one transaction). `masked`, `builtIn`, `symlink`,
  // `notRead` and `unusableName` offer none: a mask or the walk decides, or —
  // for `unusableName` — its contents ARE walked and its name is one no rule
  // can carry, which is why its sentence differs rather than resembles.
  // `builtIn`, `symlink` and `notRead` are not drawn at all.
  //
  // **What opens:** `open`, `excluded` and `excludedByAncestor` folders —
  // under a rule the tree stays readable at every depth so a file inside can
  // be reached and ticked. `symlink` must never open (`subfolder_state` asks
  // `is_symlink` about the entry itself, so a directory INSIDE a symlinked one
  // would come back `open`); `builtIn` and `unusableName` are shut because
  // both questions are asked about the WHOLE relative path, so every child
  // comes back in its parent's state. Files never open (`buildLevel` gates on
  // `isDir`).
  function describe(state: SubfolderState): {
    sentence: string;
    control: 'exclude' | 'include' | 'none';
    expandable: boolean;
    excluded: boolean;
  } {
    switch (state.kind) {
      case 'open':
        return { sentence: t('settings_subfolder_open'), control: 'exclude', expandable: true, excluded: false };
      case 'excluded':
        return { sentence: t('settings_subfolder_excluded'), control: 'include', expandable: true, excluded: true };
      // Folders tree PR 2: an empty tick that pushes the rule down, and it
      // opens, so a file deeper inside can be reached and ticked (the plan's
      // ruling 3: "no ticks" under a rule reads as empty ticks).
      case 'excludedByAncestor':
        return {
          // The prefix the STATE carries, not this row's own path: they differ,
          // and the one worth showing is the rule that has to go first.
          sentence: t('settings_subfolder_excluded_by_ancestor', { prefix: state.prefix }),
          control: 'include',
          expandable: true,
          excluded: true,
        };
      // A file under a mask: no tick, the mask decides (spec, rule 5). The
      // sentence is the tag beside the name.
      case 'masked':
        return { sentence: t('settings_entry_masked', { mask: state.mask }), control: 'none', expandable: false, excluded: false };
      // Never drawn: `buildLevel` drops both (spec, owner's ruling 4). The arms
      // stay so the switch is exhaustive and neither can be opened.
      case 'builtIn':
      case 'symlink':
      case 'notRead':
        return { sentence: '', control: 'none', expandable: false, excluded: false };
      case 'unusableName':
        return { sentence: t('settings_subfolder_unusable_name'), control: 'none', expandable: false, excluded: false };
      // Folders tree PR 3. Held by `.gitignore`: an empty tick that stores an
      // include, and it opens so a file deeper inside can be ticked alone.
      case 'gitIgnored':
        return { sentence: t('settings_subfolder_gitignored', { by: state.by }), control: 'include', expandable: true, excluded: true };
      // Held, but the walk goes in for an include below: on, and mixed
      // (`buildLevel`), so its click ticks everything inside.
      case 'gitIgnoredOpen':
        return { sentence: t('settings_subfolder_gitignored_open', { by: state.by }), control: 'exclude', expandable: true, excluded: false };
      case 'gitIncluded':
        return { sentence: t('settings_subfolder_git_included'), control: 'exclude', expandable: true, excluded: false };
      // 🔴 A compile-time arm with a run-time consequence, and the two are
      // answered in different places on purpose.
      //
      // Compile time: the `never` binding is what makes a seventh variant
      // mirrored into `ipc.ts` and left undescribed fail `npm run check`.
      //
      // Run time: a variant added to `tree.rs` and NOT mirrored arrives here
      // anyway — Rust and TypeScript share no compiler — and this arm then
      // returns the state object itself, so `control` is `undefined`. The
      // markup used to ask `row.control !== 'none'`, which `undefined`
      // satisfies, and the row grew a button with no text and no `aria-label`
      // whose click routed to `include`: an unlabelled control that removed
      // the person's exclusion rule, and under D29 sent that folder's text to
      // the provider on the next scan. The markup now names the two controls
      // it draws, so an undescribed state offers none.
      //
      // This arm deliberately does NOT also return `control: 'none'`. Two
      // guards for one fact is the shape this branch has paid for repeatedly —
      // each kills the other's mutant, and neither can be shown to work. The
      // run-time answer lives in the markup, alone, where one revert kills it.
      // The pin that stops the state arriving at all is
      // `ipc.test.ts`'s `SubfolderState is exactly what tree.rs defines`.
      default: {
        const unreachable: never = state;
        return unreachable;
      }
    }
  }

  // The `.gitignore` tag beside a name — only on the row `.gitignore` names
  // (or whose own include it is), not on every row it holds from above: the
  // spec's marks are about the row the rule is on.
  function gitTag(entry: Subfolder): string | null {
    switch (entry.state.kind) {
      case 'gitIgnored':
      case 'gitIgnoredOpen':
        return entry.state.by === entry.relativePath ? t('settings_entry_gitignore') : null;
      case 'gitIncluded':
        return entry.state.include === entry.relativePath ? t('settings_entry_gitignore_included') : null;
      default:
        return null;
    }
  }

  // Owner, 2026-11-12: roots under one long parent differ only at the end, so
  // the row cuts at the start (`dir="rtl"`) and the home folder reads `~`.
  // shortcut: home is guessed from `/Users/<name>` or `/home/<name>`; ask the
  // shell for it if a root ever lives in a home elsewhere.
  function shortPath(path: string): string {
    return path.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, '~');
  }

  type SubRow = {
    entry: Subfolder;
    sentence: string;
    control: 'exclude' | 'include' | 'none';
    controlAriaLabel: string | null;
    // The folder is on and a rule of the person's own holds something deeper:
    // the tick shows `–`. Read off the stored rules, not the expanded rows, so a
    // collapsed folder says it too.
    mixed: boolean;
    // A sentence drawn beside the name: only for the two states with no tick,
    // where it is the one thing that says why.
    tag: string | null;
    // The ⚠ beside the name: the file's own failure, or how many failed under
    // a COLLAPSED folder (open, its rows say it themselves). `title` carries
    // the reason the scan recorded.
    mark: { text: string; title: string | null } | null;
    expandable: boolean;
    excluded: boolean;
    expandAriaLabel: string;
    open: boolean;
    children: Level | null;
  };
  type Level = { unnameableLabel: string | null; rows: SubRow[] };

  // `rules` is threaded down the whole recursion rather than read from the
  // panel at each level, because it is one fact about one panel and a second
  // reading of it is a second answer that can disagree.
  function buildLevel(node: SubTree, rules: StoredExclusion[] | null): Level {
    // Built-in folders and symlinks are not drawn at all (spec, owner's ruling
    // 4): the walk never reads them, so a row would offer nothing to decide.
    const shown = node.listing.entries.filter(
      (entry) => entry.state.kind !== 'builtIn' && entry.state.kind !== 'symlink'
        && entry.state.kind !== 'notRead',
    );
    const rows = shown.map((entry) => {
      const { sentence, control, expandable, excluded } = describe(entry.state);
      const child = node.children[entry.relativePath];
      return {
        entry,
        sentence,
        control,
        controlAriaLabel:
          control === 'exclude' || control === 'include'
            ? t('settings_subfolder_index_named', { path: entry.relativePath })
            : null,
        mixed: control === 'exclude' && (
          heldBelow(rules, entry.relativePath)
          || entry.state.kind === 'gitIgnoredOpen'
          // What `.gitignore` names is as if the person had unticked it, so
          // its folder is `–`, as for a rule — opened or not.
          || entry.gitignoredBelow
        ),
        tag: control === 'none' ? sentence : gitTag(entry),
        mark:
          entry.failure !== null
            ? { text: t('settings_entry_failed'), title: entry.failure.reason }
            : entry.isDir && child === undefined && entry.failedBelow > 0
              ? { text: t('settings_entry_failed_below', { count: entry.failedBelow }), title: null }
              : null,
        expandable: expandable && entry.isDir,
        excluded,
        expandAriaLabel: t('settings_folders_expand_named', { path: entry.relativePath }),
        open: child !== undefined,
        children: child === undefined ? null : buildLevel(child, rules),
      };
    });
    return {
      unnameableLabel:
        node.listing.unnameable > 0
          ? t('settings_subfolders_unnameable', { count: node.listing.unnameable })
          : null,
      // Owner, 2026-11-12: nothing to show draws nothing — the open chevron
      // with no rows under it says so.
      rows,
    };
  }

  // 🔴 The question is built here, inside the `void $locale` rebuild, and not
  // at the moment of the click, for the reason written at the top of this
  // block: a `t()` call frozen into `panels` would keep its language through a
  // switch. The NUMBERS come from `Pending`; only the words come from here.
  type ConfirmView =
    | { kind: 'checking'; checkingLabel: string }
    | {
        kind: 'question';
        heading: string;
        cost: string;
        confirmLabel: string; confirmAriaLabel: string;
        cancelLabel: string; cancelAriaLabel: string;
      };

  function confirmView(pending: Pending): ConfirmView {
    if (pending.kind === 'checking') {
      // Two sentences, because the two presses are waiting for different things
      // — see `Pending.of`. Chosen here rather than frozen at the press, like
      // every other word on this screen: this function runs inside the
      // `void $locale` rebuild, so both follow a language switch.
      return {
        kind: 'checking',
        checkingLabel: t(
          pending.of === 'exclude'
            ? 'settings_folders_exclude_checking'
            : 'settings_folders_include_checking',
        ),
      };
    }
    const path = pending.path;
    if (pending.kind === 'all') {
      return {
        kind: 'question',
        // PR 3: with no rule of the person's to remove, "stop excluding" would
        // be false — the press is about what `.gitignore` names.
        heading: pending.rules === 0 && pending.gitignored > 0
          ? t('settings_folders_confirm_gitignored_heading', { path })
          : t('settings_folders_confirm_include_heading', { path }),
        cost: pending.gitignored === 0
          ? t('settings_folders_include_all_cost', { count: pending.rules })
          : pending.rules === 0
            ? t('settings_all_gitignored', { count: pending.gitignored })
            : `${t('settings_folders_include_all_cost', { count: pending.rules })} ${t('settings_all_gitignored', { count: pending.gitignored })}`,
        confirmLabel: t('settings_folders_confirm'),
        confirmAriaLabel: t('settings_folders_confirm_include_named', { path }),
        cancelLabel: t('settings_folders_confirm_cancel'),
        cancelAriaLabel: t('settings_folders_confirm_cancel_named', { path }),
      };
    }
    return {
      kind: 'question',
      heading:
        pending.kind === 'exclude'
          ? t('settings_folders_confirm_exclude_heading', { path })
          // PR 3: no rule of the person's holds a row `.gitignore` names.
          : pending.gitignored && !pending.pushDown
            ? t('settings_folders_confirm_gitignored_heading', { path })
            : t('settings_folders_confirm_include_heading', { path }),
      cost:
        pending.kind === 'exclude'
          ? t('settings_folders_exclude_cost', { paths: pending.paths })
          // First: a push-down row is held from above by definition, and what
          // its tick does is take that rule away — the sentence below would
          // say the opposite.
          : pending.pushDown
            ? t('settings_folders_include_cost_push_down', { path })
          // PR 3: no rule of the person's holds it — `.gitignore` does, and
          // the answer stores an include past it.
          : pending.gitignored
            ? t('settings_include_gitignored', { path })
          : pending.heldBelow
            ? t('settings_folders_include_cost_held_below')
            : t('settings_folders_include_cost'),
      confirmLabel: t('settings_folders_confirm'),
      confirmAriaLabel:
        pending.kind === 'exclude'
          ? t('settings_folders_confirm_exclude_named', { path })
          : t('settings_folders_confirm_include_named', { path }),
      cancelLabel: t('settings_folders_confirm_cancel'),
      cancelAriaLabel: t('settings_folders_confirm_cancel_named', { path }),
    };
  }

  const rows = $derived.by(() => {
    void $locale;
    return roots.map((root) => {
      const panel = panels[root.rootId];
      const mark = marks[root.rootId];
      const known = mark !== undefined && mark.path === root.absolutePath;
      return {
        root,
        selected: chosen !== null && chosen.rootId === root.rootId && chosen.path === root.absolutePath,
        // Unknown until the rules for this path have been read once: no mark
        // rather than a `✓` that could be false.
        rootMark: known ? (mark.some ? '–' : '✓') : null,
        rootMarkLabel: known ? t(mark.some ? 'settings_folders_root_some' : 'settings_folders_root_all') : null,
        countLabel: t('settings_folders_indexed', { count: root.files.length }),
        removeAriaLabel: t('settings_folders_remove_named', { path: root.absolutePath }),
        expandAriaLabel: t('settings_folders_expand_named', { path: root.absolutePath }),
        expanded: panel !== undefined,
        // 🔴 Task 9. Built HERE, inside the `void $locale` rebuild, exactly as
        // `confirmView` is and for the same reason — the words follow a
        // language switch. The PATH and the COUNT come from `removeQuestion`,
        // which froze both at the click: the sentence states what was on the
        // row the person pressed, and a re-read that renames or renumbers that
        // row underneath it changes neither half.
        removeConfirm:
          removeQuestion === null || removeQuestion.rootId !== root.rootId
            ? null
            : {
                question: t('settings_folders_confirm_remove', {
                  path: removeQuestion.path, files: removeQuestion.files,
                }),
                confirmLabel: t('settings_folders_confirm'),
                confirmAriaLabel:
                  t('settings_folders_confirm_remove_named', { path: removeQuestion.path }),
                cancelLabel: t('settings_folders_confirm_cancel'),
                cancelAriaLabel:
                  t('settings_folders_confirm_cancel_named', { path: removeQuestion.path }),
              },
        panel:
          panel === undefined
            ? null
            : {
                level: panel.tree === null ? null : buildLevel(panel.tree, panel.rules),
                loadingLabel:
                  panel.tree === null && panel.loadError === null
                    ? t('settings_subfolders_loading')
                    : null,
                loadError: panel.loadError,
                failedLabel: t('settings_subfolders_failed'),
                actionError: panel.actionError,
                confirm: panel.pending === null ? null : confirmView(panel.pending),
              },
      };
    });
  });

  // Read from `rows`, so it follows the same rebuild (and the same language) as
  // the list it is a row of; null when nothing is chosen or the choice is gone.
  const chosenRow = $derived(rows.find((row) => row.selected) ?? null);
  // Named by the path: with two folders waiting, two «Start» buttons must be
  // told apart by more than their place on screen. A function read in the
  // template, so `$locale` is tracked where it is called.
  // A collapsed root says how many files under it the scans could not read, so
  // a shut level does not hide a failure (spec E); open, its rows say it.
  const failedLabel = (root: TreeRoot, expanded: boolean) => {
    void $locale;
    return !expanded && root.failed > 0 ? t('settings_entry_failed_below', { count: root.failed }) : null;
  };
  const removeSelectedLabel = $derived.by(() => { void $locale; return t('settings_folders_remove_selected'); });
  const treeHintLabel = $derived.by(() => { void $locale; return t('settings_folders_tree_hint'); });

  // 🔴 PR 8a, fix round 5. Words derived from the boolean `rootChanged` holds,
  // under `void $locale` like every other sentence on this screen.
  const rootChangedLabel = $derived.by(() => {
    void $locale;
    return rootChanged ? t('settings_folders_folder_changed') : null;
  });
  // Task 9, all three under `void $locale` like every other sentence here.
  const removingLabel = $derived.by(() => { void $locale; return t('settings_folders_removing'); });
</script>

<!-- D202: the settings window is created hidden at start and never unmounted,
     so a folder added from the launcher reaches this list when the window is
     shown and focused, not through any job ending. The roots only: an expanded
     panel is re-walked when a scan ends, not on every focus. -->
<svelte:window onfocus={() => refresh().catch((e) => { loadError = e instanceof Error ? e.message : String(e); })} />

{#snippet subfolders(level: Level, rootId: number)}
  {#if level.unnameableLabel}<p class="tnote">{level.unnameableLabel}</p>{/if}
  <ul>
    {#each level.rows as row (row.entry.relativePath)}
      <li class="sub" class:excl={row.excluded} data-testid={`subfolder-${rootId}-${row.entry.relativePath}`}>
        <div class="trow" title={row.tag === null ? row.sentence : undefined}>
          {#if row.expandable}
            <button
              type="button"
              class="chevb"
              data-testid={`subfolder-expand-${rootId}-${row.entry.relativePath}`}
              aria-expanded={row.open}
              aria-label={row.expandAriaLabel}
              onclick={() => toggleSubfolder(rootId, row.entry.relativePath)}>{@render chevron()}</button>
          {:else}
            <span class="chevb leaf"></span>
          {/if}
          <!-- Named, never `!== 'none'` — see `describe`'s default arm. -->
          {#if row.control === 'exclude' || row.control === 'include'}
            <input
              type="checkbox"
              class="search-check"
              data-testid={`subfolder-check-${rootId}-${row.entry.relativePath}`}
              aria-label={row.controlAriaLabel}
              checked={row.control === 'exclude'}
              indeterminate={row.mixed}
              onchange={(e) => {
                // Back to what the data says: the click only asks. What the box
                // shows next is decided by the re-read after the answer.
                e.currentTarget.checked = row.control === 'exclude';
                e.currentTarget.indeterminate = row.mixed;
                // A mixed box ticks everything inside (spec, rule 4).
                if (row.control === 'exclude' && row.mixed) askAll(rootId, row.entry.relativePath);
                else if (row.control === 'exclude') askExclude(rootId, row.entry.relativePath, row.entry.isDir);
                else askInclude(
                  rootId, row.entry.relativePath,
                  row.entry.state.kind === 'excludedByAncestor',
                  row.entry.state.kind === 'gitIgnored',
                  row.entry.isDir,
                );
              }} />
          {:else}
            <span class="check-slot"></span>
          {/if}
          {#if row.entry.isDir}{@render folderIcon()}{:else}{@render fileIcon()}{/if}
          <span class="label">{row.entry.name}</span>
          <span class="tags">
            {#if row.tag}<span class="tag">{row.tag}</span>{/if}
            {#if row.mark}
              <span
                class="tag warn"
                data-testid={`subfolder-mark-${rootId}-${row.entry.relativePath}`}
                title={row.mark.title ?? undefined}>{row.mark.text}</span>
            {/if}
          </span>
        </div>
        {#if row.children}{@render subfolders(row.children, rootId)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet chevron()}
  <svg class="chev" viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4l4 4-4 4"/></svg>
{/snippet}

{#snippet plus()}
  <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M8 3v10M3 8h10"/></svg>
{/snippet}

{#snippet minus()}
  <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M3 8h10"/></svg>
{/snippet}

{#snippet fileIcon()}
  <svg class="ico" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true"><path d="M4 1.5h5l3.5 3.5v9a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5V2a.5.5 0 0 1 .5-.5z"/><path d="M9 1.5V5h3.5"/></svg>
{/snippet}

{#snippet folderIcon()}
  <svg class="ico" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true"><path d="M1.5 4.5a1 1 0 0 1 1-1h3.6l1.4 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"/></svg>
{/snippet}

<div class="folders">
  <!-- Above the list and not beside the Add control at the foot of it, where
       `actionError` sits: this sentence is about the list itself, which has
       just been re-read underneath the person, and the row they pressed on is
       one of the things that has gone from it. -->
  {#if rootChangedLabel}
    <p data-testid="folders-root-changed">{rootChangedLabel}</p>
  {/if}
  <div class="tbox">
    <!-- Fixed height with its own scroll (spec): what is inside never stretches
         the window. -->
    <div class="ttree" data-testid="folders-tree">
      {#if loadError}
        <p class="tnote">{loadFailedLabel}</p>
        <p class="tnote" data-testid="folders-load-reason">{loadError}</p>
      {:else if rows.length === 0}
        <p class="tnote">{emptyLabel}</p>
      {:else}
        <ul>
          {#each rows as { root, selected, rootMark, rootMarkLabel, countLabel, expandAriaLabel, expanded, panel } (root.rootId)}
            <li class="root" data-testid={`folder-row-${root.rootId}`}>
              <div class="trow" class:selected class:unconfirmed={!root.confirmed}>
                <button
                  type="button"
                  class="chevb"
                  data-testid={`folder-expand-${root.rootId}`}
                  aria-expanded={expanded}
                  aria-label={expandAriaLabel}
                  onclick={() => toggleRoot(root)}>{@render chevron()}</button>
                <!-- Not a control: the root's tick is the sum of what is below. -->
                <span
                  class="check-slot rmark"
                  data-testid={`folder-root-mark-${root.rootId}`}
                  role={rootMark === null ? undefined : 'img'}
                  aria-label={rootMarkLabel ?? undefined}>{rootMark ?? ''}</span>
                {@render folderIcon()}
                <button
                  type="button"
                  class="rsel"
                  data-testid={`folder-select-${root.rootId}`}
                  aria-pressed={selected}
                  title={root.absolutePath}
                  onclick={() => selectRoot(root)}><span class="fp" dir="rtl"><bdi dir="ltr">{shortPath(root.absolutePath)}</bdi></span></button>
                <span class="tags">
                  {#if removing === root.rootId}
                    <span class="tag" data-testid={`folder-removing-${root.rootId}`}>{removingLabel}</span>
                  {:else if root.confirmed}
                    <span class="tag">{countLabel}</span>
                  {:else}
                    <span class="tag" data-testid={`folder-unconfirmed-${root.rootId}`}>{unconfirmedLabel}</span>
                  {/if}
                  {#if failedLabel(root, expanded)}
                    <span class="tag warn" data-testid={`folder-failed-${root.rootId}`}>{failedLabel(root, expanded)}</span>
                  {/if}
                </span>
              </div>
              {#if panel}
                <div class="fsubs" data-testid={`folder-panel-${root.rootId}`}>
                  {#if panel.loadError}
                    <p class="tnote">{panel.failedLabel}</p>
                    <p class="tnote" data-testid={`folder-subfolders-reason-${root.rootId}`}>{panel.loadError}</p>
                  {/if}
                  {#if panel.loadingLabel}<p class="tnote">{panel.loadingLabel}</p>{/if}
                  {#if panel.level}{@render subfolders(panel.level, root.rootId)}{/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <!-- Between the tree and the bar, outside the scroll: a question about a row
         at the foot of a long tree must not be scrolled out of sight. Each is
         still tagged with the root it is about. -->
    <div class="tnotes">
      <!-- Only over a list that is on screen: a failed re-read takes the rows
           away, and a Start drawn from the last good listing confirms by id
           alone (`confirm_watched_folder`), which may name another folder by
           then. The questions go with it, as they did inside the rows. -->
      {#each loadError === null ? rows : [] as { root, panel, removeConfirm } (root.rootId)}
        <!-- A popup over the section, not a line in it (owner, 2026-11-12):
             a question about a row deep in a long tree must not read as part
             of the tree. Esc answers "Cancel". -->
        {#if removeConfirm}
          <div class="scrim">
            <div
              class="dialog"
              role="dialog"
              aria-modal="true"
              aria-label={removeConfirm.question}
              tabindex="-1"
              data-testid={`folder-remove-confirm-${root.rootId}`}
              use:dialogFocus={cancelRemove}>
              <p>{removeConfirm.question}</p>
              <div class="dialog-actions">
                <button
                  type="button"
                  aria-label={removeConfirm.confirmAriaLabel}
                  disabled={scanRunning}
                  onclick={confirmRemove}>{removeConfirm.confirmLabel}</button>
                <button
                  type="button"
                  use:focusCancel
                  aria-label={removeConfirm.cancelAriaLabel}
                  onclick={cancelRemove}>{removeConfirm.cancelLabel}</button>
              </div>
            </div>
          </div>
        {/if}
        {#if panel}
          {#if panel.actionError}
            <p data-testid={`folder-subfolder-error-${root.rootId}`}>{panel.actionError}</p>
          {/if}
          <!-- The question, and nothing stored until it is answered. The heading
               names the path, since it is not drawn inside the row it is about. -->
          {#if panel.confirm}
            {@const confirm = panel.confirm}
            <div class="scrim">
              <div
                class="dialog"
                role="dialog"
                aria-modal="true"
                aria-label={confirm.kind === 'checking' ? confirm.checkingLabel : confirm.heading}
                tabindex="-1"
                data-testid={`folder-confirm-${root.rootId}`}
                use:dialogFocus={() => dismiss(root.rootId)}>
                {#if confirm.kind === 'checking'}
                  <p>{confirm.checkingLabel}</p>
                {:else}
                  <p class="dialog-heading">{confirm.heading}</p>
                  <p data-testid={`folder-confirm-cost-${root.rootId}`}>{confirm.cost}</p>
                  <div class="dialog-actions">
                    <button
                      type="button"
                      aria-label={confirm.confirmAriaLabel}
                      onclick={() => answer(root.rootId)}>{confirm.confirmLabel}</button>
                    <button
                      type="button"
                      use:focusCancel
                      aria-label={confirm.cancelAriaLabel}
                      onclick={() => dismiss(root.rootId)}>{confirm.cancelLabel}</button>
                  </div>
                {/if}
              </div>
            </div>
          {/if}
        {/if}
      {/each}
      {#if folderAdded}
        <p data-testid="folders-added-note">{addedNoteLabel}</p>
      {/if}
      {#if actionError}<p data-testid="folders-action-error">{actionError}</p>{/if}
    </div>
    <div class="bar">
      <button
        type="button"
        data-testid="folders-add"
        aria-label={addLabel}
        title={addLabel}
        onclick={addFolder}>{@render plus()}</button>
      <span class="sep" aria-hidden="true"></span>
      <button
        type="button"
        data-testid="folders-remove"
        aria-label={chosenRow?.removeAriaLabel ?? removeSelectedLabel}
        title={chosenRow?.removeAriaLabel ?? removeSelectedLabel}
        disabled={chosenRow === null || removing !== null || scanRunning}
        onclick={() => { if (chosenRow !== null) askRemove(chosenRow.root); }}>{@render minus()}</button>
    </div>
  </div>
  <p class="hint">{treeHintLabel}</p>
</div>
