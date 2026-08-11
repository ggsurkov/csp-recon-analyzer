import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // Runes everywhere. No legacy reactive statements in this app.
    runes: true
  }
};
