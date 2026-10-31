import { defineConfig } from 'vite-plus';

export default defineConfig({
  fmt: {
    singleQuote: true,
    // Generated files stay exactly as `cargo test` writes them, so drift checks are a
    // plain diff.
    ignorePatterns: ['target/**', 'data/**', 'packages/api/src/generated/**'],
  },
  lint: {
    jsPlugins: [{ name: 'vite-plus', specifier: 'vite-plus/oxlint-plugin' }],
    rules: { 'vite-plus/prefer-vite-plus-imports': 'error' },
    options: { typeAware: true, typeCheck: false },
    ignorePatterns: ['target/**', 'data/**'],
  },
});
