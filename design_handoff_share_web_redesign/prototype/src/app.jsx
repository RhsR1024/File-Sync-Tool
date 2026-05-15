/* eslint-disable */
// File Share — main app (Chinese LAN file sharing redesign)

const { useState, useMemo, useEffect, useCallback } = React;

// ---------------------------------------------------------------------------
// Tweak defaults (rewritten on disk by host)
// ---------------------------------------------------------------------------
const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "light",
  "accent": "jade",
  "density": "cozy",
  "view": "list",
  "role": "guest",
  "showSidebar": true,
  "showStorage": true
}/*EDITMODE-END*/;

const ACCENT_HUES = { jade: 175, cobalt: 240, amber: 70, plum: 320, slate: 240 };
const ACCENT_LABELS = { jade: "茶绿", cobalt: "宝石蓝", amber: "琥珀", plum: "梅紫", slate: "石墨" };

// ---------------------------------------------------------------------------
// Icons (lightweight inline SVGs)
// ---------------------------------------------------------------------------
const Icon = ({ name, ...props }) => {
  const paths = {
    folder: <path d="M3 6.5C3 5.67 3.67 5 4.5 5h4.79c.4 0 .78.16 1.06.44L11.7 6.79c.28.28.66.44 1.06.44h6.74c.83 0 1.5.67 1.5 1.5v9.77c0 .83-.67 1.5-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5V6.5Z" fill="currentColor"/>,
    download: <path d="M12 4v10m0 0 4-4m-4 4-4-4M5 18.5h14" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" fill="none"/>,
    preview: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6S2.5 12 2.5 12Z"/><circle cx="12" cy="12" r="3"/></g>,
    edit: <path d="M14.5 5 19 9.5 8.5 20H4v-4.5L14.5 5Z" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round"/>,
    trash: <path d="M5 7h14M9 7V5.5A1.5 1.5 0 0 1 10.5 4h3A1.5 1.5 0 0 1 15 5.5V7m-6 4v6m6-6v6M6 7h12l-1 12.5A1.5 1.5 0 0 1 15.5 21h-7A1.5 1.5 0 0 1 7 19.5L6 7Z" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    more: <g fill="currentColor"><circle cx="6" cy="12" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="18" cy="12" r="1.6"/></g>,
    search: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4-4"/></g>,
    refresh: <path d="M4 12a8 8 0 0 1 13.7-5.6L20 9M20 4v5h-5M20 12a8 8 0 0 1-13.7 5.6L4 15m0 5v-5h5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    upload: <path d="M12 20V8m0 0-4 4m4-4 4 4M5 5.5h14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    newfolder: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M3 7c0-.83.67-1.5 1.5-1.5h4.79l2.13 2.13H19.5c.83 0 1.5.67 1.5 1.5v9.37c0 .83-.67 1.5-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5V7Z"/><path d="M12 11v6m3-3h-6"/></g>,
    text: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M6 4h8l4 4v12a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 5 20V5.5A1.5 1.5 0 0 1 6 4Z"/><path d="M14 4v4h4M8 13h8M8 17h5"/></g>,
    home: <path d="m3.5 11 8.5-7 8.5 7M5.5 9.5V20h5v-5h3v5h5V9.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    chevron: <path d="m9 6 6 6-6 6" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>,
    check: <path d="m5 11 4 4 10-10" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"/>,
    list: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M8 6h12M8 12h12M8 18h12"/><circle cx="4" cy="6" r="1" fill="currentColor"/><circle cx="4" cy="12" r="1" fill="currentColor"/><circle cx="4" cy="18" r="1" fill="currentColor"/></g>,
    grid: <g fill="none" stroke="currentColor" strokeWidth="1.8"><rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/></g>,
    info: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 11v5m0-8.5v.01"/></g>,
    close: <path d="m6 6 12 12M18 6 6 18" stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>,
    switch: <path d="M4 8h12l-3-3m3 3-3 3M20 16H8l3 3m-3-3 3-3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    sortAsc: <path d="M7 4v16m0 0-3-3m3 3 3-3M13 7h7M13 12h5M13 17h3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/>,
    eye: <g fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6S2.5 12 2.5 12Z"/><circle cx="12" cy="12" r="2.5"/></g>,
    clock: <g fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="12" cy="12" r="8"/><path d="M12 8v4l3 2"/></g>,
    star: <path d="m12 4 2.5 5 5.5.8-4 3.9.9 5.5L12 16.7 7.1 19.2 8 13.7 4 9.8 9.5 9 12 4Z" fill="currentColor"/>,
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" {...props}>{paths[name]}</svg>
  );
};

