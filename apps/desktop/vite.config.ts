import { resolve } from 'node:path';

import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Everything is bundled locally: no CDN, no remote fonts, no remote scripts
// (spec §130).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: 'es2021',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        // The dashboard and the floating timer are separate pages so the timer
        // never loads React, the router, the query cache or the chart library.
        index: resolve(__dirname, 'index.html'),
        timer: resolve(__dirname, 'timer.html'),
      },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
});
