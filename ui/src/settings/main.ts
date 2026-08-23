import { mount } from 'svelte';
import Settings from './Settings.svelte';
import { bootLocale, locale, t } from '../i18n';

// The native OS window title is set from Rust; this keeps the HTML <title> (and anything else
// reading it, e.g. a future browser tab) tracking the same locale.
locale.subscribe(() => { document.title = 'Mnema — ' + t('settings_title'); });

// Non-fatal: if the locale round-trip to Rust fails, the window stays on the EN default
// the i18n module boots with rather than blocking Settings from mounting.
bootLocale().catch((err) => console.error('bootLocale failed', err));

export default mount(Settings, { target: document.getElementById('app')! });
