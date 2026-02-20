import {resolve} from 'node:path';
import process from 'node:process';
import vue from '@vitejs/plugin-vue';
import {defineConfig} from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  root: 'modules/app',
  plugins: [vue()],
  resolve: {
    alias: {
      '@app': resolve(__dirname, 'modules/app/src'),
      '@common': resolve(__dirname, 'modules/common/src'),
      '@resources': resolve(__dirname, 'resources'),
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
}));
