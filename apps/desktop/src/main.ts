import { mount } from 'svelte';
import '@xterm/xterm/css/xterm.css';
import './app.css';
import App from './App.svelte';

const app = mount(App, {
  target: document.getElementById('app') as HTMLElement,
});

export default app;