// ---------------------------------------------------------------------------
// File-type styling
// ---------------------------------------------------------------------------
const EXT_STYLES = {
  pdf:  { hue: 25,  label: "PDF" },
  doc:  { hue: 240, label: "DOC" }, docx: { hue: 240, label: "DOC" },
  xls:  { hue: 150, label: "XLS" }, xlsx: { hue: 150, label: "XLS" }, csv: { hue: 150, label: "CSV" },
  ppt:  { hue: 30,  label: "PPT" }, pptx: { hue: 30,  label: "PPT" },
  zip:  { hue: 290, label: "ZIP" }, rar: { hue: 290, label: "RAR" }, "7z": { hue: 290, label: "7Z" },
  tar:  { hue: 290, label: "TAR" }, gz:  { hue: 290, label: "GZ" },
  txt:  { hue: 240, label: "TXT" }, log: { hue: 240, label: "LOG" }, md: { hue: 240, label: "MD" },
  json: { hue: 180, label: "JSON" }, xml: { hue: 180, label: "XML" }, yml: { hue: 180, label: "YML" },
  yaml: { hue: 180, label: "YML" }, toml: { hue: 180, label: "TOML" }, ini: { hue: 180, label: "INI" },
  html: { hue: 35,  label: "HTML" }, css: { hue: 240, label: "CSS" },
  js:   { hue: 80,  label: "JS" }, ts: { hue: 240, label: "TS" }, vue: { hue: 155, label: "VUE" },
  py:   { hue: 240, label: "PY" }, rs: { hue: 25, label: "RS" }, go: { hue: 195, label: "GO" },
  exe:  { hue: 350, label: "EXE" }, msi: { hue: 350, label: "MSI" },
  jpg:  { hue: 210, label: "JPG" }, jpeg: { hue: 210, label: "JPG" }, png: { hue: 210, label: "PNG" },
  gif:  { hue: 210, label: "GIF" }, svg: { hue: 210, label: "SVG" }, webp: { hue: 210, label: "WEBP" },
  mp4:  { hue: 290, label: "MP4" }, mov: { hue: 290, label: "MOV" }, mkv: { hue: 290, label: "MKV" },
  mp3:  { hue: 350, label: "MP3" }, wav: { hue: 350, label: "WAV" }, flac: { hue: 350, label: "FLAC" },
  iso:  { hue: 240, label: "ISO" }, dmg: { hue: 240, label: "DMG" },
  sql:  { hue: 180, label: "SQL" },
};

function getExtStyle(name) {
  const lower = (name || "").toLowerCase();
  const dot = lower.lastIndexOf(".");
  const ext = dot >= 0 ? lower.slice(dot + 1) : "";
  const s = EXT_STYLES[ext] || { hue: 240, label: ext ? ext.slice(0, 4).toUpperCase() : "FILE" };
  return {
    label: s.label,
    color: `oklch(0.42 0.10 ${s.hue})`,
    bg: `oklch(0.96 0.025 ${s.hue})`,
    border: `oklch(0.88 0.05 ${s.hue})`,
  };
}

