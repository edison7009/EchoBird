import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import pkg from './package.json';

export default defineConfig({
  base: './',
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_EDITION__: JSON.stringify(process.env.EDITION || 'full'),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // index.html is the only app entry. Without this, vite's dependency
  // pre-bundling scan crawls every .html in the repo — including the vendored
  // `references/**` folders (gitignored but present on disk) — and fails on
  // their broken imports whenever it re-optimizes (e.g. after a lockfile
  // change). Limiting the scan to the real entry keeps `vite dev` green.
  optimizeDeps: {
    entries: ['index.html'],
  },
  // The app loads its bundle from disk, not over a network, so the web download
  // warning does not apply. The main chunk and the 3D pet scene are about 630/500 kB.
  build: {
    chunkSizeWarningLimit: 700,
  },
  server: {
    port: 5173,
    open: false,
    watch: {
      ignored: ['**/src-tauri/target/**', '**/dist/**', '**/build/**'],
    },
  },
  // Tauri expects a fixed port and no browser auto-open
  clearScreen: false,
});
