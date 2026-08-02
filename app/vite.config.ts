import {resolve} from 'node:path';
import process from 'node:process';
import vue from '@vitejs/plugin-vue';
import {defineConfig} from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  root: 'modules/app',
  plugins: [vue()],
  build: {
    sourcemap: 'inline',
  },
  resolve: {
    alias: {
      '@app': resolve(import.meta.dirname, 'modules/app/src'),
      '@generated': resolve(import.meta.dirname, 'modules/app/src/generated'),
      '@resources': resolve(import.meta.dirname, 'resources'),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ?
        {
          protocol: 'ws',
          host,
          port: 1421,
        } :
      undefined,
    watch: {
      ignored: ['**/modules/backend/**'],
    },
  },
});
