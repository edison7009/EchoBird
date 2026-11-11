import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';
import { localizedRoutesPlugin } from './localized-routes-plugin.js';

export default defineConfig({
  root: path.resolve(__dirname),
  publicDir: path.resolve(__dirname, '../public'),
  plugins: [react(), localizedRoutesPlugin(__dirname)],
  resolve: { alias: { '/src': path.resolve(__dirname, '../src') } },
  base: '/',
  define: { __APP_EDITION__: JSON.stringify('full'), __APP_VERSION__: JSON.stringify('demo') },
  css: { postcss: path.resolve(__dirname, '..') },
  build: {
    outDir: '../build/website',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: path.resolve(__dirname, 'index.html'),
        demo: path.resolve(__dirname, 'demo.html'),
        usage: path.resolve(__dirname, 'usage.html'),
        features: path.resolve(__dirname, 'features.html'),
        docs: path.resolve(__dirname, 'docs.html'),
      },
    },
  },
  server: {
    host: '127.0.0.1',
    port: 8877,
    strictPort: true,
    fs: { allow: [path.resolve(__dirname, '..')] },
  },
});
