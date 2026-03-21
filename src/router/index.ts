import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import MainConsole from '@/pages/MainConsole.vue'
import TaskStatusPage from '@/pages/TaskStatusPage.vue'
import SettingsPage from '@/pages/SettingsPage.vue'
import HistoryPage from '@/pages/HistoryPage.vue'
import EnableApplianceSshPage from '@/pages/EnableApplianceSshPage.vue'

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
    redirect: '/tasks',
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
  {
    path: '/tools/framework-password',
    component: () => import('../pages/FrameworkPasswordPage.vue'),
  },
  {
    path: '/tools/appliance-ssh',
    component: EnableApplianceSshPage,
  },
  {
    path: '/tools/code-statistics',
    component: () => import('../pages/CodeStatisticsPage.vue'),
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

export default router
