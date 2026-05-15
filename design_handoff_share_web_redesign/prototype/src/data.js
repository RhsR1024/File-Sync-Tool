// Mock file-share data — Chinese-language LAN file share scenario.

window.FS_DEVICE = {
  hostname: "DESKTOP-JKKITHV",
  mac: "F4:F1:8E:5E:C1:60",
  ip: "192.168.31.42",
  os: "Windows 11",
  online: true,
  uptime: "已运行 3 天 14 小时",
};

window.FS_USERS = {
  guest: {
    username: "guest",
    label: "访客",
    is_guest: true,
    role: "guest",
    permissions: {
      browse: true,
      download: true,
      preview: true,
      search_current: true,
      search_global: true,
      upload: false,
      create_dir: false,
      create_text: false,
      rename: false,
      delete: false,
    },
  },
  admin: {
    username: "yuxin.li",
    label: "管理员",
    is_guest: false,
    role: "admin",
    permissions: {
      browse: true,
      download: true,
      preview: true,
      search_current: true,
      search_global: true,
      upload: true,
      create_dir: true,
      create_text: true,
      rename: true,
      delete: true,
    },
  },
};

// Each path level: array of entries
window.FS_TREE = {
  "/": [
    { id: "ums",  name: "UMS_TEMP",       is_dir: true,  size: null, modified: "2026-05-12 10:37", count: 18, hint: "构建产物 · 临时区", pinned: true },
    { id: "rls",  name: "Releases",       is_dir: true,  size: null, modified: "2026-05-10 16:21", count: 7,  hint: "对外发布版本" },
    { id: "doc",  name: "Documents",      is_dir: true,  size: null, modified: "2026-05-09 11:02", count: 24, hint: "团队文档归档" },
    { id: "mda",  name: "Media",          is_dir: true,  size: null, modified: "2026-05-08 22:14", count: 132 },
    { id: "snd",  name: "_sandbox",       is_dir: true,  size: null, modified: "2026-05-08 09:30", count: 3 },
    { id: "rd1",  name: "readme.txt",     is_dir: false, size: 2_410,          modified: "2026-05-12 09:11", ext: "txt" },
    { id: "rd2",  name: "网络部署清单.md",  is_dir: false, size: 8_733,          modified: "2026-05-10 19:45", ext: "md" },
  ],
  "/UMS_TEMP": [
    { id: "f-1", name: "2026_05_12_09_55(1.3.7.P25.H19)", is_dir: true, size: null, modified: "2026-05-12 10:37", count: 4, hint: "最新构建" },
    { id: "f-2", name: "2026_04_30_12_30(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-30 13:52", count: 4 },
    { id: "f-3", name: "2026_04_30_08_52(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-30 09:40", count: 4 },
    { id: "f-4", name: "2026_04_29_18_13(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-30 09:24", count: 4 },
    { id: "f-5", name: "2026_04_29_14_54(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-29 17:30", count: 4 },
    { id: "f-6", name: "2026_04_29_09_03(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-29 10:47", count: 4 },
    { id: "f-7", name: "2026_04_24_12_59(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-24 13:53", count: 4 },
    { id: "f-8", name: "2026_04_23_14_16(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-23 14:49", count: 4 },
    { id: "f-9", name: "2026_04_20_11_33(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-20 13:36", count: 4 },
    { id: "f-10", name: "2026_04_20_09_21(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-20 11:01", count: 4 },
    { id: "f-11", name: "2026_04_17_10_54(1.3.7.P02.L24)", is_dir: true, size: null, modified: "2026-04-20 10:59", count: 4 },
    { id: "x-1", name: "构建日志.log",            is_dir: false, size: 184_320, modified: "2026-05-12 10:38", ext: "log" },
    { id: "x-2", name: "checksum.sha256",          is_dir: false, size: 1_024,   modified: "2026-05-12 10:38", ext: "txt" },
  ],
  "/UMS_TEMP/2026_05_12_09_55(1.3.7.P25.H19)": [
    { id: "b-1", name: "UMS-installer-1.3.7.P25.H19.exe", is_dir: false, size: 78_201_344, modified: "2026-05-12 10:37", ext: "exe" },
    { id: "b-2", name: "release-notes.md",                is_dir: false, size: 4_822,      modified: "2026-05-12 10:37", ext: "md" },
    { id: "b-3", name: "manifest.json",                   is_dir: false, size: 3_120,      modified: "2026-05-12 10:37", ext: "json" },
    { id: "b-4", name: "screenshots",                     is_dir: true,  size: null,       modified: "2026-05-12 10:37", count: 6 },
  ],
  "/Documents": [
    { id: "d-1", name: "工程师入职手册.pdf",   is_dir: false, size: 2_457_600,  modified: "2026-05-09 11:02", ext: "pdf" },
    { id: "d-2", name: "Q2_OKR.xlsx",         is_dir: false, size: 184_320,    modified: "2026-05-08 14:30", ext: "xlsx" },
    { id: "d-3", name: "架构评审.pptx",        is_dir: false, size: 5_120_000,  modified: "2026-05-07 09:15", ext: "pptx" },
    { id: "d-4", name: "Specs",                is_dir: true,  size: null,       modified: "2026-05-06 17:22", count: 12 },
    { id: "d-5", name: "Meeting Notes",        is_dir: true,  size: null,       modified: "2026-05-05 14:00", count: 28 },
    { id: "d-6", name: "competitor-analysis.docx", is_dir: false, size: 96_768, modified: "2026-05-04 11:33", ext: "docx" },
  ],
  "/Media": [
    { id: "m-1", name: "team-photo-2026.jpg",      is_dir: false, size: 4_096_000, modified: "2026-05-08 22:14", ext: "jpg" },
    { id: "m-2", name: "product-demo.mp4",         is_dir: false, size: 188_743_680, modified: "2026-05-07 18:30", ext: "mp4" },
    { id: "m-3", name: "intro-music.mp3",          is_dir: false, size: 5_242_880, modified: "2026-05-06 12:00", ext: "mp3" },
    { id: "m-4", name: "logo.svg",                  is_dir: false, size: 8_192, modified: "2026-05-06 12:00", ext: "svg" },
  ],
  "/Releases": [
    { id: "r-1", name: "v1.3.7",   is_dir: true,  size: null,       modified: "2026-05-10 16:21", count: 8 },
    { id: "r-2", name: "v1.3.6",   is_dir: true,  size: null,       modified: "2026-04-28 14:00", count: 8 },
    { id: "r-3", name: "v1.3.5",   is_dir: true,  size: null,       modified: "2026-04-15 09:11", count: 8 },
    { id: "r-4", name: "CHANGELOG.md", is_dir: false, size: 18_432, modified: "2026-05-10 16:22", ext: "md" },
  ],
  "/_sandbox": [
    { id: "s-1", name: "scratch.txt",   is_dir: false, size: 412,    modified: "2026-05-08 09:30", ext: "txt" },
    { id: "s-2", name: "experiment.py", is_dir: false, size: 6_144,  modified: "2026-05-07 22:15", ext: "py" },
    { id: "s-3", name: "out.csv",       is_dir: false, size: 51_200, modified: "2026-05-06 18:00", ext: "csv" },
  ],
};

// Quick links shown in sidebar.
window.FS_QUICKLINKS = [
  { id: "ums",  label: "UMS_TEMP",   path: "/UMS_TEMP",  hint: "构建产物" },
  { id: "rls",  label: "Releases",    path: "/Releases",  hint: "发布版本" },
  { id: "doc",  label: "Documents",   path: "/Documents", hint: "团队文档" },
];

// Storage snapshot
window.FS_STORAGE = {
  used_gb: 184.2,
  total_gb: 512,
  shared_count: 1238,
  today_downloads: 47,
};
