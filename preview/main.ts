import { createApp, h } from 'vue';
import { createRouter, createWebHashHistory } from 'vue-router';

import '../src/style.css';
import { installMockBackend } from './mock-backend';

// The bridge has to exist before any app module runs, so the page and its shared
// simulator store stay behind dynamic imports below.
installMockBackend();

Promise.all([
  import('../src/i18n'),
  import('./PreviewShell.vue'),
]).then(([{ i18n }, shell]) => {
  const app = createApp(shell.default);
  // The page pushes to /tools/network from the "find available IPs" button, so a
  // router has to be present even though this harness renders a single page.
  app.use(createRouter({
    history: createWebHashHistory(),
    routes: [
      { path: '/', component: shell.default },
      // A render function, not a template string: the Vite build of Vue ships
      // without the runtime compiler.
      { path: '/tools/network', component: { render: () => h('p', { class: 'p-8 text-slate-600' }, '（预览中不含网络工具页）') } },
    ],
  }));
  app.use(i18n);
  app.mount('#preview');
});
