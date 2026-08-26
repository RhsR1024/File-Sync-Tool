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
const AboutPage = () => import('../pages/AboutPage.vue')
const ToolsHubPage = () => import('../pages/ToolsHubPage.vue')
const UmsInitialPasswordPage = () => import('../pages/UmsInitialPasswordPage.vue')
const PortalAutoLoginPage = () => import('../pages/PortalAutoLoginPage.vue')
const RemotePackagePatchPage = () => import('../pages/RemotePackagePatchPage.vue')
const CodeStatisticsPage = () => import('../pages/CodeStatisticsPage.vue')
const NetworkToolsPage = () => import('../pages/NetworkToolsPage.vue')
const DisplayControlPage = () => import('../pages/DisplayControlPage.vue')
const ScreenSharePage = () => import('../pages/ScreenSharePage.vue')
const VideoDeviceSimulatorPage = () => import('../pages/VideoDeviceSimulatorPage.vue')
const FileSharePage = () => import('../pages/FileSharePage.vue')
const TftpServerPage = () => import('../pages/TftpServerPage.vue')
const DiskCacheCleanupPage = () => import('../pages/DiskCacheCleanupPage.vue')
const ClipboardManagerPage = () => import('../pages/ClipboardManagerPage.vue')
const ErrorCodeLookupPage = () => import('../pages/ErrorCodeLookupPage.vue')
const NotepadExtensionsPage = () => import('../pages/NotepadExtensionsPage.vue')
const PaperTodoPage = () => import('../pages/PaperTodoPage.vue')

type RouteComponentLoader = () => Promise<unknown>

const routePreloaders: Readonly<Record<string, readonly RouteComponentLoader[]>> = {
  '/': [RuntimeLogsPage],
  '/sync': [SyncConsolePage, SyncOverviewPage],
  '/sync/tasks': [SyncConsolePage, SyncTasksPage],
  '/sync/delivery': [SyncConsolePage, SyncDeliveryPage],
  '/settings': [SettingsPage],
  '/about': [AboutPage],
  '/tools': [ToolsHubPage],
  '/tools/ums-initial-password': [UmsInitialPasswordPage],
  '/tools/appliance-ssh': [EnableApplianceSshPage],
  '/tools/portal-auto-login': [PortalAutoLoginPage],
  '/tools/remote-package-patch': [RemotePackagePatchPage],
  '/tools/code-statistics': [CodeStatisticsPage],
  '/tools/network': [NetworkToolsPage],
  '/tools/display-control': [DisplayControlPage],
  '/tools/screen-share': [ScreenSharePage],
  '/tools/video-device-simulator': [VideoDeviceSimulatorPage],
  '/tools/file-share': [FileSharePage],
  '/tools/tftp-server': [TftpServerPage],
  '/tools/disk-cache-cleanup': [DiskCacheCleanupPage],
  '/tools/clipboard': [ClipboardManagerPage],
  '/tools/error-code-lookup': [ErrorCodeLookupPage],
  '/tools/notepad-extensions': [NotepadExtensionsPage],
  '/tools/paper-todo': [PaperTodoPage],
}

const routeComponentPreloadCache = new WeakMap<RouteComponentLoader, Promise<unknown>>()

function preloadRouteComponent(loader: RouteComponentLoader): Promise<unknown> {
  const cached = routeComponentPreloadCache.get(loader)
  if (cached) return cached

  const pending = loader().catch((error) => {
    routeComponentPreloadCache.delete(loader)
    throw error
  })
  routeComponentPreloadCache.set(loader, pending)
  return pending
}

export async function preloadRoute(path: string): Promise<void> {
  const preloaders = routePreloaders[path]
  if (!preloaders) return
  await Promise.all(preloaders.map(preloadRouteComponent))
}

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
    component: AboutPage,
  },
  {
    path: '/tools',
    component: ToolsHubPage,
  },
  {
    path: '/tools/ums-initial-password',
    component: UmsInitialPasswordPage,
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
    component: PortalAutoLoginPage,
  },
  {
    path: '/tools/remote-package-patch',
    component: RemotePackagePatchPage,
  },
  {
    path: '/tools/code-statistics',
    component: CodeStatisticsPage,
  },
  {
    path: '/tools/network',
    component: NetworkToolsPage,
  },
  {
    path: '/tools/display-control',
    component: DisplayControlPage,
  },
  {
    path: '/tools/screen-share',
    component: ScreenSharePage,
  },
  {
    path: '/tools/video-device-simulator',
    component: VideoDeviceSimulatorPage,
  },
  {
    path: '/tools/file-share',
    component: FileSharePage,
  },
  {
    path: '/tools/tftp-server',
    component: TftpServerPage,
  },
  {
    path: '/tools/disk-cache-cleanup',
    component: DiskCacheCleanupPage,
  },
  {
    path: '/tools/clipboard',
    component: ClipboardManagerPage,
  },
  {
    path: '/tools/error-code-lookup',
    component: ErrorCodeLookupPage,
  },
  {
    path: '/tools/notepad-extensions',
    component: NotepadExtensionsPage,
  },
  {
    path: '/tools/paper-todo',
    component: PaperTodoPage,
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
