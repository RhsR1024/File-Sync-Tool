import type { CodeCountResult } from './tauri';

type AnalysisMode = 'incremental' | 'newProject';

export interface CodeStatisticsFileTypeSummaryEntry {
  ext: string;
  total: number;
  codeAdded: number;
  codeDeleted: number;
  codeModified: number;
  commentAdded: number;
  commentDeleted: number;
  commentModified: number;
}

export interface CodeStatisticsHtmlReportContext {
  mode: AnalysisMode;
  oldPath: string;
  newPath: string;
  oldScopeText: string;
  newScopeText: string;
  projectScopeText: string;
  extensionFilterText: string;
  netCode: number;
  netComment: number;
  fileTypeSummaryEntries: CodeStatisticsFileTypeSummaryEntry[];
  t: (key: string) => string;
}

const esc = (value: string) =>
  value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

export const generateCodeStatisticsHtmlReport = (
  data: CodeCountResult,
  context: CodeStatisticsHtmlReportContext,
) => {
  const {
    mode,
    oldPath,
    newPath,
    oldScopeText,
    newScopeText,
    projectScopeText,
    extensionFilterText,
    netCode,
    netComment,
    fileTypeSummaryEntries,
    t,
  } = context;

  const fileTypeRows = fileTypeSummaryEntries
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
              mode === 'incremental'
                ? t('codeStatistics.modeIncremental')
                : t('codeStatistics.modeNewProject'),
            )}</div>
          </div>
          ${
            mode === 'incremental'
              ? `
          <div class="meta-item">
            <div class="meta-label">旧版本代码路径</div>
            <div class="meta-value">${esc(oldPath.trim())}</div>
          </div>
          <div class="meta-item">
            <div class="meta-label">新版本代码路径</div>
            <div class="meta-value">${esc(newPath.trim())}</div>
          </div>`
              : `
          <div class="meta-item">
            <div class="meta-label">项目代码路径</div>
            <div class="meta-value">${esc(newPath.trim())}</div>
          </div>`
          }
          ${
            mode === 'incremental'
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
            <div class="stat-row"><span>净变更</span><strong>${netCode}</strong></div>
          </div>
          <div class="stat-panel">
            <h3>注释统计</h3>
            <div class="stat-row"><span>新增</span><strong>${data.summary.commentAdded}</strong></div>
            <div class="stat-row"><span>删除</span><strong>${data.summary.commentDeleted}</strong></div>
            <div class="stat-row"><span>修改</span><strong>${data.summary.commentModified}</strong></div>
            <div class="stat-row"><span>净变更</span><strong>${netComment}</strong></div>
          </div>
        </div>
      </section>

      <section class="section">
        <h2>${esc(t('codeStatistics.fileTypeTitle'))}</h2>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>文件类型</th>
                <th>代码新增</th>
                <th>代码删除</th>
                <th>代码修改</th>
                <th>注释新增</th>
                <th>注释删除</th>
                <th>注释修改</th>
                <th>总变更</th>
              </tr>
            </thead>
            <tbody>
              ${fileTypeRows || '<tr><td colspan="8">暂无数据</td></tr>'}
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
                <th>代码新增</th>
                <th>代码删除</th>
                <th>代码修改</th>
                <th>注释新增</th>
                <th>注释删除</th>
                <th>注释修改</th>
              </tr>
            </thead>
            <tbody>
              ${fileRows || '<tr><td colspan="7">暂无数据</td></tr>'}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  </body>
</html>`;
};
