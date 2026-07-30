import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'

// Every route loads on demand. The console pages only exist in the main window,
// and eager imports would put them in the shared entry chunk that each
// borderless helper window — clipboard panel, paper capsules, overlays — has to
// download and parse before it can paint. Keep-alive still matches these pages
// by their declared component names.
const SyncOverviewPage = () => import('@/pages/sync/SyncOverviewPage.vue')
const RuntimeLogsPage = () => import('@/pages/RuntimeLogsPage.vue')
const SettingsPage = () => import('@/pages/SettingsPage.vue')
const EnableApplianceSshPage = () => import('@/pages/EnableApplianceSshPage.vue')
const SyncConsolePage = () => import('@/pages/sync/SyncConsolePage.vue')
const SyncTasksPage = () => import('@/pages/sync/SyncTasksPage.vue')
const SyncDeliveryPage = () => import('@/pages/sync/SyncDeliveryPage.vue')

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
    path: '/tools/ums-initial-password',
    component: () => import('../pages/UmsInitialPasswordPage.vue'),
  },
  {
    path: '/tools/framework-password',
    redirect: '/tools/ums-initial-password',
  },
  {
    path: '/tools/appliance-ssh',
    component: EnableApplianceSshPage,
  },
  {
    path: '/tools/portal-auto-login',
    component: () => import('../pages/PortalAutoLoginPage.vue'),
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
    path: '/tools/tftp-server',
    component: () => import('../pages/TftpServerPage.vue'),
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
    path: '/tools/paper-todo',
    component: () => import('../pages/PaperTodoPage.vue'),
  },
  {
    path: '/paper-todo/window/:id',
    component: () => import('../pages/PaperTodoWindowPage.vue'),
    meta: { noLayout: true },
  },
  {
    path: '/paper-todo/launcher',
    component: () => import('../pages/PaperTodoLauncherPage.vue'),
    meta: { noLayout: true },
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
  {
    path: '/screen-share-overlay',
    component: () => import('../pages/ScreenShareOverlayPage.vue'),
    meta: { noLayout: true },
  },
  {
    path: '/screen-share-annotation-bar',
    component: () => import('../pages/ScreenShareAnnotationBarPage.vue'),
    meta: { noLayout: true },
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
