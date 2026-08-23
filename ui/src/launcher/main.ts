import { mount } from 'svelte';
import Launcher from './Launcher.svelte';
import { bootLocale } from '../i18n';

document.title = 'Mnema';

// Non-fatal: if the locale round-trip to Rust fails, the window stays on the EN default
// the i18n module boots with rather than blocking the launcher from mounting.
bootLocale().catch((err) => console.error('bootLocale failed', err));

export default mount(Launcher, { target: document.getElementById('app')! });
