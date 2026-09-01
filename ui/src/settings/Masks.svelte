<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { listMasks, maskPreview, addMask, removeMask } from '../lib/ipc';

  // 🔴 The editor computes no counts of its own. `mask_preview` is the only
  // thing that may answer "how much goes", because it asks the walk's own
  // matcher (`tree.rs`); a number derived here would be a second
  // implementation of the rule and would disagree with the walk at exactly the
  // edges the mask layer was built to pin — the `.gitignore` parser edges, the
  // case folding, the normalisation form. There is no glob library in
  // `ui/package.json` and none is wanted.
  //
  // 🔴 And the preview is called on an explicit press, never per keystroke.
  // `mask_preview` holds the index mutex across a scan of every indexed path of
  // every root (`tree.rs`), so a call per character would contend with a
  // running walk. A debounce would only make the contention later; an explicit
  // press makes it once, and the person is already pressing something.
  type Pending =
    // Numbers, not a sentence, for `Folders.svelte`'s reason: a sentence frozen
    // at the press keeps its language through a switch. And read ONCE, from one
    // reply — re-deriving them per render would let the question a person is
    // answering renumber itself underneath them.
    | { kind: 'checking'; mask: string }
    | { kind: 'add'; mask: string; paths: number; documents: number }
    | { kind: 'remove'; mask: string };

  // `null` until the first read answers, and NOT `[]`: "no mask is stored" and
  // "nobody has said yet" are different claims, and the first one printed on a
  // screen that does not know it yet is a claim about the person's protection.
  let masks = $state<string[] | null>(null);
  let loadError = $state<string | null>(null);
  let draft = $state('');
  let pending = $state<Pending | null>(null);
  // 🔴 The mask AS TYPED, held beside the sentence rather than taken from the
  // sentence. `RulesError::InvalidMask`'s `reason` is `globset`'s own text and
  // it quotes the FOLDED pattern — someone who typed `[A-_]x.txt` reads about
  // `[a-_]x.txt`, which they never wrote. Nothing here may branch on an error
  // kind, so the fix is presentation: the frame names what they typed, the
  // sentence is shown verbatim under it, and a third line says why the two can
  // differ in case.
  //
  // `of` is carried because the same two lines serve all three failures, and
  // each is a different sentence: "was not added" is false of a removal that
  // failed, and a failure from `add_mask` itself is not what "the check
  // answered" — the check passed, or there would be no question to confirm.
  // `add-check`: `mask_preview` (inside `askAdd`) rejected — nothing was ever
  //   asked to store the mask.
  // `add-store`: the check passed, the question was answered, and `add_mask`
  //   (inside `answer`) rejected.
  // `remove`: `remove_mask` (inside `answer`) rejected.
  let refused = $state<{ mask: string; of: 'add-check' | 'add-store' | 'remove' } | null>(null);
  let actionError = $state<string | null>(null);
  let alreadyGone = $state(false);

  // Not `$state`: nothing renders from it. The list read is reachable from
  // three places — mount, an add and a remove — so two `list_masks` calls can
  // be on the wire at once, and the one asked for first is not the one that has
  // to answer first. The older answer would otherwise land behind the newer one
  // and draw a list that has been superseded.
  let reads = 0;

  // Not `$state`, same reason as `reads`: nothing renders from it. `askAdd` is
  // the only caller of `mask_preview`, but the reply can still land after the
  // question it was answering has stopped being the one on screen — a press on
  // Remove, a Cancel, or a second Add before the first reply arrives all move
  // `pending` on without waiting for it. Bumped by every one of those, so a
  // reply whose generation has gone stale is dropped rather than overwriting
  // whatever question replaced it.
  let previews = 0;

  function message(e: unknown) {
    return e instanceof Error ? e.message : String(e);
  }

  async function refresh() {
    const n = ++reads;
    try {
      const list = await listMasks();
      if (n !== reads) return;
      masks = list;
      loadError = null;
    } catch (e) {
      if (n !== reads) return;
      loadError = message(e);
    }
  }

  onMount(refresh);

  function forget() {
    refused = null;
    actionError = null;
    alreadyGone = false;
  }

  async function askAdd() {
    // The literal empty string is `validate_mask`'s one deliberate non-error —
    // it previews as two zeros and `add_mask` refuses it — so the blank row is
    // simply not pressable and neither answer is reached. NOT trimmed: `"   "`
    // is a refusal with a sentence of its own, and trimming here would hand a
    // person who typed spaces the wrong one of the two.
    const mask = draft;
    if (mask === '') return;
    forget();
    const n = ++previews;
    pending = { kind: 'checking', mask };
    try {
      const preview = await maskPreview(mask);
      if (n !== previews) return; // a newer question has replaced this one
      pending = { kind: 'add', mask, paths: preview.paths, documents: preview.documents };
    } catch (e) {
      if (n !== previews) return;
      pending = null;
      refused = { mask, of: 'add-check' };
      actionError = message(e);
    }
  }

  // No preview on this side, and it is not an omission: `mask_preview` counts
  // what a mask REMOVES, and what removing one releases is a different set —
  // every file the mask was holding back, including the ones no scan has ever
  // looked at. The cost is stated in words instead, because no count here would
  // be the count of that.
  function askRemove(mask: string) {
    ++previews; // a standing add-preview reply must not land on this question
    forget();
    pending = { kind: 'remove', mask };
  }

  function dismiss() {
    ++previews; // a standing add-preview reply must not resurrect the question just dismissed
    pending = null;
  }

  async function answer() {
    const p = pending;
    if (p === null || p.kind === 'checking') return;
    pending = null;
    try {
      if (p.kind === 'add') {
        await addMask(p.mask);
        draft = '';
      } else {
        // `remove_mask` answers whether a row actually went: a second window may
        // have removed the same mask first, and that is a different sentence.
        alreadyGone = !(await removeMask(p.mask));
      }
    } catch (e) {
      // The check already passed by the time this runs — this is `answer`, not
      // `askAdd` — so a failure here is the STORE's, never the check's.
      refused = { mask: p.mask, of: p.kind === 'add' ? 'add-store' : 'remove' };
      actionError = message(e);
    }
    // After a refusal too: what the person is looking at then is the index as
    // it is now, not the array from before the press.
    await refresh();
  }

  // `void $locale` on every derived sentence: a bare `t()` is evaluated once and
  // keeps its language through a switch.
  const heading = $derived.by(() => { void $locale; return t('settings_masks_heading'); });
  // The three facts none of which follows from the rest of the screen: a mask is
  // global (D-c), so it is not about the folder it is drawn beside; each folder
  // applies it on its OWN next scan, so one scan settles nothing; and letter
  // case does not matter — the mask and the file name are both case-folded and
  // normalised, so `*.PDF` and `*.pdf` are one rule stored as two rows. They are
  // not deduplicated behind the person's back; the sentence says they are the
  // same instead.
  const explainer = $derived.by(() => { void $locale; return t('settings_masks_explainer'); });
  const emptyLabel = $derived.by(() => { void $locale; return t('settings_masks_none'); });
  const addLabel = $derived.by(() => { void $locale; return t('settings_masks_add'); });
  const inputLabel = $derived.by(() => { void $locale; return t('settings_masks_input_label'); });
  const removeLabel = $derived.by(() => { void $locale; return t('settings_masks_remove'); });
  const loadFailedLabel = $derived.by(() => { void $locale; return t('settings_masks_load_failed'); });
  const alreadyGoneLabel = $derived.by(() => {
    void $locale;
    return alreadyGone ? t('settings_masks_already_gone') : null;
  });

  const rows = $derived.by(() => {
    void $locale;
    return (masks ?? []).map((mask) => ({
      mask,
      // Named by the mask itself, for `settings_folders_remove_named`'s reason:
      // every row's control carries the same word, and only the accessible name
      // tells two of them apart.
      removeAriaLabel: t('settings_masks_remove_named', { mask }),
    }));
  });

  const refusal = $derived.by(() => {
    void $locale;
    const r = refused;
    if (r === null || actionError === null) return null;
    // Branches on WHICH COMMAND WAS CALLED — state this component already
    // holds — never on the error's shape: nothing here may inspect what an
    // error looks like to decide what it means.
    const heading = t(
      r.of === 'add-check' ? 'settings_masks_refused_add'
        : r.of === 'add-store' ? 'settings_masks_refused_store'
        : 'settings_masks_refused_remove',
      { mask: r.mask },
    );
    return {
      heading,
      // The case note belongs to the check only: it explains a folded pattern
      // quoted inside a COMPILE refusal, and neither the add-store path (the
      // check already passed) nor the removal path folds anything.
      note: r.of === 'add-check' ? t('settings_masks_refused_case_note') : null,
    };
  });

  const question = $derived.by(() => {
    void $locale;
    const p = pending;
    if (p === null) return null;
    if (p.kind === 'checking') return { kind: 'checking' as const, label: t('settings_masks_checking') };
    return {
      kind: 'question' as const,
      heading: t(
        p.kind === 'add' ? 'settings_masks_confirm_add_heading' : 'settings_masks_confirm_remove_heading',
        { mask: p.mask },
      ),
      cost:
        p.kind === 'add'
          // 🔴 The zero has a sentence of its own. The shared one's `=0` arm for
          // documents says "each one is also indexed under another path", and
          // when no path matched at all there is nobody to say that about.
          // There is no shortcut past the question either: a preview of zero
          // means the INDEXED set holds nothing that matches today, and the next
          // scan can still take files that never finished indexing.
          ? p.paths === 0
            ? t('settings_masks_add_cost_none')
            : t('settings_masks_add_cost', { paths: p.paths, documents: p.documents })
          : t('settings_masks_remove_cost'),
      confirmLabel: t('settings_masks_confirm'),
      confirmAriaLabel: t(
        p.kind === 'add' ? 'settings_masks_confirm_add_named' : 'settings_masks_confirm_remove_named',
        { mask: p.mask },
      ),
      cancelLabel: t('settings_masks_confirm_cancel'),
      cancelAriaLabel: t('settings_masks_confirm_cancel_named', { mask: p.mask }),
    };
  });
