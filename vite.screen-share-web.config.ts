import path from 'path';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  root: path.resolve(__dirname, 'src/screen-share-web'),
  base: '/',
  plugins: [vue()],
  resolve: {
    alias: {
      '@screen-share-web': path.resolve(__dirname, 'src/screen-share-web'),
    },
  },
  build: {
    outDir: path.resolve(__dirname, 'dist/screen-share-web'),
    emptyOutDir: true,
    sourcemap: false,
    assetsDir: 'assets',
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
  },
});
