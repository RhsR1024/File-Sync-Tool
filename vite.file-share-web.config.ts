import path from 'path';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  root: path.resolve(__dirname, 'src/share-web'),
  base: '/',
  plugins: [vue()],
  resolve: {
    alias: {
      '@share-web': path.resolve(__dirname, 'src/share-web'),
    },
  },
  build: {
    outDir: path.resolve(__dirname, 'dist/file-share-web'),
    emptyOutDir: true,
    sourcemap: false,
  },
});
