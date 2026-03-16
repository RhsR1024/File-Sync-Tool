<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  BarChart3,
  Download,
  FileCode,
  FolderOpen,
  FolderPlus,
  GitCompareArrows,
  Loader,
  Trash2,
} from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import CodeStatisticsScopeTreeNode from '@/components/CodeStatisticsScopeTreeNode.vue';
import {
  codeCountAnalyze,
  codeCountListScopeTree,
  openDirectory,
  openPathParent,
  saveTextFile,
  type CodeCountProgress,
  type CodeCountResult,
  type CodeCountScopeTreeNode as ScopeTreeNode,
} from '../lib/tauri';

defineOptions({ name: 'CodeStatisticsPage' });

const { t } = useI18n();

type AnalysisMode = 'incremental' | 'newProject';
type ExportFormat = 'csv' | 'html';
type ExportMessageType = 'success' | 'error' | 'info';
type ScopePanelKey = 'old' | 'new' | 'project';

interface ExportMessageState {
  type: ExportMessageType;
  text: string;
  path?: string;
}

interface ScopePanelState {
  tree: ScopeTreeNode[];
  selectedFilePaths: string[];
  expandedKeys: string[];
  error: string;
  isLoading: boolean;
}

interface ScopePanelView {
  key: ScopePanelKey;
  title: string;
  path: string;
  state: ScopePanelState;
}

const createScopePanelState = (): ScopePanelState => ({
  tree: [],
  selectedFilePaths: [],
  expandedKeys: [],
  error: '',
  isLoading: false,
});

const mode = ref<AnalysisMode>('incremental');
const oldPath = ref('');
const newPath = ref('');
const isAnalyzing = ref(false);
const isExporting = ref<ExportFormat | null>(null);
const progress = ref<CodeCountProgress | null>(null);
const result = ref<CodeCountResult | null>(null);
const errorMsg = ref('');
const exportMessage = ref<ExportMessageState | null>(null);
const includeExtensionsInput = ref('');
const excludeExtensionsInput = ref('');
const oldScopeState = reactive(createScopePanelState());
const newScopeState = reactive(createScopePanelState());
const projectScopeState = reactive(createScopePanelState());

let unlistenProgress: UnlistenFn | null = null;

const shouldShowScopeSelector = computed(() =>
  mode.value === 'incremental'
    ? Boolean(oldPath.value.trim() && newPath.value.trim())
    : Boolean(newPath.value.trim()),
);

const normalizeExtensionToken = (value: string) => {
  const trimmed = value.trim().replace(/^\*+/, '').replace(/^\./, '');
  if (!trimmed) return '';
  return `.${trimmed.toLowerCase()}`;
};

const parseExtensionInput = (value: string) =>
  Array.from(
    new Set(
      value
        .split(/[\s,，;；]+/)
        .map(normalizeExtensionToken)
        .filter(Boolean),
    ),
  );

const includedExtensions = computed(() => parseExtensionInput(includeExtensionsInput.value));
const excludedExtensions = computed(() => parseExtensionInput(excludeExtensionsInput.value));

const extensionFilterSummary = computed(() => {
  if (includedExtensions.value.length === 0 && excludedExtensions.value.length === 0) {
    return t('codeStatistics.extensionFilterAll');
  }

  const parts: string[] = [];

  if (includedExtensions.value.length > 0) {
    parts.push(
      t('codeStatistics.includeExtensionsSummary', {
        value: includedExtensions.value.join(', '),
      }),
    );
  }

  if (excludedExtensions.value.length > 0) {
    parts.push(
      t('codeStatistics.excludeExtensionsSummary', {
        value: excludedExtensions.value.join(', '),
      }),
    );
  }

  return parts.join(' | ');
});

const collectLeafKeysFromNode = (node: ScopeTreeNode): string[] => {
  if (node.kind === 'file') {
    return [node.key];
  }

  return node.children.flatMap(collectLeafKeysFromNode);
};

const collectLeafKeysFromTree = (nodes: ScopeTreeNode[]) =>
  nodes.flatMap(collectLeafKeysFromNode);

const collectDirectoryKeysFromTree = (nodes: ScopeTreeNode[]): string[] => {
  const keys: string[] = [];

  for (const node of nodes) {
    if (node.kind !== 'directory') {
      continue;
    }

    keys.push(node.key);
    keys.push(...collectDirectoryKeysFromTree(node.children));
  }

  return keys;
};

const defaultExpandedKeysFromTree = (nodes: ScopeTreeNode[]) =>
  nodes.filter((node) => node.kind === 'directory').map((node) => node.key);

const getScopeTotalSelectableFiles = (state: ScopePanelState) =>
  collectLeafKeysFromTree(state.tree).length;

const getScopeSelectedFileCount = (state: ScopePanelState) => state.selectedFilePaths.length;

const getScopeSummaryText = (state: ScopePanelState) => {
  const total = getScopeTotalSelectableFiles(state);
  const selected = getScopeSelectedFileCount(state);
  return t('codeStatistics.currentScopeSummary', {
    selected,
    total,
  });
};

const scopePanels = computed<ScopePanelView[]>(() =>
  mode.value === 'incremental'
    ? [
        {
          key: 'old',
          title: t('codeStatistics.oldScopeTitle'),
          path: oldPath.value.trim(),
          state: oldScopeState,
        },
        {
          key: 'new',
          title: t('codeStatistics.newScopeTitle'),
          path: newPath.value.trim(),
          state: newScopeState,
        },
      ]
    : [
        {
          key: 'project',
          title: t('codeStatistics.projectScopeTitle'),
          path: newPath.value.trim(),
          state: projectScopeState,
        },
      ],
);

const isLoadingScopes = computed(() =>
  scopePanels.value.some((panel) => panel.state.isLoading),
);

const scopeSummaryText = computed(() => {
  if (mode.value === 'incremental') {
    return [oldScopeSummaryLine.value, newScopeSummaryLine.value].join(' | ');
  }

  return projectScopeSummaryLine.value;
});

const oldScopeSummaryText = computed(() => getScopeSummaryText(oldScopeState));
const newScopeSummaryText = computed(() => getScopeSummaryText(newScopeState));
const projectScopeSummaryText = computed(() => getScopeSummaryText(projectScopeState));
const oldScopeSummaryLine = computed(() =>
  t('codeStatistics.oldScopeSummaryLine', {
    selected: getScopeSelectedFileCount(oldScopeState),
    total: getScopeTotalSelectableFiles(oldScopeState),
  }),
);
const newScopeSummaryLine = computed(() =>
  t('codeStatistics.newScopeSummaryLine', {
    selected: getScopeSelectedFileCount(newScopeState),
    total: getScopeTotalSelectableFiles(newScopeState),
  }),
);
const projectScopeSummaryLine = computed(() =>
  t('codeStatistics.projectScopeSummaryLine', {
    selected: getScopeSelectedFileCount(projectScopeState),
    total: getScopeTotalSelectableFiles(projectScopeState),
  }),
);
const resultScopeSummaryLines = computed(() =>
  mode.value === 'incremental'
    ? [oldScopeSummaryLine.value, newScopeSummaryLine.value]
    : [projectScopeSummaryLine.value],
);

const getMissingScopePanelTitles = () =>
  scopePanels.value
    .filter((panel) => {
      const total = getScopeTotalSelectableFiles(panel.state);
      return total > 0 && panel.state.selectedFilePaths.length === 0;
    })
    .map((panel) => panel.title);

