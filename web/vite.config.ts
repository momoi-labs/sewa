import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const controller = {
  target: process.env.SEWA_CONTROLLER_URL ?? 'http://127.0.0.1:8080',
  changeOrigin: false,
};

export default defineConfig({
  plugins: [react()],
  build: {
    assetsInlineLimit: 8 * 1024,
  },
  server: {
    proxy: {
      '/api': controller,
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
    css: false,
    globals: true,
    env: { TZ: 'UTC' },
  },
});
