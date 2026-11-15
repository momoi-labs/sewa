import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// In development, Vite serves the app and forwards API calls to
// sewa-controller. In production, sewa-controller serves the built files.
const controller = {
  target: process.env.SEWA_CONTROLLER_URL ?? 'http://127.0.0.1:8080',
  changeOrigin: false,
};

export default defineConfig({
  plugins: [react()],
  build: {
    // The error page shows when the controller is down, so the assets it
    // draws, such as Kiso's hatch pattern, are inlined, not fetched.
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
    // Times show in the browser's zone; tests pin it.
    env: { TZ: 'UTC' },
  },
});
