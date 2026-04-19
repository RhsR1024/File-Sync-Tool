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
    path: '/tools',
    component: () => import('../pages/ToolsHubPage.vue'),
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
  {
    path: '/tools/network',
    component: () => import('../pages/NetworkToolsPage.vue'),
  },
  {
    path: '/tools/screen-share',
    component: () => import('../pages/ScreenSharePage.vue'),
  },
  {
    path: '/tools/file-share',
    component: () => import('../pages/FileSharePage.vue'),
  },
  {
    path: '/tools/clipboard',
    component: () => import('../pages/ClipboardManagerPage.vue'),
  },
  {
    path: '/clipboard-panel',
    component: () => import('../pages/ClipboardPanelPage.vue'),
    meta: { noLayout: true },
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

export default router
