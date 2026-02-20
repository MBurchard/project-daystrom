import {resolve} from 'node:path';
import {defineConfig} from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@app': resolve(__dirname, 'modules/app/src'),
    },
  },
  test: {
    include: ['modules/app/src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      include: ['modules/app/src/**/*.ts'],
      exclude: ['**/*.test.ts', '**/*.d.ts'],
    },
  },
});
