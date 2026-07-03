import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import Inspector from 'unplugin-vue-dev-locator/vite'

// Single structured source for the app release date (ISO-like, dash-separated).
// Consumed by `AboutPage.vue` as a fallback when the manifest lacks an entry
// for the currently running build. Keep this in sync with the version label
// rendered in `src/locales/messages.ts` (`sidebar.version`) on each release.
const APP_RELEASE_DATE = '2026-07-03'

// https://vite.dev/config/
export default defineConfig({
  build: {
    sourcemap: 'hidden',
  },
  define: {
    __APP_RELEASE_DATE__: JSON.stringify(APP_RELEASE_DATE),
  },
  plugins: [
    vue(),
    tailwindcss(),
    Inspector(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'), // ✅ 定义 @ = src
    },
  },
})
