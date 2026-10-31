import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite-plus';

// `vp dev` proxies the API to a controller: a local `cargo run` by default, or e.g.
// AIRCON_API=https://aircon.rifat.homes to develop against the live one.
const api = process.env.AIRCON_API ?? 'http://localhost:8080';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  server: {
    proxy: {
      '/api': { target: api, changeOrigin: true, secure: false },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
