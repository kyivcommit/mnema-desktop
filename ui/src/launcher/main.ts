import { mount } from 'svelte';
import Launcher from './Launcher.svelte';
export default mount(Launcher, { target: document.getElementById('app')! });
