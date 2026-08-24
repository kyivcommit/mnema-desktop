<script lang="ts">
  import { locale, t } from '../i18n';
  import { setSearchArms } from '../lib/ipc';

  let { textOn = $bindable(true), contentOn = $bindable(false), provider = false }: {
    textOn?: boolean;
    contentOn?: boolean;
    provider?: boolean; // a provider key is present — content needs it (§7.2)
  } = $props();

  const textLabel = $derived.by(() => { void $locale; return t('arm_text'); });
  const contentLabel = $derived.by(() => { void $locale; return t('arm_content'); });

  // An arm is "active" when on AND usable: text is always usable (the floor),
  // content only with a provider (§7.2). At least one active arm stays on — the
  // last one locks.
  const contentActive = $derived(contentOn && provider);
  const textLocked = $derived(textOn && !contentActive);
  const contentLocked = $derived(contentActive && !textOn);

  async function persist() {
    // content off on the wire without a provider, so the box and the arm that
    // actually runs (read_arms, bridge.rs:315) cannot disagree.
    await setSearchArms(textOn, contentOn && provider);
  }
  async function toggleText() {
    if (textLocked) return;
    textOn = !textOn;
    await persist();
  }
  async function toggleContent() {
    if (!provider || contentLocked) return;
    contentOn = !contentOn;
    await persist();
  }
</script>

<div class="arms">
  <label>
    <input type="checkbox" checked={textOn} disabled={textLocked} onchange={toggleText} />
    {textLabel}
  </label>
  <label class:unavailable={!provider}>
    <input type="checkbox" checked={contentActive} disabled={!provider || contentLocked} onchange={toggleContent} />
    {contentLabel}
  </label>
</div>
