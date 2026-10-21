import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte({ hot: false })],
  // Component tests mount real Svelte components: resolve the browser build.
  resolve: {
    conditions: ['browser'],
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
    server: {
      deps: {
        // The Tauri plugins are externalized by default, and a mock of the
        // IPC layer does not reach an externalized module. Inlining them
        // makes the contract test see the real plugin call.
        inline: ['@tauri-apps/plugin-dialog'],
      },
    },
  },
});
