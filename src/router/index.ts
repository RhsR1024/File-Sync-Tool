import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import MainConsole from '@/pages/MainConsole.vue'
import ManualCopyPage from '@/pages/ManualCopyPage.vue'
import TaskStatusPage from '@/pages/TaskStatusPage.vue'
import SettingsPage from '@/pages/SettingsPage.vue'
import HistoryPage from '@/pages/HistoryPage.vue'

const routes: RouteRecordRaw[] = [
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
    path: '/manual-copy',
    name: 'manual-copy',
    component: ManualCopyPage,
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