</script>

<div class="masks">
  <h3>{heading}</h3>
  <p>{explainer}</p>
  {#if loadError}
    <p>{loadFailedLabel}</p>
    <p data-testid="masks-load-reason">{loadError}</p>
  {:else if masks !== null && rows.length === 0}
    <p>{emptyLabel}</p>
  {:else}
    <ul>
      {#each rows as { mask, removeAriaLabel } (mask)}
        <li data-testid={`mask-row-${mask}`}>
          <span>{mask}</span>
          <button type="button" aria-label={removeAriaLabel} onclick={() => askRemove(mask)}>{removeLabel}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <!-- The question, and nothing stored until it is answered. -->
  {#if question}
    {@const q = question}
    <div data-testid="mask-confirm">
      {#if q.kind === 'checking'}
        <p>{q.label}</p>
      {:else}
        <p>{q.heading}</p>
        <p data-testid="mask-confirm-cost">{q.cost}</p>
        <button type="button" aria-label={q.confirmAriaLabel} onclick={answer}>{q.confirmLabel}</button>
        <button type="button" aria-label={q.cancelAriaLabel} onclick={dismiss}>{q.cancelLabel}</button>
      {/if}
    </div>
  {/if}
  {#if alreadyGoneLabel}<p data-testid="mask-already-gone">{alreadyGoneLabel}</p>{/if}
  {#if refusal}
    <p data-testid="mask-refused-heading">{refusal.heading}</p>
    <!-- Verbatim, and it is the whole of the rejection: nothing here branches on
         an error kind, so this sentence is the only thing that can say which
         rule was refused and why. -->
    <p data-testid="mask-refused-reason">{actionError}</p>
    {#if refusal.note}<p>{refusal.note}</p>{/if}
  {/if}
  <label class="fl" for="mask-draft-input">{inputLabel}</label>
  <input id="mask-draft-input" type="text" bind:value={draft} />
  <!-- Also disabled while `checking`: closes the OTHER route to the same guard
       — nothing stopped a second Add before the first reply landed, queuing
       two `mask_preview` calls on the mutex with no order guarantee between
       them. The generation guard above is what makes either route safe; this
       is what keeps the second call from being placed at all. -->
  <button type="button" disabled={draft === '' || pending?.kind === 'checking'} onclick={askAdd}>{addLabel}</button>
</div>