const netCode = computed(() => {
  if (!result.value) return 0;
  return result.value.summary.codeAdded - result.value.summary.codeDeleted;
});

const netComment = computed(() => {
  if (!result.value) return 0;
  return result.value.summary.commentAdded - result.value.summary.commentDeleted;
});

const totalChanged = computed(() => {
  if (!result.value) return 0;
  return result.value.operationSummary.changedTotal;
});

const fileTypeSummaryEntries = computed(() => {
  if (!result.value) return [];

  return Object.entries(result.value.fileTypeSummary)
    .map(([ext, summary]) => ({
      ext,
      total:
        summary.codeAdded +
        summary.codeDeleted +
        summary.codeModified +
        summary.commentAdded +
        summary.commentDeleted +
        summary.commentModified,
      ...summary,
    }))
    .sort((a, b) => b.total - a.total);
});

const maxFileTypeTotal = computed(() =>
  Math.max(...fileTypeSummaryEntries.value.map((entry) => entry.total), 1),
);

const fileTypeTotal = computed(() => {
  if (!fileTypeSummaryEntries.value.length) return null;

  return fileTypeSummaryEntries.value.reduce(
    (acc, entry) => ({
      codeAdded: acc.codeAdded + entry.codeAdded,
      codeDeleted: acc.codeDeleted + entry.codeDeleted,
      codeModified: acc.codeModified + entry.codeModified,
      commentAdded: acc.commentAdded + entry.commentAdded,
      commentDeleted: acc.commentDeleted + entry.commentDeleted,
      commentModified: acc.commentModified + entry.commentModified,
      total: acc.total + entry.total,
    }),
    {
      codeAdded: 0,
      codeDeleted: 0,
      codeModified: 0,
      commentAdded: 0,
      commentDeleted: 0,
      commentModified: 0,
      total: 0,
    },
  );
});

const barChartMaxValue = computed(() => {
  if (!result.value) return 1;

  const {
    codeAdded,
    codeDeleted,
    codeModified,
    commentAdded,
    commentDeleted,
    commentModified,
  } = result.value.summary;

  return Math.max(
    codeAdded,
    codeDeleted,
    codeModified,
    commentAdded,
    commentDeleted,
    commentModified,
    1,
  );
});

const pieStyle = computed(() => {
  if (!result.value) return { background: '#e2e8f0' };

  const { addedTotal, deletedTotal, modifiedTotal } = result.value.operationSummary;
  const total = addedTotal + deletedTotal + modifiedTotal;
  if (!total) return { background: '#e2e8f0' };

  const addedDeg = (addedTotal / total) * 360;
  const deletedDeg = addedDeg + (deletedTotal / total) * 360;

  return {
    background: `conic-gradient(#22c55e 0deg, #22c55e ${addedDeg}deg, #ef4444 ${addedDeg}deg, #ef4444 ${deletedDeg}deg, #f59e0b ${deletedDeg}deg, #f59e0b 360deg)`,
  };
});

const phaseLabel = computed(() => {
  if (!progress.value) return '';

  switch (progress.value.phase) {
    case 'scan':
      return t('codeStatistics.phaseScan');
    case 'diff':
      return t('codeStatistics.phaseDiff');
    case 'completed':
      return t('codeStatistics.phaseCompleted');
    default:
      return progress.value.phase;
  }
});

const exportMessageClasses = computed(() => {
  if (!exportMessage.value) return '';

  switch (exportMessage.value.type) {
    case 'success':
      return 'bg-emerald-50 border-emerald-200 text-emerald-800';
    case 'error':
      return 'bg-red-50 border-red-200 text-red-800';
    default:
      return 'bg-slate-50 border-slate-200 text-slate-700';
  }
});

const barHeight = (value: number) =>
  `${Math.round((value / barChartMaxValue.value) * 150)}px`;

const resetScopeState = (state: ScopePanelState) => {
  state.tree = [];
  state.selectedFilePaths = [];
  state.expandedKeys = [];
  state.error = '';
  state.isLoading = false;
};

const syncScopeState = (state: ScopePanelState, tree: ScopeTreeNode[]) => {
  const allLeafKeys = collectLeafKeysFromTree(tree);
  const allDirectoryKeys = collectDirectoryKeysFromTree(tree);
  const preserveEmptySelection =
    state.tree.length > 0 && state.selectedFilePaths.length === 0;
  const preserveCollapsedState =
    state.tree.length > 0 && state.expandedKeys.length === 0;
  const previousSelection = new Set(state.selectedFilePaths);
  const previousExpanded = new Set(state.expandedKeys);
  const retainedSelection = allLeafKeys.filter((key) => previousSelection.has(key));
  const retainedExpanded = allDirectoryKeys.filter((key) => previousExpanded.has(key));

  state.tree = tree;
  state.selectedFilePaths = preserveEmptySelection
    ? []
    : retainedSelection.length > 0
      ? retainedSelection
      : allLeafKeys;
  state.expandedKeys = preserveCollapsedState
    ? []
    : retainedExpanded.length > 0
      ? retainedExpanded
      : defaultExpandedKeysFromTree(tree);
};

const refreshScopePanel = async (state: ScopePanelState, path: string) => {
  const trimmedPath = path.trim();
  if (!trimmedPath) {
    resetScopeState(state);
    return;
  }

  state.isLoading = true;
  state.error = '';

  try {
    const tree = await codeCountListScopeTree(
      [trimmedPath],
      includedExtensions.value,
      excludedExtensions.value,
    );
    syncScopeState(state, tree);
  } catch (error) {
    state.tree = [];
    state.selectedFilePaths = [];
    state.expandedKeys = [];
    state.error = t('codeStatistics.scopeLoadFailed', {
      error: error instanceof Error ? error.message : String(error),
    });
  } finally {
    state.isLoading = false;
  }
};

const refreshScopeTree = async () => {
  if (mode.value === 'incremental') {
    if (!shouldShowScopeSelector.value) {
      resetScopeState(oldScopeState);
      resetScopeState(newScopeState);
      return;
    }

    await Promise.all([
      refreshScopePanel(oldScopeState, oldPath.value),
      refreshScopePanel(newScopeState, newPath.value),
    ]);
    return;
  }

  resetScopeState(oldScopeState);
  resetScopeState(newScopeState);

  if (!shouldShowScopeSelector.value) {
    resetScopeState(projectScopeState);
    return;
  }

  await refreshScopePanel(projectScopeState, newPath.value);
};

const handlePathBlur = async () => {
  await refreshScopeTree();
};

const handleFilterBlur = async () => {
  await refreshScopeTree();
};

const browseOld = async () => {
  try {
    const dir = await openDirectory();
    if (dir) {
      oldPath.value = dir;
      await refreshScopeTree();
    }
  } catch (error) {
    console.error(error);
  }
};

const browseNew = async () => {
  try {
    const dir = await openDirectory();
    if (dir) {
      newPath.value = dir;
      await refreshScopeTree();
    }
  } catch (error) {
    console.error(error);
  }
};

