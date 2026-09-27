import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  server: { port: 5190 },
  build: { target: 'es2022', chunkSizeWarningLimit: 1200 },
  worker: { format: 'es' },
});
