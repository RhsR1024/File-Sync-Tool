(function () {
  const data = {
    productName: "File Share",
    workspaceLabel: "共享工作区",
    title: "把共享目录整理成一个真正好用的网页工具",
    subtitle: "在同一画布里完成浏览、搜索、上传和轻量管理，让文件共享更像产品，而不是临时后台。",
    session: {
      name: "设计协作空间",
      status: "已登录 · 可上传 · 可重命名",
      syncTime: "最近同步 09:42",
    },
    breadcrumbs: ["共享空间", "品牌资产", "2026 发布会"],
    summaryChips: ["单画布", "平衡密度", "高级浏览体验"],
    searchPlaceholder: "搜索文件名、目录或关键字",
    searchScope: ["当前目录", "全部共享目录"],
    flashTitle: "内容已更新",
    flashText: "过去 10 分钟内新增 3 个文件，最近一次刷新完成于 09:42。",
    actions: [
      { id: "upload-file", label: "上传文件", tone: "primary" },
      { id: "upload-folder", label: "上传目录", tone: "secondary" },
      { id: "new-folder", label: "新建目录", tone: "ghost" },
      { id: "refresh", label: "刷新", tone: "ghost" },
    ],
    rows: [
      {
        kind: "folder",
        name: "品牌资产",
        subtitle: "共享目录 · 12 项内容",
        size: "12 项",
        modified: "今天 09:36",
      },
      {
        kind: "folder",
        name: "发布会现场素材",
        subtitle: "目录 · 图片、视频与海报",
        size: "48 项",
        modified: "今天 08:14",
      },
      {
        kind: "pptx",
        name: "2026-Q2-发布会提案.pptx",
        subtitle: "演示文件 · 适合投屏审阅",
        size: "18.4 MB",
        modified: "昨天 21:12",
      },
      {
        kind: "xlsx",
        name: "共享目录权限矩阵.xlsx",
        subtitle: "表格 · 包含账号与目录映射",
        size: "2.1 MB",
        modified: "昨天 18:05",
      },
      {
        kind: "png",
        name: "现场布置参考图.png",
        subtitle: "图片 · 可直接网页预览",
        size: "6.9 MB",
        modified: "昨天 16:47",
      },
      {
        kind: "md",
        name: "screen-share-recovery-notes.md",
        subtitle: "文档 · 用于内部交接",
        size: "84 KB",
        modified: "周一 11:23",
      },
      {
        kind: "zip",
        name: "release-bundle-1.1.0.zip",
        subtitle: "压缩包 · 可归档下载",
        size: "128 MB",
        modified: "周一 09:08",
      },
    ],
    footerLeft: "当前方案为设计评审稿，目标是比较视觉层级与文件浏览体验。",
    footerRight: "保留现有核心能力：面包屑、搜索、上传、预览、下载、重命名、删除。",
  };

  const typeMap = {
    folder: { label: "DIR", accent: "folder" },
    pptx: { label: "PPT", accent: "accent" },
    xlsx: { label: "XLS", accent: "accent" },
    png: { label: "PNG", accent: "accent" },
    md: { label: "MD", accent: "accent" },
    zip: { label: "ZIP", accent: "accent" },
    default: { label: "FILE", accent: "accent" },
  };

  function escapeHtml(value) {
    return String(value)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function iconSvg(name) {
    if (name === "folder") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.5 7.5h5.2l1.8 2H20.5v7A2.5 2.5 0 0 1 18 19H6A2.5 2.5 0 0 1 3.5 16.5Z"></path></svg>';
    }
    if (name === "search") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5"></circle><path d="m16 16 4.5 4.5"></path></svg>';
    }
    if (name === "upload") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V4.5"></path><path d="m7.5 9 4.5-4.5L16.5 9"></path><path d="M4.5 16.5v1.5A1.5 1.5 0 0 0 6 19.5h12a1.5 1.5 0 0 0 1.5-1.5v-1.5"></path></svg>';
    }
    if (name === "preview") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2.5 12s3.5-5.5 9.5-5.5 9.5 5.5 9.5 5.5-3.5 5.5-9.5 5.5S2.5 12 2.5 12Z"></path><circle cx="12" cy="12" r="2.6"></circle></svg>';
    }
    if (name === "download") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4.5v9"></path><path d="m8 10.5 4 4 4-4"></path><path d="M4.5 18h15"></path></svg>';
    }
    if (name === "rename") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m4.5 16.5 8.7-8.7 4 4-8.7 8.7-4.5.5Z"></path><path d="m14.3 5.7 1.5-1.5a1.5 1.5 0 0 1 2.1 0l1 1a1.5 1.5 0 0 1 0 2.1l-1.5 1.5"></path></svg>';
    }
    if (name === "delete") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5.5 7.5h13"></path><path d="M9.5 4.5h5l1 2h-7Z"></path><path d="M8.5 7.5 9.2 19a1.5 1.5 0 0 0 1.5 1.4h2.6a1.5 1.5 0 0 0 1.5-1.4l.7-11.5"></path></svg>';
    }
    if (name === "refresh") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.3-5.7"></path><path d="M20 4.5v5.5h-5.5"></path></svg>';
    }
    if (name === "new-folder") {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.5 7.5h5.2l1.8 2H20.5v7A2.5 2.5 0 0 1 18 19H6A2.5 2.5 0 0 1 3.5 16.5Z"></path><path d="M12 11v4"></path><path d="M10 13h4"></path></svg>';
    }
    return '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="6"></circle></svg>';
  }

  function renderBadge(row) {
    const info = typeMap[row.kind] || typeMap.default;
    if (row.kind === "folder") {
      return '<span class="file-badge folder">' + iconSvg("folder") + "</span>";
    }
    return '<span class="file-badge ext" data-tone="' + info.accent + '">' + escapeHtml(info.label) + "</span>";
  }

  function renderBreadcrumbs() {
    return data.breadcrumbs
      .map(function (crumb, index) {
        const active = index === data.breadcrumbs.length - 1 ? " active" : "";
        const divider = index < data.breadcrumbs.length - 1 ? '<span class="breadcrumb-divider"></span>' : "";
        return '<span class="breadcrumb-item' + active + '"><span>' + escapeHtml(crumb) + "</span>" + divider + "</span>";
      })
      .join("");
  }

  function renderSummaryChips() {
    return data.summaryChips
      .map(function (label, index) {
        const cls = index === 0 ? "chip strong" : "chip";
        return '<span class="' + cls + '">' + escapeHtml(label) + "</span>";
      })
      .join("");
  }

  function renderActions(mode) {
    const accents = mode || {};
    return data.actions
      .map(function (action) {
        const tone = accents[action.id] || action.tone;
        const iconName =
          action.id === "upload-file" || action.id === "upload-folder"
            ? "upload"
            : action.id === "new-folder"
              ? "new-folder"
              : "refresh";
        return '<button class="action-button ' + tone + '">' + iconSvg(iconName) + '<span>' + escapeHtml(action.label) + "</span></button>";
      })
      .join("");
  }

  function renderSearch(prominent) {
    const shellClass = prominent ? "search-shell prominent panel soft" : "search-shell panel";
    const fieldClass = prominent ? "search-field prominent" : "search-field";
    return (
      '<div class="' + shellClass + '">' +
        '<div class="scope-switch">' +
          '<span class="scope-label">搜索范围</span>' +
          '<span class="scope-option active">' + escapeHtml(data.searchScope[0]) + "</span>" +
          '<span class="scope-option">' + escapeHtml(data.searchScope[1]) + "</span>" +
        "</div>" +
        '<div class="' + fieldClass + '">' +
          iconSvg("search") +
          '<input value="" placeholder="' + escapeHtml(data.searchPlaceholder) + '" />' +
          '<button class="submit-button">搜索</button>' +
        "</div>" +
      "</div>"
    );
  }

  function renderFlashBanner() {
    return (
      '<div class="message-banner">' +
        "<div>" +
          "<strong>" + escapeHtml(data.flashTitle) + "</strong>" +
          '<div><span>' + escapeHtml(data.flashText) + "</span></div>" +
        "</div>" +
        '<span class="pill accent">状态已更新</span>' +
      "</div>"
    );
  }

  function renderRows() {
    return data.rows
      .map(function (row) {
        return (
          '<div class="table-row">' +
            '<div class="name-cell">' +
              renderBadge(row) +
              '<div class="file-copy">' +
                '<div class="file-title">' + escapeHtml(row.name) + "</div>" +
                '<div class="file-subtitle">' + escapeHtml(row.subtitle) + "</div>" +
              "</div>" +
            "</div>" +
            '<div class="meta-cell" data-label="规模">' + escapeHtml(row.size) + "</div>" +
            '<div class="meta-cell" data-label="更新">' + escapeHtml(row.modified) + "</div>" +
            '<div class="action-cell">' +
              '<button class="icon-button" title="预览">' + iconSvg("preview") + "</button>" +
              '<button class="icon-button" title="下载">' + iconSvg("download") + "</button>" +
              '<button class="icon-button" title="重命名">' + iconSvg("rename") + "</button>" +
              '<button class="icon-button" title="删除">' + iconSvg("delete") + "</button>" +
            "</div>" +
          "</div>"
        );
      })
      .join("");
  }

  function renderTable(title, note) {
    return (
      '<section class="panel table-shell">' +
        '<div class="canvas-inner" style="padding:0">' +
          '<div style="padding:18px 20px 0">' +
            '<div class="meta-row">' +
              '<div>' +
                '<div class="eyebrow">Current Directory</div>' +
                '<h2 class="section-title" style="font-size:28px;margin-top:10px">' + escapeHtml(title) + "</h2>" +
              "</div>" +
              '<span class="pill">' + escapeHtml(note) + "</span>" +
            "</div>" +
          "</div>" +
          '<div class="table-header">' +
            "<span>名称</span><span>规模</span><span>最近更新</span><span style=\"text-align:right\">操作</span>" +
          "</div>" +
          renderRows() +
        "</div>" +
      "</section>"
    );
  }

  function renderFooter() {
    return (
      '<div class="footer-note">' +
        "<span>" + escapeHtml(data.footerLeft) + "</span>" +
        "<span>" + escapeHtml(data.footerRight) + "</span>" +
      "</div>"
    );
  }

  window.fileShareHomepageMock = {
    data: data,
    iconSvg: iconSvg,
    renderActions: renderActions,
    renderBreadcrumbs: renderBreadcrumbs,
    renderSummaryChips: renderSummaryChips,
    renderSearch: renderSearch,
    renderFlashBanner: renderFlashBanner,
    renderTable: renderTable,
    renderFooter: renderFooter,
    escapeHtml: escapeHtml,
  };
})();