const toggleTreeSelection = (state: ScopePanelState, node: ScopeTreeNode) => {
  const targetLeafKeys = collectLeafKeysFromNode(node);
  const selected = new Set(state.selectedFilePaths);
  const allSelected = targetLeafKeys.every((key) => selected.has(key));

  if (allSelected) {
    for (const key of targetLeafKeys) {
      selected.delete(key);
    }
  } else {
    for (const key of targetLeafKeys) {
      selected.add(key);
    }
  }

  const orderedLeafKeys = collectLeafKeysFromTree(state.tree);
  state.selectedFilePaths = orderedLeafKeys.filter((key) => selected.has(key));
};

const selectAllScopes = (state: ScopePanelState) => {
  state.selectedFilePaths = collectLeafKeysFromTree(state.tree);
};

const clearScopeSelection = (state: ScopePanelState) => {
  state.selectedFilePaths = [];
};

const expandAllScopes = (state: ScopePanelState) => {
  state.expandedKeys = collectDirectoryKeysFromTree(state.tree);
};

const collapseAllScopes = (state: ScopePanelState) => {
  state.expandedKeys = [];
};

const toggleExpandedScope = (state: ScopePanelState, key: string) => {
  if (state.expandedKeys.includes(key)) {
    state.expandedKeys = state.expandedKeys.filter((item) => item !== key);
  } else {
    state.expandedKeys = [...state.expandedKeys, key];
  }
};

const clearResults = () => {
  result.value = null;
  progress.value = null;
  errorMsg.value = '';
  exportMessage.value = null;
};

const formatTimestampForFileName = () => {
  const now = new Date();
  const pad = (value: number) => value.toString().padStart(2, '0');

  return [
    now.getFullYear(),
    pad(now.getMonth() + 1),
    pad(now.getDate()),
    '-',
    pad(now.getHours()),
    pad(now.getMinutes()),
    pad(now.getSeconds()),
  ].join('');
};

const buildDefaultFileName = (format: ExportFormat) => {
  const modeName = mode.value === 'incremental' ? 'incremental' : 'project';
  return `code-statistics-${modeName}-${formatTimestampForFileName()}.${format}`;
};

const esc = (value: string) =>
  value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

