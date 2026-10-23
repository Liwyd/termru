import { mount } from 'svelte';
// Self-hosted fonts (SIL OFL): the CSP is `font-src 'self'`, so no font may
// come from a remote URL. Vite copies them into the bundle. The mono face
// ships the Latin subsets only (the interface is English, Doc A).
import '@fontsource-variable/inter';
import '@fontsource/ibm-plex-mono/latin-400.css';
import '@fontsource/ibm-plex-mono/latin-500.css';
import '@fontsource/ibm-plex-mono/latin-ext-400.css';
import '@fontsource/ibm-plex-mono/latin-ext-500.css';
import '@xterm/xterm/css/xterm.css';
import './app.css';
import App from './App.svelte';

const app = mount(App, {
  target: document.getElementById('app') as HTMLElement,
});

export default app;
