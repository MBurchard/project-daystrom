import {resolve} from 'node:path';
import vue from '@vitejs/plugin-vue';
import {defineConfig} from 'vitest/config';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@app': resolve(import.meta.dirname, 'modules/app/src'),
      '@generated': resolve(import.meta.dirname, 'modules/app/src/generated'),
    },
  },
  test: {
    coverage: {
      provider: 'v8',
      include: ['modules/app/src/**/*.{ts,vue}'],
      exclude: [
        '**/*.spec.ts',
        '**/*.d.ts',
        '**/__tests__/**',
        'modules/app/src/generated/**',
        'modules/app/src/main.ts',
        'modules/app/src/types.ts',
      ],
      thresholds: {
        100: true,
      },
    },
    projects: [
      {
        extends: true,
        test: {
          name: 'node',
          include: ['**/__tests__/**/*.spec.ts'],
          exclude: ['**/__tests__/**/*.dom.spec.ts'],
          environment: 'node',
        },
      },
      {
        extends: true,
        test: {
          name: 'jsdom',
          include: ['**/__tests__/**/*.dom.spec.ts'],
          environment: 'jsdom',
        },
      },
    ],
  },
});
