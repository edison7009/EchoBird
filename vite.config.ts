import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import pkg from './package.json';

export default defineConfig({
  base: './',
  plugins: [
    react(),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_EDITION__: JSON.stringify(process.env.EDITION || 'full'),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
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
