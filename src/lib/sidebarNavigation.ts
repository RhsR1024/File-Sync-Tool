export type SidebarIconKey =
  | 'sync'
  | 'console'
  | 'settings'
  | 'toolsOverview'
  | 'umsInitialPassword'
  | 'applianceSsh'
  | 'portalAutoLogin'
  | 'remotePackagePatch'
  | 'codeStatistics'
  | 'networkTools'
  | 'displayControl'
  | 'screenShare'
  | 'videoDeviceSimulator'
  | 'fileShare'
  | 'tftpServer'
  | 'diskCacheCleanup'
  | 'clipboardManager'
  | 'errorCodeLookup'
  | 'notepadExtensions'
  | 'paperTodo';

export type SidebarMatchMode = 'exact' | 'prefix';
export type SidebarRuntimeKey = 'screenShare' | 'fileShare' | 'deviceSimulator' | 'tftpServer';

export interface SidebarNavItem {
  key: string;
  labelKey: string;
  path: string;
  iconKey: SidebarIconKey;
  matchMode?: SidebarMatchMode;
  runtimeKey?: SidebarRuntimeKey;
}

export interface SidebarNavSection {
  key: string;
  labelKey: string;
  items: readonly SidebarNavItem[];
}

export const SIDEBAR_NAV_SECTIONS = [
  {
    key: 'common',
    labelKey: 'sidebar.commonGroup',
    items: [
      {
        key: 'sync',
        labelKey: 'sidebar.syncConsole',
        path: '/sync',
        iconKey: 'sync',
        matchMode: 'prefix',
      },
      {
        key: 'console',
        labelKey: 'sidebar.console',
        path: '/',
        iconKey: 'console',
        matchMode: 'exact',
      },
    ],
  },
  {
    key: 'tools',
    labelKey: 'sidebar.tools',
    items: [
      {
        key: 'tools-overview',
        labelKey: 'sidebar.toolsOverview',
        path: '/tools',
        iconKey: 'toolsOverview',
        matchMode: 'exact',
      },
      {
        key: 'appliance-ssh',
        labelKey: 'sidebar.applianceSsh',
        path: '/tools/appliance-ssh',
        iconKey: 'applianceSsh',
        matchMode: 'prefix',
      },
      {
        key: 'remote-package-patch',
        labelKey: 'sidebar.remotePackagePatch',
        path: '/tools/remote-package-patch',
        iconKey: 'remotePackagePatch',
        matchMode: 'prefix',
      },
      {
        key: 'ums-initial-password',
        labelKey: 'sidebar.umsInitialPassword',
        path: '/tools/ums-initial-password',
        iconKey: 'umsInitialPassword',
        matchMode: 'prefix',
      },
      {
        key: 'code-statistics',
        labelKey: 'sidebar.codeStatistics',
        path: '/tools/code-statistics',
        iconKey: 'codeStatistics',
        matchMode: 'prefix',
      },
      {
        key: 'network-tools',
        labelKey: 'sidebar.networkTools',
        path: '/tools/network',
        iconKey: 'networkTools',
        matchMode: 'prefix',
      },
      {
        key: 'portal-auto-login',
        labelKey: 'sidebar.portalAutoLogin',
        path: '/tools/portal-auto-login',
        iconKey: 'portalAutoLogin',
        matchMode: 'prefix',
      },
      {
        key: 'display-control',
        labelKey: 'sidebar.displayControl',
        path: '/tools/display-control',
        iconKey: 'displayControl',
        matchMode: 'prefix',
      },
      {
        key: 'screen-share',
        labelKey: 'sidebar.screenShare',
        path: '/tools/screen-share',
        iconKey: 'screenShare',
        matchMode: 'prefix',
        runtimeKey: 'screenShare',
      },
      {
        key: 'video-device-simulator',
        labelKey: 'sidebar.videoDeviceSimulator',
        path: '/tools/video-device-simulator',
        iconKey: 'videoDeviceSimulator',
        matchMode: 'prefix',
        runtimeKey: 'deviceSimulator',
      },
      {
        key: 'file-share',
        labelKey: 'sidebar.fileShare',
        path: '/tools/file-share',
        iconKey: 'fileShare',
        matchMode: 'prefix',
        runtimeKey: 'fileShare',
      },
      {
        key: 'tftp-server',
        labelKey: 'sidebar.tftpServer',
        path: '/tools/tftp-server',
        iconKey: 'tftpServer',
        matchMode: 'prefix',
        runtimeKey: 'tftpServer',
      },
      {
        key: 'disk-cache-cleanup',
        labelKey: 'sidebar.diskCacheCleanup',
        path: '/tools/disk-cache-cleanup',
        iconKey: 'diskCacheCleanup',
        matchMode: 'prefix',
      },
      {
        key: 'clipboard-manager',
        labelKey: 'sidebar.clipboardManager',
        path: '/tools/clipboard',
        iconKey: 'clipboardManager',
        matchMode: 'prefix',
      },
      {
        key: 'error-code-lookup',
        labelKey: 'sidebar.errorCodeLookup',
        path: '/tools/error-code-lookup',
        iconKey: 'errorCodeLookup',
        matchMode: 'prefix',
      },
      {
        key: 'notepad-extensions',
        labelKey: 'sidebar.notepadExtensions',
        path: '/tools/notepad-extensions',
        iconKey: 'notepadExtensions',
        matchMode: 'prefix',
      },
      {
        key: 'paper-todo',
        labelKey: 'sidebar.paperTodo',
        path: '/tools/paper-todo',
        iconKey: 'paperTodo',
        matchMode: 'prefix',
      },
    ],
  },
  {
    key: 'system',
    labelKey: 'sidebar.systemGroup',
    items: [
      {
        key: 'settings',
        labelKey: 'sidebar.settings',
        path: '/settings',
        iconKey: 'settings',
        matchMode: 'prefix',
      },
    ],
  },
] as const satisfies readonly SidebarNavSection[];

export function isSidebarItemActive(
  currentPath: string,
  item: Pick<SidebarNavItem, 'path' | 'matchMode'>,
) {
  if (item.matchMode === 'exact') {
    return currentPath === item.path;
  }

  return currentPath === item.path || currentPath.startsWith(`${item.path}/`);
}
