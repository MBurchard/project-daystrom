import {resolve} from 'node:path';
import {defineConfig} from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@app': resolve(__dirname, 'modules/app/src'),
      '@generated': resolve(__dirname, 'modules/app/src/generated'),
    },
  },
  test: {
    include: ['**/__tests__/**/*.spec.ts'],
    coverage: {
      provider: 'v8',
      include: ['modules/app/src/**/*.ts'],
      exclude: ['**/*.spec.ts', '**/*.d.ts'],
    },
  },
});