function formatSize(bytes) {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

function timeAgo(modified) {
  // crude relative — based on May 14 2026
  const now = new Date("2026-05-14T12:00:00");
  const d = new Date(modified.replace(" ", "T"));
  const diff = Math.max(0, (now - d) / 1000);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)} 天前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / 86400 / 30)} 个月前`;
  return `${Math.floor(diff / 86400 / 365)} 年前`;
}

// ---------------------------------------------------------------------------
// Glyph (folder / file-ext badge)
// ---------------------------------------------------------------------------
const Glyph = ({ entry, size = "row" }) => {
  if (entry.is_dir) {
    const cls = size === "tile" ? "tile-glyph folder" : "glyph folder";
    return (
      <div className={cls}>
        <Icon name="folder" />
      </div>
    );
  }
  const s = getExtStyle(entry.name);
  const style = { color: s.color, background: s.bg, borderColor: s.border };
  if (size === "tile") {
    return <div className="tile-glyph" style={style}>{s.label}</div>;
  }
  return <div className="glyph ext" style={style}>{s.label}</div>;
};

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------
const TopBar = ({ user, onRefresh, onSwitchAccount, onToggleRole }) => {
  const device = window.FS_DEVICE;
  return (
    <header className="topbar">
      <div className="brand">
        <div className="brand-mark" aria-hidden="true">FS</div>
        <div>
          <div className="brand-name">File Share</div>
          <div className="brand-sub">{device.ip}:8421</div>
        </div>
      </div>

      <div className="device-chip" title={`${device.mac} · ${device.os}`}>
        <span className="dot" />
        <span className="host">{device.hostname}</span>
        <span className="meta">· {device.uptime}</span>
      </div>

      <div className="topbar-spacer" />

      <button className="btn ghost" onClick={onRefresh} title="刷新">
        <Icon name="refresh" />
        <span>刷新</span>
      </button>

      <div className="user-chip">
        <div className="avatar">{user.username.slice(0, 1).toUpperCase()}</div>
        <div className="who">
          <div className="name">{user.username}</div>
          <div className="role">{user.is_guest ? "guest · 访客" : user.label}</div>
        </div>
      </div>

      <button className="btn" onClick={onSwitchAccount}>
        <Icon name="switch" />
        <span>切换账号</span>
      </button>
    </header>
  );
};

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------
const Sidebar = ({ path, onNavigate, storage, showStorage }) => {
  const links = window.FS_QUICKLINKS;
  const isHome = path === "/";
  return (
    <aside className="sidebar">
      <div className="side-section">
        <button
          className={`side-item ${isHome ? "active" : ""}`}
          onClick={() => onNavigate("/")}
        >
          <span className="ico"><Icon name="home" /></span>
          <span>首页</span>
        </button>
      </div>

      <div className="side-section">
        <div className="side-title">共享目录</div>
        {links.map((l) => {
          const active = path === l.path || path.startsWith(l.path + "/");
          const tree = window.FS_TREE[l.path] || [];
          return (
            <button
              key={l.id}
              className={`side-item ${active ? "active" : ""}`}
              onClick={() => onNavigate(l.path)}
            >
              <span className="ico"><Icon name="folder" /></span>
              <span>{l.label}</span>
              <span className="count">{tree.length}</span>
            </button>
          );
        })}
      </div>

      <div className="side-section">
        <div className="side-title">最近</div>
        <button className="side-item" onClick={() => onNavigate("/UMS_TEMP")}>
          <span className="ico"><Icon name="clock" /></span>
          <span>2026_05_12 构建</span>
        </button>
        <button className="side-item" onClick={() => onNavigate("/Documents")}>
          <span className="ico"><Icon name="text" /></span>
          <span>Q2_OKR.xlsx</span>
        </button>
      </div>

      {showStorage && (
        <div className="storage-card">
          <div className="storage-row">
            <span className="label">设备存储</span>
            <span className="value">{storage.used_gb} / {storage.total_gb} GB</span>
          </div>
          <div className="storage-bar">
            <div style={{ width: `${(storage.used_gb / storage.total_gb) * 100}%` }} />
          </div>
          <div className="storage-stats">
            <div className="storage-stat">
              <span className="n">{storage.shared_count.toLocaleString()}</span>
              <span className="k">共享文件</span>
            </div>
            <div className="storage-stat">
              <span className="n">{storage.today_downloads}</span>
              <span className="k">今日下载</span>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
};

// ---------------------------------------------------------------------------
// Breadcrumbs
// ---------------------------------------------------------------------------
const Crumbs = ({ path, onNavigate }) => {
  const segs = path === "/" ? [] : path.split("/").filter(Boolean);
  return (
    <nav className="crumbs">
      <button onClick={() => onNavigate("/")}>
        <Icon name="home" style={{ width: 13, height: 13, verticalAlign: "-1px", marginRight: 4 }} />
        首页
      </button>
      {segs.map((s, i) => {
        const subPath = "/" + segs.slice(0, i + 1).join("/");
        const last = i === segs.length - 1;
        return (
          <React.Fragment key={subPath}>
            <span className="sep">/</span>
            {last
              ? <span className="last">{s}</span>
              : <button onClick={() => onNavigate(subPath)}>{s}</button>}
          </React.Fragment>
        );
      })}
    </nav>
  );
};

// ---------------------------------------------------------------------------
// Toolbar (search + scope + view switch)
// ---------------------------------------------------------------------------
const Toolbar = ({ query, setQuery, scope, setScope, view, setView, canScopeCurrent }) => (
  <div className="toolbar">
    <div className="search">
      <Icon name="search" />
      <input
        placeholder="搜索文件名、扩展名…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <kbd>⌘K</kbd>
    </div>
    <div className="scope-toggle" role="tablist" aria-label="搜索范围">
      <button
        className={scope === "current" ? "active" : ""}
        onClick={() => setScope("current")}
        disabled={!canScopeCurrent}
        title={canScopeCurrent ? "在当前目录内搜索" : "首页下不可用"}
        style={!canScopeCurrent ? { opacity: 0.4, cursor: "not-allowed" } : {}}
      >当前目录</button>
      <button
        className={scope === "global" ? "active" : ""}
        onClick={() => setScope("global")}
      >全部共享</button>
    </div>
    <div className="view-toggle" role="tablist" aria-label="视图">
      <button className={view === "list" ? "active" : ""} onClick={() => setView("list")} title="列表"><Icon name="list" /></button>
      <button className={view === "grid" ? "active" : ""} onClick={() => setView("grid")} title="网格"><Icon name="grid" /></button>
    </div>
  </div>
);

// ---------------------------------------------------------------------------
// File row (list view)
// ---------------------------------------------------------------------------
const FileRow = ({ entry, selected, onSelect, onOpen, onDownload, perms }) => {
  return (
    <div className={`row ${selected ? "selected" : ""}`}>
      <button
        className={`check ${selected ? "checked" : ""}`}
        onClick={(e) => { e.stopPropagation(); onSelect(entry.id); }}
        aria-label="选择"
      >
        <Icon name="check" />
      </button>
      <div className="name-cell" onClick={() => onOpen(entry)} style={{ cursor: "pointer" }}>
        <Glyph entry={entry} />
        <div className="name-text">
          <div className="n">
            <span>{entry.name}</span>
            {entry.pinned && <span className="pin">PINNED</span>}
          </div>
          <div className={`hint ${/[\u4e00-\u9fa5]/.test(entry.hint || "") ? "cn" : ""}`}>
            {entry.is_dir
              ? (entry.hint || `${entry.count ?? 0} 项`)
              : (entry.hint || getExtStyle(entry.name).label)}
          </div>
        </div>
      </div>
      <span className="cell-size">{entry.is_dir ? `${entry.count ?? 0} 项` : formatSize(entry.size)}</span>
      <span className="cell-modified">
        {entry.modified}
        <span className="ago">{timeAgo(entry.modified)}</span>
      </span>
      <div className="cell-actions">
        {!entry.is_dir && perms.preview && (
          <button className="row-action" title="预览"><Icon name="preview" /></button>
        )}
        {perms.download && (
          <button className="row-action primary" title="下载" onClick={(e) => { e.stopPropagation(); onDownload(entry); }}>
            <Icon name="download" />
          </button>
        )}
        {perms.rename && (
          <button className="row-action" title="重命名"><Icon name="edit" /></button>
        )}
        {perms.delete && (
          <button className="row-action danger" title="删除"><Icon name="trash" /></button>
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// File tile (grid view)
// ---------------------------------------------------------------------------
const FileTile = ({ entry, selected, onSelect, onOpen, onDownload, perms }) => {
  return (
    <div
      className={`tile ${selected ? "selected" : ""}`}
      onClick={(e) => { if (e.shiftKey) { onSelect(entry.id); } else { onOpen(entry); } }}
    >
      <Glyph entry={entry} size="tile" />
      <div className="tile-name">{entry.name}</div>
      <div className="tile-meta">
        <span>{entry.is_dir ? `${entry.count ?? 0} 项` : formatSize(entry.size)}</span>
        <span>{timeAgo(entry.modified)}</span>
      </div>
      {perms.download && (
        <button
          className="row-action primary"
          onClick={(e) => { e.stopPropagation(); onDownload(entry); }}
          title="下载"
        >
          <Icon name="download" />
        </button>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main App
// ---------------------------------------------------------------------------
function App() {
  const { TweaksPanel, TweakSection, TweakRadio, TweakSelect, TweakToggle } = window;
  const [tweaks, setTweak] = window.useTweaks(TWEAK_DEFAULTS);

  // Apply theme + density + accent via root attrs/vars
  useEffect(() => {
    document.documentElement.dataset.theme = tweaks.theme;
    document.documentElement.dataset.density = tweaks.density;
    document.documentElement.style.setProperty("--accent-h", ACCENT_HUES[tweaks.accent] || 175);
    window.__setTweak = setTweak; // debug: enable screenshot scripting
  }, [tweaks.theme, tweaks.density, tweaks.accent]);

  // App state
  const [path, setPath] = useState("/UMS_TEMP");
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState("current"); // current | global
  const [selected, setSelected] = useState(new Set());
  const [flash, setFlash] = useState("");
  const [noticeOpen, setNoticeOpen] = useState(true);

  const user = window.FS_USERS[tweaks.role];
  const perms = user.permissions;
  const view = tweaks.view;

  // Reset selection when path/scope/query changes
  useEffect(() => { setSelected(new Set()); }, [path, scope, query]);

  // Compute entries
  const entries = useMemo(() => {
    if (query) {
      const q = query.toLowerCase();
      if (scope === "global") {
        // search across whole tree
        const all = [];
        Object.entries(window.FS_TREE).forEach(([p, arr]) => {
          arr.forEach((e) => {
            if (e.name.toLowerCase().includes(q)) {
              all.push({ ...e, _path: p });
            }
          });
        });
        return all;
      }
      return (window.FS_TREE[path] || []).filter((e) => e.name.toLowerCase().includes(q));
    }
    return window.FS_TREE[path] || [];
  }, [path, query, scope]);

  // Auto-switch scope when navigating to home
  useEffect(() => {
    if (path === "/" && scope === "current") setScope("global");
  }, [path]); // eslint-disable-line

  const canScopeCurrent = path !== "/";

  function onNavigate(p) {
    setPath(p);
    setQuery("");
  }

  function onOpen(entry) {
    if (entry.is_dir) {
      const next = path === "/" ? "/" + entry.name : path + "/" + entry.name;
      // only navigate if data exists, else simulate
      if (window.FS_TREE[next]) onNavigate(next);
      else {
        setFlash(`已打开「${entry.name}」`);
      }
    } else {
      if (perms.preview) setFlash(`预览：${entry.name}`);
    }
  }

  function onDownload(entry) {
    setFlash(`已开始下载 · ${entry.name}`);
  }

  function toggleSelect(id) {
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }

  function selectAll() {
    if (selected.size === entries.length) setSelected(new Set());
    else setSelected(new Set(entries.map((e) => e.id)));
  }

  function bulkDownload() {
    setFlash(`已开始下载 ${selected.size} 项`);
    setSelected(new Set());
  }

  function onRefresh() {
    setFlash("已刷新");
  }

  function onSwitchAccount() {
    setTweak("role", tweaks.role === "guest" ? "admin" : "guest");
    setFlash(tweaks.role === "guest" ? "已切换为管理员视图" : "已切换为访客视图");
  }

  // Flash auto-dismiss
  useEffect(() => {
    if (!flash) return;
    const id = setTimeout(() => setFlash(""), 1800);
    return () => clearTimeout(id);
  }, [flash]);

  // Page title detail
  const pageName = path === "/" ? "首页" : path.split("/").filter(Boolean).slice(-1)[0];
  const pageHint = useMemo(() => {
    if (path === "/") return "全部共享目录与文件";
    const entry = window.FS_TREE["/"]?.find((e) => path === "/" + e.name) ||
                  Object.values(window.FS_TREE).flat().find((e) => e.is_dir && path.endsWith("/" + e.name));
    if (entry?.hint) return entry.hint;
    return `${entries.length} 项 · 最近更新 ${timeAgo(entries[0]?.modified || "2026-05-12 10:37")}`;
  }, [path, entries]);

  const showBrowseOnly = user.is_guest && noticeOpen && (path !== "/" || true);

  // Sort indicator (mock)
  const sortLabel = "修改时间 · 最近优先";

  return (
    <div className="app" data-screen-label="01 File browser">
      <TopBar
        user={user}
        onRefresh={onRefresh}
        onSwitchAccount={onSwitchAccount}
      />
      {tweaks.showSidebar && (
        <Sidebar
          path={path}
          onNavigate={onNavigate}
          storage={window.FS_STORAGE}
          showStorage={tweaks.showStorage}
        />
      )}
      <main className="main" style={!tweaks.showSidebar ? { gridColumn: "1 / -1" } : undefined}>
        <Crumbs path={path} onNavigate={onNavigate} />

        <div className="page-head">
          <div>
            <h1 className="page-title">
              {pageName}
              {path !== "/" && <span className="sub">{entries.filter(e => e.is_dir).length} 文件夹 · {entries.filter(e => !e.is_dir).length} 文件</span>}
            </h1>
            <div className="page-sub">{pageHint}</div>
          </div>
          <div className="page-actions">
            {perms.upload && (
              <button className="btn"><Icon name="upload" /><span>上传</span></button>
            )}
            {perms.create_dir && (
              <button className="btn"><Icon name="newfolder" /><span>新建文件夹</span></button>
            )}
            {perms.create_text && (
              <button className="btn"><Icon name="text" /><span>新建文本</span></button>
            )}
            {!perms.upload && (
              <button className="btn primary" onClick={() => entries[0] && onDownload(entries[0])}>
                <Icon name="download" /><span>下载全部</span>
              </button>
            )}
          </div>
        </div>

        {showBrowseOnly && (
          <div className="notice">
            <span className="ico"><Icon name="info" /></span>
            <span>
              {user.is_guest
                ? "当前为访客模式，仅可浏览和下载。需要管理操作请通过右上角切换账号。"
                : `已登录为 ${user.label} · ${user.username}`}
            </span>
            <button className="close" onClick={() => setNoticeOpen(false)} aria-label="关闭">关闭</button>
          </div>
        )}

        <Toolbar
          query={query} setQuery={setQuery}
          scope={scope} setScope={setScope}
          view={view} setView={(v) => setTweak("view", v)}
          canScopeCurrent={canScopeCurrent}
        />

        <div className="list-card" style={view === "grid" ? { background: "transparent", border: 0, boxShadow: "none" } : undefined}>
          {view === "list" && (
            <>
              <div className="list-meta">
                <span><span className="count">{entries.length}</span> 项</span>
                <span className="sep">·</span>
                <span>{entries.filter(e => e.is_dir).length} 文件夹 · {entries.filter(e => !e.is_dir).length} 文件</span>
                {query && <><span className="sep">·</span><span>"{query}" 在 {scope === "global" ? "全部共享" : "当前目录"} 中</span></>}
                <div className="sort">
                  <button className="sort-btn" title={sortLabel}>
                    <Icon name="sortAsc" /> {sortLabel}
                  </button>
                </div>
              </div>

              <div className="list-head">
                <button
                  className={`check ${selected.size > 0 && selected.size === entries.length ? "checked" : ""}`}
                  onClick={selectAll}
                  aria-label="全选"
                >
                  <Icon name="check" />
                </button>
                <span>名称</span>
                <span>大小</span>
                <span>修改时间</span>
                <span className="col-actions">操作</span>
              </div>

              {entries.length === 0 ? (
                <div className="empty">
                  <div className="icon"><Icon name="search" /></div>
                  <div className="title">没有找到匹配的文件</div>
                  <div className="sub">
                    {query
                      ? `「${query}」在${scope === "global" ? "全部共享" : "当前目录"}中没有结果。试着调整范围或关键词。`
                      : "这个目录是空的。"}
                  </div>
                </div>
              ) : entries.map((entry) => (
                <FileRow
                  key={entry.id + (entry._path || "")}
                  entry={entry}
                  selected={selected.has(entry.id)}
                  onSelect={toggleSelect}
                  onOpen={onOpen}
                  onDownload={onDownload}
                  perms={perms}
                />
              ))}
            </>
          )}

          {view === "grid" && (
            entries.length === 0 ? (
              <div className="empty" style={{ gridColumn: "1 / -1" }}>
                <div className="icon"><Icon name="search" /></div>
                <div className="title">没有找到匹配的文件</div>
              </div>
            ) : (
              <div className="grid-card">
                {entries.map((entry) => (
                  <FileTile
                    key={entry.id + (entry._path || "")}
                    entry={entry}
                    selected={selected.has(entry.id)}
                    onSelect={toggleSelect}
                    onOpen={onOpen}
                    onDownload={onDownload}
                    perms={perms}
                  />
                ))}
              </div>
            )
          )}
        </div>

        {selected.size > 0 && (
          <div className="bulkbar">
            <div className="count">
              <span className="pill">{selected.size}</span>
              <span>项已选择</span>
            </div>
            <div className="divider" />
            <button className="primary" onClick={bulkDownload}>
              <Icon name="download" /> 打包下载
            </button>
            {perms.delete && (
              <button className="danger"><Icon name="trash" /> 删除</button>
            )}
            <div className="divider" />
            <button onClick={() => setSelected(new Set())}><Icon name="close" /> 取消</button>
          </div>
        )}
      </main>

      {flash && <div className="flash">{flash}</div>}

      <TweaksPanel title="Tweaks">
        <TweakSection label="外观">
          <TweakRadio label="主题" value={tweaks.theme}
            options={[{ value: "light", label: "明亮" }, { value: "dark", label: "深色" }]}
            onChange={(v) => setTweak("theme", v)} />
          <TweakSelect label="强调色" value={tweaks.accent}
            options={Object.keys(ACCENT_HUES).map(v => ({ value: v, label: ACCENT_LABELS[v] }))}
            onChange={(v) => setTweak("accent", v)} />
          <TweakRadio label="密度" value={tweaks.density}
            options={[{ value: "compact", label: "紧凑" }, { value: "cozy", label: "舒适" }]}
            onChange={(v) => setTweak("density", v)} />
        </TweakSection>
        <TweakSection label="视图">
          <TweakRadio label="文件视图" value={tweaks.view}
            options={[{ value: "list", label: "列表" }, { value: "grid", label: "网格" }]}
            onChange={(v) => setTweak("view", v)} />
          <TweakToggle label="显示侧边栏" value={tweaks.showSidebar} onChange={(v) => setTweak("showSidebar", v)} />
          <TweakToggle label="显示存储卡片" value={tweaks.showStorage} onChange={(v) => setTweak("showStorage", v)} />
        </TweakSection>
        <TweakSection label="账号角色">
          <TweakRadio label="角色" value={tweaks.role}
            options={[{ value: "guest", label: "访客" }, { value: "admin", label: "管理员" }]}
            onChange={(v) => setTweak("role", v)} />
        </TweakSection>
      </TweaksPanel>
    </div>
  );
}

// Mount
ReactDOM.createRoot(document.getElementById("root")).render(<App />);
