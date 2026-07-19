import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'
import SyncOverviewPage from '@/pages/sync/SyncOverviewPage.vue'
import RuntimeLogsPage from '@/pages/RuntimeLogsPage.vue'
import SettingsPage from '@/pages/SettingsPage.vue'
import EnableApplianceSshPage from '@/pages/EnableApplianceSshPage.vue'
import SyncConsolePage from '@/pages/sync/SyncConsolePage.vue'
import SyncTasksPage from '@/pages/sync/SyncTasksPage.vue'
import SyncDeliveryPage from '@/pages/sync/SyncDeliveryPage.vue'

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'runtime-logs',
    component: RuntimeLogsPage,
  },
  {
    path: '/tasks',
    redirect: '/sync',
  },
  {
    path: '/manual-copy',
    redirect: '/sync',
  },
  {
    path: '/sync/logs',
    redirect: '/',
  },
  {
    path: '/sync',
    component: SyncConsolePage,
    children: [
      {
        path: '',
        name: 'sync-overview',
        component: SyncOverviewPage,
      },
      {
        path: 'tasks',
        name: 'sync-tasks',
        component: SyncTasksPage,
      },
      {
        path: 'strategy',
        name: 'sync-strategy',
        redirect: '/sync/tasks',
      },
      {
        path: 'delivery',
        name: 'sync-delivery',
        component: SyncDeliveryPage,
      },
    ],
  },
  {
    path: '/settings',
    name: 'settings',
    component: SettingsPage,
  },
  {
    path: '/about',
    name: 'about',
    component: () => import('../pages/AboutPage.vue'),
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
    path: '/tools/remote-package-patch',
    component: () => import('../pages/RemotePackagePatchPage.vue'),
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
    path: '/tools/display-control',
    component: () => import('../pages/DisplayControlPage.vue'),
  },
  {
    path: '/tools/screen-share',
    component: () => import('../pages/ScreenSharePage.vue'),
  },
  {
    path: '/tools/video-device-simulator',
    component: () => import('../pages/VideoDeviceSimulatorPage.vue'),
  },
  {
    path: '/tools/file-share',
    component: () => import('../pages/FileSharePage.vue'),
  },
  {
    path: '/tools/disk-cache-cleanup',
    component: () => import('../pages/DiskCacheCleanupPage.vue'),
  },
  {
    path: '/tools/clipboard',
    component: () => import('../pages/ClipboardManagerPage.vue'),
  },
  {
    path: '/tools/error-code-lookup',
    component: () => import('../pages/ErrorCodeLookupPage.vue'),
  },
  {
    path: '/tools/notepad-extensions',
    component: () => import('../pages/NotepadExtensionsPage.vue'),
  },
  {
    path: '/clipboard-panel',
    component: () => import('../pages/ClipboardPanelPage.vue'),
    meta: { noLayout: true },
  },
  {
    path: '/clipboard-preview/image',
    component: () => import('../pages/ClipboardImagePreview.vue'),
    meta: { noLayout: true },
  },
  {
    path: '/clipboard-preview/text',
    component: () => import('../pages/ClipboardTextPreview.vue'),
    meta: { noLayout: true },
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
