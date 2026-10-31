import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// Read by svelte-check and the Svelte language server.
export default {
  preprocess: vitePreprocess(),
};
