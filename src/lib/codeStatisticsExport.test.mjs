import assert from 'node:assert/strict';

import { generateCodeStatisticsHtmlReport } from './codeStatisticsExport.ts';

const html = generateCodeStatisticsHtmlReport(
  {
    files: [
      {
        filePath: 'src/main.ts',
        codeAdded: 12,
        codeDeleted: 3,
        codeModified: 4,
        commentAdded: 1,
        commentDeleted: 0,
        commentModified: 2,
      },
    ],
    summary: {
      codeAdded: 12,
      codeDeleted: 3,
      codeModified: 4,
      commentAdded: 1,
      commentDeleted: 0,
      commentModified: 2,
    },
    operationSummary: {
      addedTotal: 13,
      deletedTotal: 3,
      modifiedTotal: 6,
      changedTotal: 22,
    },
    fileTypeSummary: {},
  },
  {
    mode: 'incremental',
    oldPath: 'C:/old-code',
    newPath: 'C:/new-code',
    oldScopeText: '旧版本参与对比文件数/总文件数：1 / 1',
    newScopeText: '新版本参与对比文件数/总文件数：1 / 1',
    projectScopeText: '',
    extensionFilterText: '后缀过滤规则：未设置，按全部受支持后缀统计',
    netCode: 9,
    netComment: 3,
    fileTypeSummaryEntries: [
      {
        ext: '.ts',
        total: 22,
        codeAdded: 12,
        codeDeleted: 3,
        codeModified: 4,
        commentAdded: 1,
        commentDeleted: 0,
        commentModified: 2,
      },
    ],
    t: (key) => {
      if (key === 'codeStatistics.modeIncremental') return '增量修改统计';
      if (key === 'codeStatistics.modeNewProject') return '全新项目统计';
      if (key === 'codeStatistics.fileTypeTitle') return '变更文件类型统计';
      return key;
    },
  },
);

assert.match(html, /<h2>变更文件类型统计<\/h2>/);
assert.doesNotMatch(html, /\{\{\s*t\('codeStatistics\.fileTypeTitle'\)\s*\}\}/);
