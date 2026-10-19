import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and no auto-clearing of the terminal
// (the Rust watcher prints next to it).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
  },
  build: {
    target: 'esnext',
    minify: 'oxc',
    sourcemap: true,
  },
});