const toCsvCell = (value: string | number) => {
  const text = String(value).replace(/"/g, '""');
  return `"${text}"`;
};

const buildCsvContent = (data: CodeCountResult) => {
  const lines = [
    [
      'File Path',
      'Code Added',
      'Code Deleted',
      'Code Modified',
      'Comment Added',
      'Comment Deleted',
      'Comment Modified',
    ].join(','),
  ];

  for (const file of data.files) {
    lines.push(
      [
        toCsvCell(file.filePath),
        file.codeAdded,
        file.codeDeleted,
        file.codeModified,
        file.commentAdded,
        file.commentDeleted,
        file.commentModified,
      ].join(','),
    );
  }

  lines.push('');
  lines.push(
    [
      'Summary',
      data.summary.codeAdded,
      data.summary.codeDeleted,
      data.summary.codeModified,
      data.summary.commentAdded,
      data.summary.commentDeleted,
      data.summary.commentModified,
    ].join(','),
  );

  lines.push('');
  lines.push(['Operation Summary', 'Value'].join(','));
  lines.push(['Added Total', data.operationSummary.addedTotal].join(','));
  lines.push(['Deleted Total', data.operationSummary.deletedTotal].join(','));
  lines.push(['Modified Total', data.operationSummary.modifiedTotal].join(','));
  lines.push(['Changed Total', data.operationSummary.changedTotal].join(','));
  lines.push('');
  if (mode.value === 'incremental') {
    lines.push(['Old Scope', toCsvCell(oldScopeSummaryText.value)].join(','));
    lines.push(['New Scope', toCsvCell(newScopeSummaryText.value)].join(','));
  } else {
    lines.push(['Scope', toCsvCell(projectScopeSummaryText.value)].join(','));
  }
  lines.push(['Extension Filter', toCsvCell(extensionFilterSummary.value)].join(','));

  return `\uFEFF${lines.join('\n')}`;
};

const generateHtmlReport = (data: CodeCountResult) => {
  const oldScopeText = oldScopeSummaryText.value;
  const newScopeText = newScopeSummaryText.value;
  const projectScopeText = projectScopeSummaryText.value;
  const extensionFilterText = extensionFilterSummary.value;
  const fileTypeRows = fileTypeSummaryEntries.value
    .map(
      (entry) => `
        <tr>
          <td>${esc(entry.ext)}</td>
          <td>${entry.codeAdded}</td>
          <td>${entry.codeDeleted}</td>
          <td>${entry.codeModified}</td>
          <td>${entry.commentAdded}</td>
          <td>${entry.commentDeleted}</td>
          <td>${entry.commentModified}</td>
          <td>${entry.total}</td>
        </tr>`,
    )
    .join('');

  const fileRows = data.files
    .map(
      (file) => `
        <tr>
          <td>${esc(file.filePath)}</td>
          <td>${file.codeAdded}</td>
          <td>${file.codeDeleted}</td>
          <td>${file.codeModified}</td>
          <td>${file.commentAdded}</td>
          <td>${file.commentDeleted}</td>
          <td>${file.commentModified}</td>
        </tr>`,
    )
    .join('');

  return `<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>代码修改统计报告</title>
    <style>
      * { box-sizing: border-box; }
      body {
        margin: 0;
        font-family: "Microsoft YaHei", "Segoe UI", sans-serif;
        background: #f4f7fb;
        color: #0f172a;
      }
      .container {
        max-width: 1280px;
        margin: 0 auto;
        padding: 32px 20px 48px;
      }
      .hero,
      .section {
        background: #ffffff;
        border: 1px solid #dbe4f0;
        border-radius: 18px;
        box-shadow: 0 10px 30px rgba(15, 23, 42, 0.05);
      }
      .hero {
        padding: 28px;
        margin-bottom: 20px;
        background: linear-gradient(135deg, #ffffff 0%, #eef6ff 100%);
      }
      .hero h1 {
        margin: 0 0 8px;
        font-size: 30px;
      }
      .hero p {
        margin: 0;
        color: #475569;
      }
      .meta {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
        gap: 12px;
        margin-top: 20px;
      }
      .meta-item {
        padding: 14px 16px;
        border-radius: 14px;
        background: rgba(255, 255, 255, 0.85);
        border: 1px solid #dbeafe;
      }
      .meta-label {
        font-size: 12px;
        color: #64748b;
        margin-bottom: 6px;
      }
      .meta-value {
        font-size: 14px;
        color: #0f172a;
        word-break: break-all;
      }
      .section {
        padding: 24px;
        margin-bottom: 18px;
      }
      .section h2 {
        margin: 0 0 16px;
        font-size: 22px;
      }
      .cards {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
        gap: 16px;
      }
      .card {
        border-radius: 16px;
        padding: 18px;
        color: #ffffff;
      }
      .card small {
        display: block;
        opacity: 0.92;
        margin-bottom: 10px;
      }
      .card strong {
        font-size: 34px;
      }
      .card-green { background: linear-gradient(135deg, #16a34a, #34d399); }
      .card-red { background: linear-gradient(135deg, #dc2626, #fb7185); }
      .card-amber { background: linear-gradient(135deg, #d97706, #fbbf24); }
      .card-sky { background: linear-gradient(135deg, #0284c7, #38bdf8); }
      .stats {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
        gap: 16px;
        margin-top: 18px;
      }
      .stat-panel {
        border: 1px solid #e2e8f0;
        border-radius: 16px;
        padding: 18px;
        background: #f8fafc;
      }
      .stat-panel h3 {
        margin: 0 0 12px;
        font-size: 18px;
      }
      .stat-row {
        display: flex;
        justify-content: space-between;
        gap: 12px;
        padding: 8px 0;
        border-bottom: 1px solid #e2e8f0;
      }
      .stat-row:last-child {
        border-bottom: none;
      }
      .table-wrap {
        overflow: auto;
      }
      table {
        width: 100%;
        border-collapse: collapse;
        font-size: 14px;
      }
      th,
      td {
        padding: 10px 12px;
        border-bottom: 1px solid #e2e8f0;
        text-align: right;
        white-space: nowrap;
      }
      th:first-child,
      td:first-child {
        text-align: left;
      }
      thead th {
        background: #eff6ff;
        color: #1e3a8a;
      }
      tbody tr:nth-child(even) {
        background: #f8fafc;
      }
    </style>
  </head>
  <body>
    <div class="container">
      <section class="hero">
        <h1>代码修改统计报告</h1>
        <p>生成时间：${esc(new Date().toLocaleString('zh-CN'))}</p>
        <div class="meta">
          <div class="meta-item">
            <div class="meta-label">统计模式</div>
            <div class="meta-value">${esc(
              mode.value === 'incremental'
                ? t('codeStatistics.modeIncremental')
                : t('codeStatistics.modeNewProject'),
            )}</div>
          </div>
          ${
            mode.value === 'incremental'
              ? `
          <div class="meta-item">
            <div class="meta-label">旧版本代码路径</div>
            <div class="meta-value">${esc(oldPath.value.trim())}</div>
          </div>
          <div class="meta-item">
            <div class="meta-label">新版本代码路径</div>
            <div class="meta-value">${esc(newPath.value.trim())}</div>
          </div>`
              : `
          <div class="meta-item">
            <div class="meta-label">项目代码路径</div>
            <div class="meta-value">${esc(newPath.value.trim())}</div>
          </div>`
          }
          ${
            mode.value === 'incremental'
              ? `
          <div class="meta-item">
            <div class="meta-label">旧版本代码范围</div>
            <div class="meta-value">${esc(oldScopeText)}</div>
          </div>
          <div class="meta-item">
            <div class="meta-label">新版本代码范围</div>
            <div class="meta-value">${esc(newScopeText)}</div>
          </div>`
              : `
          <div class="meta-item">
            <div class="meta-label">项目代码范围</div>
            <div class="meta-value">${esc(projectScopeText)}</div>
          </div>`
          }
          <div class="meta-item">
            <div class="meta-label">后缀过滤</div>
            <div class="meta-value">${esc(extensionFilterText)}</div>
          </div>
        </div>
      </section>

      <section class="section">
        <h2>汇总统计</h2>
        <div class="cards">
          <div class="card card-green">
            <small>新增总计</small>
            <strong>${data.operationSummary.addedTotal}</strong>
          </div>
          <div class="card card-red">
            <small>删除总计</small>
            <strong>${data.operationSummary.deletedTotal}</strong>
          </div>
          <div class="card card-amber">
            <small>修改总计</small>
            <strong>${data.operationSummary.modifiedTotal}</strong>
          </div>
          <div class="card card-sky">
            <small>变更总计</small>
            <strong>${data.operationSummary.changedTotal}</strong>
          </div>
        </div>
        <div class="stats">
          <div class="stat-panel">
            <h3>代码统计</h3>
            <div class="stat-row"><span>新增</span><strong>${data.summary.codeAdded}</strong></div>
            <div class="stat-row"><span>删除</span><strong>${data.summary.codeDeleted}</strong></div>
            <div class="stat-row"><span>修改</span><strong>${data.summary.codeModified}</strong></div>
            <div class="stat-row"><span>净变更</span><strong>${netCode.value}</strong></div>
          </div>
          <div class="stat-panel">
            <h3>注释统计</h3>
            <div class="stat-row"><span>新增</span><strong>${data.summary.commentAdded}</strong></div>
            <div class="stat-row"><span>删除</span><strong>${data.summary.commentDeleted}</strong></div>
            <div class="stat-row"><span>修改</span><strong>${data.summary.commentModified}</strong></div>
            <div class="stat-row"><span>净变更</span><strong>${netComment.value}</strong></div>
          </div>
        </div>
      </section>

      <section class="section">
        <h2>文件类型统计</h2>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>文件类型</th>
                <th>代码+</th>
                <th>代码-</th>
                <th>代码~</th>
                <th>注释+</th>
                <th>注释-</th>
                <th>注释~</th>
                <th>总变更</th>
              </tr>
            </thead>
            <tbody>
              ${fileTypeRows || `<tr><td colspan="8">暂无数据</td></tr>`}
            </tbody>
          </table>
        </div>
      </section>

      <section class="section">
        <h2>文件明细</h2>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>文件路径</th>
                <th>代码+</th>
                <th>代码-</th>
                <th>代码~</th>
                <th>注释+</th>
                <th>注释-</th>
                <th>注释~</th>
              </tr>
            </thead>
            <tbody>
              ${fileRows || `<tr><td colspan="7">暂无数据</td></tr>`}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  </body>
</html>`;
};

const handleExport = async (format: ExportFormat) => {
  if (!result.value) return;

  isExporting.value = format;
  exportMessage.value = null;

  try {
    const content =
      format === 'csv' ? buildCsvContent(result.value) : generateHtmlReport(result.value);
    const savedPath = await saveTextFile(
      content,
      buildDefaultFileName(format),
      format === 'csv' ? 'CSV Files' : 'HTML Files',
      [format],
    );

    if (!savedPath) {
      exportMessage.value = {
        type: 'info',
        text: t('codeStatistics.exportCancelled'),
      };
      return;
    }

    exportMessage.value = {
      type: 'success',
      text: t('codeStatistics.exportSuccess', { format: format.toUpperCase() }),
      path: savedPath,
    };
  } catch (error) {
    exportMessage.value = {
      type: 'error',
      text: t('codeStatistics.exportFailed', {
        error: error instanceof Error ? error.message : String(error),
      }),
    };
  } finally {
    isExporting.value = null;
  }
};

const openExportLocation = async () => {
  if (!exportMessage.value?.path) return;

  try {
    await openPathParent(exportMessage.value.path);
  } catch (error) {
    exportMessage.value = {
      type: 'error',
      text: t('codeStatistics.openExportPathFailed', {
        error: error instanceof Error ? error.message : String(error),
      }),
      path: exportMessage.value.path,
    };
  }
};

const startAnalysis = async () => {
  if (mode.value === 'incremental') {
    if (!oldPath.value.trim() || !newPath.value.trim()) {
      errorMsg.value = t('codeStatistics.fillRequired');
      return;
    }
  } else if (!newPath.value.trim()) {
    errorMsg.value = t('codeStatistics.fillRequiredNewProject');
    return;
  }

  await refreshScopeTree();

  const missingScopePanels = getMissingScopePanelTitles();
  if (missingScopePanels.length > 0) {
    errorMsg.value = t('codeStatistics.selectAtLeastOneScopeFor', {
      labels: missingScopePanels.join('、'),
    });
    return;
  }

  isAnalyzing.value = true;
  errorMsg.value = '';
  exportMessage.value = null;
  result.value = null;
  progress.value = null;

  try {
    const oldPathArg = mode.value === 'incremental' ? oldPath.value.trim() : '';
    result.value = await codeCountAnalyze(
      oldPathArg,
      newPath.value.trim(),
      mode.value === 'incremental'
        ? getScopeTotalSelectableFiles(oldScopeState) > 0
          ? oldScopeState.selectedFilePaths
          : undefined
        : undefined,
      mode.value === 'incremental'
        ? getScopeTotalSelectableFiles(newScopeState) > 0
          ? newScopeState.selectedFilePaths
          : undefined
        : getScopeTotalSelectableFiles(projectScopeState) > 0
          ? projectScopeState.selectedFilePaths
          : undefined,
      includedExtensions.value,
      excludedExtensions.value,
    );
  } catch (error) {
    errorMsg.value = error instanceof Error ? error.message : String(error);
  } finally {
    isAnalyzing.value = false;
  }
};

watch(mode, () => {
  exportMessage.value = null;
  errorMsg.value = '';
  void refreshScopeTree();
});

onMounted(async () => {
  unlistenProgress = await listen<CodeCountProgress>('code-count-progress', (event) => {
    progress.value = event.payload;
  });
});

onUnmounted(() => {
  unlistenProgress?.();
});
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 p-8 overflow-y-auto">
    <div class="mb-6 flex items-start justify-between gap-4">
      <div>
        <h1 class="text-4xl font-bold text-slate-900 mb-2">{{ t('codeStatistics.title') }}</h1>
        <p class="text-slate-600 text-lg">
          {{
            mode === 'incremental'
              ? t('codeStatistics.modeIncrementalDesc')
              : t('codeStatistics.modeNewProjectDesc')
          }}
        </p>
      </div>
      <button
        v-if="result"
        @click="clearResults"
        class="flex items-center gap-1.5 px-4 py-2 text-sm font-medium text-red-600 bg-red-50 hover:bg-red-100 border border-red-200 rounded-lg transition-colors shrink-0"
      >
        <Trash2 class="w-4 h-4" />
        {{ t('codeStatistics.clearResults') }}
      </button>
    </div>

    <div class="bg-white border border-slate-200 rounded-lg p-1.5 shadow-sm mb-6 inline-flex self-start">
      <button
        @click="mode = 'incremental'"
        class="flex items-center gap-2 px-5 py-2.5 rounded-md text-sm font-medium transition-all duration-200"
        :class="
          mode === 'incremental'
            ? 'bg-blue-600 text-white shadow-sm'
            : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
        "
      >
        <GitCompareArrows class="w-4 h-4" />
        {{ t('codeStatistics.modeIncremental') }}
      </button>
      <button
        @click="mode = 'newProject'"
        class="flex items-center gap-2 px-5 py-2.5 rounded-md text-sm font-medium transition-all duration-200"
        :class="
          mode === 'newProject'
            ? 'bg-blue-600 text-white shadow-sm'
            : 'text-slate-600 hover:text-slate-900 hover:bg-slate-50'
        "
      >
        <FolderPlus class="w-4 h-4" />
        {{ t('codeStatistics.modeNewProject') }}
      </button>
    </div>

    <div class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm mb-6">
      <div v-if="mode === 'incremental'" class="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
        <div>
          <label class="block text-sm font-semibold text-slate-700 mb-2">
            {{ t('codeStatistics.oldPath') }}
          </label>
          <div class="flex gap-2">
            <input
              v-model="oldPath"
              type="text"
              :placeholder="t('codeStatistics.oldPathPlaceholder')"
              :disabled="isAnalyzing"
              class="flex-1 px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
              @blur="handlePathBlur"
            />
            <button
              @click="browseOld"
              :disabled="isAnalyzing"
              class="px-3 py-2 bg-slate-100 border border-slate-300 rounded-lg hover:bg-slate-200 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              :title="t('codeStatistics.browse')"
            >
              <FolderOpen class="w-5 h-5 text-slate-600" />
            </button>
          </div>
        </div>
        <div>
          <label class="block text-sm font-semibold text-slate-700 mb-2">
            {{ t('codeStatistics.newPath') }}
          </label>
          <div class="flex gap-2">
            <input
              v-model="newPath"
              type="text"
              :placeholder="t('codeStatistics.newPathPlaceholder')"
              :disabled="isAnalyzing"
              class="flex-1 px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
              @blur="handlePathBlur"
            />
            <button
              @click="browseNew"
              :disabled="isAnalyzing"
              class="px-3 py-2 bg-slate-100 border border-slate-300 rounded-lg hover:bg-slate-200 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              :title="t('codeStatistics.browse')"
            >
              <FolderOpen class="w-5 h-5 text-slate-600" />
            </button>
          </div>
        </div>
      </div>

      <div v-else class="mb-6">
        <label class="block text-sm font-semibold text-slate-700 mb-2">
          {{ t('codeStatistics.projectPath') }}
        </label>
        <div class="flex gap-2">
          <input
            v-model="newPath"
            type="text"
            :placeholder="t('codeStatistics.projectPathPlaceholder')"
            :disabled="isAnalyzing"
            class="flex-1 px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
            @blur="handlePathBlur"
          />
          <button
            @click="browseNew"
            :disabled="isAnalyzing"
            class="px-3 py-2 bg-slate-100 border border-slate-300 rounded-lg hover:bg-slate-200 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            :title="t('codeStatistics.browse')"
          >
            <FolderOpen class="w-5 h-5 text-slate-600" />
          </button>
        </div>
      </div>

      <div class="mb-6 rounded-2xl border border-slate-200 bg-slate-50/70 p-5">
        <div class="mb-4">
          <h3 class="text-base font-semibold text-slate-900">
            {{ t('codeStatistics.extensionFilterTitle') }}
          </h3>
          <p class="mt-1 text-sm text-slate-500">
            {{ t('codeStatistics.extensionFilterDescription') }}
          </p>
        </div>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-semibold text-slate-700 mb-2">
              {{ t('codeStatistics.includeExtensionsLabel') }}
            </label>
            <input
              v-model="includeExtensionsInput"
              type="text"
              :placeholder="t('codeStatistics.includeExtensionsPlaceholder')"
              :disabled="isAnalyzing"
              class="w-full px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
              @blur="handleFilterBlur"
            />
          </div>
          <div>
            <label class="block text-sm font-semibold text-slate-700 mb-2">
              {{ t('codeStatistics.excludeExtensionsLabel') }}
            </label>
            <input
              v-model="excludeExtensionsInput"
              type="text"
              :placeholder="t('codeStatistics.excludeExtensionsPlaceholder')"
              :disabled="isAnalyzing"
              class="w-full px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
              @blur="handleFilterBlur"
            />
          </div>
        </div>
        <p class="mt-3 text-xs text-slate-500">
          {{ t('codeStatistics.extensionFilterHint') }}
        </p>
        <p class="mt-2 text-xs font-medium text-slate-600">
          {{ extensionFilterSummary }}
        </p>
      </div>

      <div
        v-if="shouldShowScopeSelector"
        class="mb-6 rounded-2xl border border-slate-200 bg-slate-50/70 p-5"
      >
        <div class="flex flex-wrap items-start justify-between gap-4 mb-4">
          <div>
            <h3 class="text-base font-semibold text-slate-900">
              {{ t('codeStatistics.scopeTitle') }}
            </h3>
            <p class="text-sm text-slate-500 mt-1">
              {{ t('codeStatistics.scopeDescription') }}
            </p>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <button
              @click="refreshScopeTree"
              type="button"
              class="px-3 py-1.5 text-sm border border-slate-300 text-slate-600 hover:bg-white rounded-lg transition-colors"
              :disabled="isAnalyzing || isLoadingScopes"
            >
              {{ t('codeStatistics.refreshScopes') }}
            </button>
          </div>
        </div>

        <div
          class="grid gap-4"
          :class="mode === 'incremental' ? 'grid-cols-1 xl:grid-cols-2' : 'grid-cols-1'"
        >
          <div
            v-for="panel in scopePanels"
            :key="panel.key"
            class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm"
          >
            <div class="mb-4">
              <div class="min-w-0 min-h-[3.5rem]">
                <h4 class="text-sm font-semibold text-slate-900">{{ panel.title }}</h4>
                <p class="mt-1 text-xs font-mono text-slate-400 break-all">{{ panel.path }}</p>
              </div>
              <div class="mt-3 flex flex-wrap items-center gap-2">
                <button
                  @click="expandAllScopes(panel.state)"
                  type="button"
                  class="px-3 py-1.5 text-sm border border-slate-300 text-slate-600 hover:bg-slate-50 rounded-lg transition-colors"
                  :disabled="isAnalyzing || panel.state.isLoading || panel.state.tree.length === 0"
                >
                  {{ t('codeStatistics.expandAllScopes') }}
                </button>
                <button
                  @click="collapseAllScopes(panel.state)"
                  type="button"
                  class="px-3 py-1.5 text-sm border border-slate-300 text-slate-600 hover:bg-slate-50 rounded-lg transition-colors"
                  :disabled="isAnalyzing || panel.state.isLoading || panel.state.tree.length === 0"
                >
                  {{ t('codeStatistics.collapseAllScopes') }}
                </button>
                <button
                  @click="selectAllScopes(panel.state)"
                  type="button"
                  class="px-3 py-1.5 text-sm border border-slate-300 text-slate-600 hover:bg-slate-50 rounded-lg transition-colors"
                  :disabled="isAnalyzing || panel.state.isLoading || panel.state.tree.length === 0"
                >
                  {{ t('codeStatistics.selectAllScopes') }}
                </button>
                <button
                  @click="clearScopeSelection(panel.state)"
                  type="button"
                  class="px-3 py-1.5 text-sm border border-slate-300 text-slate-600 hover:bg-slate-50 rounded-lg transition-colors"
                  :disabled="isAnalyzing || panel.state.isLoading || panel.state.tree.length === 0"
                >
                  {{ t('codeStatistics.clearScopeSelection') }}
                </button>
              </div>
            </div>

            <div
              v-if="panel.state.isLoading"
              class="flex items-center gap-2 rounded-xl border border-dashed border-slate-300 bg-slate-50 px-4 py-5 text-sm text-slate-600"
            >
              <Loader class="w-4 h-4 animate-spin" />
              {{ t('codeStatistics.loadingScopes') }}
            </div>

            <div
              v-else-if="panel.state.error"
              class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
            >
              {{ panel.state.error }}
            </div>

            <div
              v-else-if="panel.state.tree.length === 0"
              class="rounded-xl border border-dashed border-slate-300 bg-slate-50 px-4 py-5 text-sm text-slate-500"
            >
              {{ t('codeStatistics.noScopeOptions') }}
            </div>

            <div v-else>
              <div class="flex flex-wrap items-center justify-between gap-2 mb-3 text-xs text-slate-500">
                <span>
                  {{
                    t('codeStatistics.scopeSelectionSummary', {
                      selected: getScopeSelectedFileCount(panel.state),
                      total: getScopeTotalSelectableFiles(panel.state),
                    })
                  }}
                </span>
                <span>{{ t('codeStatistics.scopeHint') }}</span>
              </div>
              <div class="rounded-2xl border border-slate-200 bg-slate-50/50 p-3 max-h-[420px] overflow-y-auto">
                <CodeStatisticsScopeTreeNode
                  v-for="node in panel.state.tree"
                  :key="node.key"
                  :node="node"
                  :selected-leaf-keys="panel.state.selectedFilePaths"
                  :expanded-keys="panel.state.expandedKeys"
                  @toggle-selection="toggleTreeSelection(panel.state, $event)"
                  @toggle-expand="toggleExpandedScope(panel.state, $event)"
                />
              </div>
            </div>
          </div>
        </div>
      </div>

      <button
        @click="startAnalysis"
        :disabled="isAnalyzing"
        class="w-full px-6 py-3 bg-gradient-to-r from-blue-600 to-blue-700 text-white font-semibold rounded-lg hover:from-blue-700 hover:to-blue-800 focus:outline-none focus:ring-2 focus:ring-blue-500/50 disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2 text-lg"
      >
        <Loader v-if="isAnalyzing" class="w-5 h-5 animate-spin" />
        <BarChart3 v-else class="w-5 h-5" />
        <span>{{ isAnalyzing ? t('codeStatistics.analyzing') : t('codeStatistics.startAnalysis') }}</span>
      </button>
    </div>

    <div v-if="isAnalyzing && progress" class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm mb-6">
      <div class="flex items-center justify-between mb-2">
        <span class="text-sm font-medium text-slate-700">{{ phaseLabel }}</span>
        <span class="text-sm text-slate-500">{{ progress.percent }}%</span>
      </div>
      <div class="w-full bg-slate-200 rounded-full h-2.5 mb-2">
        <div
          class="bg-gradient-to-r from-blue-500 to-blue-600 h-2.5 rounded-full transition-all duration-300"
          :style="{ width: `${progress.percent}%` }"
        ></div>
      </div>
      <p class="text-xs text-slate-500 truncate">{{ progress.currentFile }}</p>
    </div>

    <div v-if="errorMsg" class="bg-red-50 border-l-4 border-red-500 p-4 rounded-lg mb-6">
      <p class="text-red-800 text-sm">{{ errorMsg }}</p>
    </div>

    <template v-if="result">
      <div class="mb-6 rounded-2xl border border-slate-200 bg-white p-5 shadow-sm">
        <div class="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <p class="text-sm font-medium text-slate-700">{{ t('codeStatistics.currentScopeLabel') }}</p>
            <p
              v-for="line in resultScopeSummaryLines"
              :key="line"
              class="mt-1 text-sm text-slate-500"
            >
              {{ line }}
            </p>
            <p class="mt-1 text-xs text-slate-400">{{ extensionFilterSummary }}</p>
            <p class="mt-2 text-xs text-slate-400">{{ t('codeStatistics.exportActionsDesc') }}</p>
          </div>
          <div class="flex flex-wrap gap-2">
            <button
              @click="handleExport('csv')"
              :disabled="isExporting !== null"
              class="flex items-center gap-1.5 text-sm px-3 py-1.5 bg-slate-100 hover:bg-slate-200 text-slate-700 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Loader v-if="isExporting === 'csv'" class="w-4 h-4 animate-spin" />
              <Download v-else class="w-4 h-4" />
              {{ t('codeStatistics.exportCsv') }}
            </button>
            <button
              @click="handleExport('html')"
              :disabled="isExporting !== null"
              class="flex items-center gap-1.5 text-sm px-3 py-1.5 bg-blue-50 hover:bg-blue-100 text-blue-700 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Loader v-if="isExporting === 'html'" class="w-4 h-4 animate-spin" />
              <FileCode v-else class="w-4 h-4" />
              {{ t('codeStatistics.exportHtml') }}
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="exportMessage"
        class="mb-6 rounded-xl border px-4 py-3"
        :class="exportMessageClasses"
      >
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="min-w-0">
            <p class="text-sm font-medium">{{ exportMessage.text }}</p>
            <button
              v-if="exportMessage.path"
              type="button"
              class="mt-1 text-xs font-mono underline underline-offset-2 break-all text-left"
              @click="openExportLocation"
            >
              {{ exportMessage.path }}
            </button>
          </div>
          <button
            v-if="exportMessage.path"
            type="button"
            class="px-3 py-1.5 text-xs rounded-lg border border-current/20 bg-white/60 hover:bg-white transition-colors"
            @click="openExportLocation"
          >
            {{ t('codeStatistics.openExportFolder') }}
          </button>
        </div>
      </div>

      <div v-if="result.files.length === 0" class="bg-green-50 border-l-4 border-green-500 p-4 rounded-lg mb-6">
        <p class="text-green-800 text-sm">
          {{
            mode === 'incremental'
              ? t('codeStatistics.noChanges')
              : t('codeStatistics.noChangesNewProject')
          }}
        </p>
      </div>

      <template v-else>
        <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4 mb-6">
          <div class="rounded-lg p-5 text-white text-center bg-gradient-to-br from-green-500 to-emerald-400 shadow-sm">
            <div class="text-sm font-medium mb-1 opacity-90">{{ t('codeStatistics.totalAdded') }}</div>
            <div class="text-4xl font-bold">{{ result.operationSummary.addedTotal }}</div>
          </div>
          <div class="rounded-lg p-5 text-white text-center bg-gradient-to-br from-red-500 to-rose-400 shadow-sm">
            <div class="text-sm font-medium mb-1 opacity-90">{{ t('codeStatistics.totalDeleted') }}</div>
            <div class="text-4xl font-bold">{{ result.operationSummary.deletedTotal }}</div>
          </div>
          <div class="rounded-lg p-5 text-white text-center bg-gradient-to-br from-amber-500 to-yellow-400 shadow-sm">
            <div class="text-sm font-medium mb-1 opacity-90">{{ t('codeStatistics.totalModified') }}</div>
            <div class="text-4xl font-bold">{{ result.operationSummary.modifiedTotal }}</div>
          </div>
          <div class="rounded-lg p-5 text-white text-center bg-gradient-to-br from-sky-500 to-cyan-400 shadow-sm">
            <div class="text-sm font-medium mb-1 opacity-90">{{ t('codeStatistics.totalChanged') }}</div>
            <div class="text-4xl font-bold">{{ totalChanged }}</div>
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6">
          <div class="bg-white border border-slate-200 rounded-lg p-5 shadow-sm border-l-4 border-l-blue-500">
            <h4 class="font-semibold text-slate-800 mb-3">{{ t('codeStatistics.codeStats') }}</h4>
            <div class="space-y-1.5">
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.added') }}</span>
                <strong class="text-green-600">+{{ result.summary.codeAdded }}</strong>
              </div>
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.deleted') }}</span>
                <strong class="text-red-600">-{{ result.summary.codeDeleted }}</strong>
              </div>
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.modified') }}</span>
                <strong class="text-amber-600">~{{ result.summary.codeModified }}</strong>
              </div>
              <div class="flex justify-between py-1">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.netChange') }}</span>
                <strong :class="netCode >= 0 ? 'text-green-600' : 'text-red-600'">
                  {{ netCode >= 0 ? '+' : '' }}{{ netCode }}
                </strong>
              </div>
            </div>
          </div>
          <div class="bg-white border border-slate-200 rounded-lg p-5 shadow-sm border-l-4 border-l-teal-500">
            <h4 class="font-semibold text-slate-800 mb-3">{{ t('codeStatistics.commentStats') }}</h4>
            <div class="space-y-1.5">
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.added') }}</span>
                <strong class="text-green-600">+{{ result.summary.commentAdded }}</strong>
              </div>
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.deleted') }}</span>
                <strong class="text-red-600">-{{ result.summary.commentDeleted }}</strong>
              </div>
              <div class="flex justify-between py-1 border-b border-slate-100">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.modified') }}</span>
                <strong class="text-amber-600">~{{ result.summary.commentModified }}</strong>
              </div>
              <div class="flex justify-between py-1">
                <span class="text-slate-600 text-sm">{{ t('codeStatistics.netChange') }}</span>
                <strong :class="netComment >= 0 ? 'text-green-600' : 'text-red-600'">
                  {{ netComment >= 0 ? '+' : '' }}{{ netComment }}
                </strong>
              </div>
            </div>
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6">
          <div class="bg-white border border-slate-200 rounded-lg p-5 shadow-sm">
            <h4 class="font-semibold text-slate-800 mb-4 text-center">
              {{ t('codeStatistics.codeCommentChart') }}
            </h4>
            <div class="flex items-end justify-around border-b border-slate-200 pb-3 mb-3" style="height: 190px;">
              <div class="flex flex-col items-center">
                <div class="flex items-end gap-1.5 h-40">
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.codeAdded }}</span>
                    <div class="w-full bg-green-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.codeAdded) }"></div>
                  </div>
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.codeDeleted }}</span>
                    <div class="w-full bg-red-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.codeDeleted) }"></div>
                  </div>
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.codeModified }}</span>
                    <div class="w-full bg-amber-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.codeModified) }"></div>
                  </div>
                </div>
                <span class="text-xs text-slate-500 mt-1.5 font-medium">{{ t('codeStatistics.code') }}</span>
              </div>
              <div class="flex flex-col items-center">
                <div class="flex items-end gap-1.5 h-40">
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.commentAdded }}</span>
                    <div class="w-full bg-green-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.commentAdded) }"></div>
                  </div>
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.commentDeleted }}</span>
                    <div class="w-full bg-red-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.commentDeleted) }"></div>
                  </div>
                  <div class="flex flex-col items-center justify-end w-7 h-full">
                    <span class="text-[10px] font-bold text-slate-600 mb-0.5 leading-none">{{ result.summary.commentModified }}</span>
                    <div class="w-full bg-amber-400 rounded-t transition-all" :style="{ height: barHeight(result.summary.commentModified) }"></div>
                  </div>
                </div>
                <span class="text-xs text-slate-500 mt-1.5 font-medium">{{ t('codeStatistics.comment') }}</span>
              </div>
            </div>
            <div class="flex items-center justify-center gap-4 text-xs text-slate-600">
              <div class="flex items-center gap-1">
                <div class="w-3 h-3 bg-green-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.added') }}
              </div>
              <div class="flex items-center gap-1">
                <div class="w-3 h-3 bg-red-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.deleted') }}
              </div>
              <div class="flex items-center gap-1">
                <div class="w-3 h-3 bg-amber-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.modified') }}
              </div>
            </div>
          </div>

          <div class="bg-white border border-slate-200 rounded-lg p-5 shadow-sm flex flex-col items-center justify-center">
            <h4 class="font-semibold text-slate-800 mb-4 text-center">
              {{ t('codeStatistics.changeTypeChart') }}
            </h4>
            <div class="w-40 h-40 rounded-full shadow-inner" :style="pieStyle"></div>
            <div class="flex flex-wrap justify-center gap-4 mt-5 text-sm text-slate-700">
              <div class="flex items-center gap-1.5">
                <div class="w-3 h-3 bg-green-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.added') }} ({{ result.operationSummary.addedTotal }})
              </div>
              <div class="flex items-center gap-1.5">
                <div class="w-3 h-3 bg-red-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.deleted') }} ({{ result.operationSummary.deletedTotal }})
              </div>
              <div class="flex items-center gap-1.5">
                <div class="w-3 h-3 bg-amber-400 rounded-sm shrink-0"></div>
                {{ t('codeStatistics.modified') }} ({{ result.operationSummary.modifiedTotal }})
              </div>
            </div>
          </div>
        </div>

        <div v-if="fileTypeSummaryEntries.length > 0" class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm mb-6">
          <h3 class="text-lg font-semibold text-slate-900 mb-5">{{ t('codeStatistics.fileTypeTitle') }}</h3>

          <div class="space-y-2 mb-6">
            <div v-for="entry in fileTypeSummaryEntries" :key="entry.ext" class="flex items-center gap-3">
              <span class="w-12 text-sm font-mono font-semibold text-slate-700 shrink-0">{{ entry.ext }}</span>
              <div class="flex-1 bg-slate-100 rounded-full h-5 overflow-hidden">
                <div
                  class="h-full bg-gradient-to-r from-blue-500 to-cyan-500 rounded-full transition-all"
                  :style="{ width: `${Math.round((entry.total / maxFileTypeTotal) * 100)}%` }"
                ></div>
              </div>
              <span class="text-sm font-bold text-slate-700 w-10 text-right shrink-0">{{ entry.total }}</span>
            </div>
          </div>

          <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-3 mb-6">
            <div
              v-for="entry in fileTypeSummaryEntries"
              :key="`${entry.ext}-card`"
              class="bg-slate-50 border border-slate-200 rounded-lg p-3 text-center"
            >
              <div class="font-mono font-bold text-blue-600 mb-1 text-sm">{{ entry.ext }}</div>
              <div class="text-2xl font-bold text-slate-800">{{ entry.total }}</div>
              <div class="text-xs text-slate-500 mt-1">
                {{ t('codeStatistics.code') }}: {{ entry.codeAdded + entry.codeDeleted + entry.codeModified }}
                &nbsp;|&nbsp;
                {{ t('codeStatistics.comment') }}: {{ entry.commentAdded + entry.commentDeleted + entry.commentModified }}
              </div>
            </div>
          </div>

          <h4 class="font-semibold text-slate-700 mb-2 text-sm">{{ t('codeStatistics.fileTypeDetail') }}</h4>
          <div class="overflow-x-auto">
            <table class="w-full">
              <thead>
                <tr class="border-b border-slate-200 bg-slate-50">
                  <th class="px-3 py-2 text-left text-xs font-semibold text-slate-600">{{ t('codeStatistics.fileTypeCol') }}</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-green-700">{{ t('codeStatistics.code') }}+</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-red-700">{{ t('codeStatistics.code') }}-</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-amber-700">{{ t('codeStatistics.code') }}~</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-green-700">{{ t('codeStatistics.comment') }}+</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-red-700">{{ t('codeStatistics.comment') }}-</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-amber-700">{{ t('codeStatistics.comment') }}~</th>
                  <th class="px-3 py-2 text-right text-xs font-semibold text-slate-700">{{ t('codeStatistics.totalChanges') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="entry in fileTypeSummaryEntries" :key="`${entry.ext}-row`" class="border-b border-slate-100 hover:bg-slate-50">
                  <td class="px-3 py-2"><span class="font-mono text-xs bg-slate-100 text-blue-600 px-2 py-0.5 rounded font-bold">{{ entry.ext }}</span></td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-green-600">{{ entry.codeAdded }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-red-600">{{ entry.codeDeleted }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-amber-600">{{ entry.codeModified }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-green-600">{{ entry.commentAdded }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-red-600">{{ entry.commentDeleted }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono text-amber-600">{{ entry.commentModified }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-slate-800">{{ entry.total }}</td>
                </tr>
                <tr v-if="fileTypeTotal" class="bg-slate-50 border-t-2 border-slate-200">
                  <td class="px-3 py-2 text-sm font-bold text-slate-800">{{ t('codeStatistics.totalRow') }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-green-600">{{ fileTypeTotal.codeAdded }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-red-600">{{ fileTypeTotal.codeDeleted }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-amber-600">{{ fileTypeTotal.codeModified }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-green-600">{{ fileTypeTotal.commentAdded }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-red-600">{{ fileTypeTotal.commentDeleted }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-amber-600">{{ fileTypeTotal.commentModified }}</td>
                  <td class="px-3 py-2 text-right text-sm font-mono font-bold text-slate-800">{{ fileTypeTotal.total }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
          <div class="px-6 py-4 border-b border-slate-200 flex flex-wrap items-center justify-between gap-3">
            <h3 class="text-lg font-semibold text-slate-900">
              {{ t('codeStatistics.fileListTitle') }} ({{ result.files.length }})
            </h3>
          </div>
          <div class="overflow-x-auto max-h-[400px] overflow-y-auto">
            <table class="w-full">
              <thead class="sticky top-0 z-10">
                <tr class="border-b border-slate-200 bg-slate-50">
                  <th class="px-4 py-3 text-left text-xs font-semibold text-slate-600">{{ t('codeStatistics.filePath') }}</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-green-700">{{ t('codeStatistics.code') }}+</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-red-700">{{ t('codeStatistics.code') }}-</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-amber-700">{{ t('codeStatistics.code') }}~</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-green-700">{{ t('codeStatistics.comment') }}+</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-red-700">{{ t('codeStatistics.comment') }}-</th>
                  <th class="px-3 py-3 text-right text-xs font-semibold text-amber-700">{{ t('codeStatistics.comment') }}~</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="file in result.files" :key="file.filePath" class="border-b border-slate-100 hover:bg-slate-50 transition-colors">
                  <td class="px-4 py-2 text-sm text-slate-800 font-mono truncate max-w-xs" :title="file.filePath">{{ file.filePath }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.codeAdded > 0 ? 'text-green-600' : 'text-slate-300'">{{ file.codeAdded > 0 ? `+${file.codeAdded}` : '-' }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.codeDeleted > 0 ? 'text-red-600' : 'text-slate-300'">{{ file.codeDeleted > 0 ? `-${file.codeDeleted}` : '-' }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.codeModified > 0 ? 'text-amber-600' : 'text-slate-300'">{{ file.codeModified > 0 ? `~${file.codeModified}` : '-' }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.commentAdded > 0 ? 'text-green-600' : 'text-slate-300'">{{ file.commentAdded > 0 ? `+${file.commentAdded}` : '-' }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.commentDeleted > 0 ? 'text-red-600' : 'text-slate-300'">{{ file.commentDeleted > 0 ? `-${file.commentDeleted}` : '-' }}</td>
                  <td class="px-3 py-2 text-sm text-right font-mono" :class="file.commentModified > 0 ? 'text-amber-600' : 'text-slate-300'">{{ file.commentModified > 0 ? `~${file.commentModified}` : '-' }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>
