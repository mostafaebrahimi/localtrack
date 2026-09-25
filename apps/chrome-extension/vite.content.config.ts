import { resolve } from 'node:path';

import { defineConfig } from 'vite';

// Content scripts cannot use ES module imports, so this one is bundled into a
// single IIFE file.
export default defineConfig({
  publicDir: false,
  build: {
    outDir: 'dist',
    emptyOutDir: false,
    target: 'es2021',
    sourcemap: false,
    lib: {
      entry: resolve(__dirname, 'src/content/index.ts'),
      formats: ['iife'],
      name: 'LocalTrackContent',
      fileName: () => 'content.js',
    },
  },
});
