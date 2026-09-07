import '../styles/tokens.css';
import '../styles/fonts.css';
import '../styles/base.css';
import '../styles/launcher.css';
import { mount } from 'svelte';
import Launcher from './Launcher.svelte';
import { bootLocale } from '../i18n';
import { bootTheme } from '../theme';

document.title = 'Mnema';

// Non-fatal: if the locale round-trip to Rust fails, the window stays on the EN default
// the i18n module boots with rather than blocking the launcher from mounting.
bootLocale().catch((err) => console.error('bootLocale failed', err));
// Non-fatal for the same reason: a window that cannot ask follows the OS.
bootTheme().catch((err) => console.error('bootTheme failed', err));

export default mount(Launcher, { target: document.getElementById('app')! });
