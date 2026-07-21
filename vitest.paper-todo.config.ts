import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';
import { fileURLToPath } from 'node:url';

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  test: {
    environment: 'jsdom',
    include: [
      'src/lib/paperTodo.test.ts',
      'src/composables/usePaperTodo.test.ts',
      'src/pages/PaperTodoWindowPage.test.ts',
    ],
  },
});
