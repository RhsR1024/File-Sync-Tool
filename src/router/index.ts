import { createRouter, createWebHistory } from 'vue-router'
import MainConsole from '@/pages/MainConsole.vue'
import TaskStatusPage from '@/pages/TaskStatusPage.vue'
import SettingsPage from '@/pages/SettingsPage.vue'
import HistoryPage from '@/pages/HistoryPage.vue'

const routes = [
  {
    path: '/',
    name: 'console',
    component: MainConsole,
  },
  {
    path: '/tasks',
    name: 'tasks',
    component: TaskStatusPage,
  },
  {
    path: '/history',
    name: 'history',
    component: HistoryPage,
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
