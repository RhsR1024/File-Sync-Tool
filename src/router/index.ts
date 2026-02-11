import { createRouter, createWebHistory } from 'vue-router'
import MainConsole from '@/pages/MainConsole.vue'
import SettingsPage from '@/pages/SettingsPage.vue'

const routes = [
  {
    path: '/',
    name: 'console',
    component: MainConsole,
  },
  {
    path: '/settings',
    name: 'settings',
    component: SettingsPage,
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

export default router
