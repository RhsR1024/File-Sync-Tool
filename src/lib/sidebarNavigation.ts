export type SidebarIconKey =
  | 'tasks'
  | 'console'
  | 'history'
  | 'settings'
  | 'toolsOverview'
  | 'frameworkPassword'
  | 'applianceSsh'
  | 'codeStatistics'
  | 'networkTools'
  | 'screenShare'
  | 'fileShare'
  | 'clipboardManager';

export type SidebarMatchMode = 'exact' | 'prefix';
export type SidebarRuntimeKey = 'screenShare' | 'fileShare';

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
        key: 'tasks',
        labelKey: 'sidebar.tasks',
        path: '/tasks',
        iconKey: 'tasks',
        matchMode: 'prefix',
      },
      {
        key: 'console',
        labelKey: 'sidebar.console',
        path: '/',
        iconKey: 'console',
        matchMode: 'exact',
      },
      {
        key: 'history',
        labelKey: 'sidebar.history',
        path: '/history',
        iconKey: 'history',
        matchMode: 'prefix',
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
        key: 'framework-password',
        labelKey: 'sidebar.frameworkPassword',
        path: '/tools/framework-password',
        iconKey: 'frameworkPassword',
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
        key: 'screen-share',
        labelKey: 'sidebar.screenShare',
        path: '/tools/screen-share',
        iconKey: 'screenShare',
        matchMode: 'prefix',
        runtimeKey: 'screenShare',
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
        key: 'clipboard-manager',
        labelKey: 'sidebar.clipboardManager',
        path: '/tools/clipboard',
        iconKey: 'clipboardManager',
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
  if (item.matchMode === 'exact' || item.path === '/') {
    return currentPath === item.path;
  }

  return currentPath === item.path || currentPath.startsWith(`${item.path}/`);
}
