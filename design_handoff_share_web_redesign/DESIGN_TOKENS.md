# Design Tokens

所有值采用 **OKLCH** 色彩空间，明暗模式由 `data-theme="light|dark"` 控制（写在 `<html>` 上）。

完整定义在 `prototype/src/styles.css` 的 `:root` 块。本文件是阅读用清单。

---

## 1. 颜色

### 1.1 中性色（light 模式）
| Token | 值 | 用途 |
|---|---|---|
| `--bg` | `oklch(0.985 0.004 200)` | 页面基底 |
| `--bg-2` | `oklch(0.975 0.006 200)` | 次级背景 |
| `--surface` | `oklch(1 0 0)` | 卡片/胶囊背景 |
| `--surface-2` | `oklch(0.985 0.004 200)` | hover/分组头 |
| `--border` | `oklch(0.91 0.006 200)` | 默认描边 |
| `--border-strong` | `oklch(0.84 0.008 200)` | 强描边（复选框等） |
| `--text` | `oklch(0.21 0.02 240)` | 正文 |
| `--text-2` | `oklch(0.42 0.02 240)` | 次级文字 |
| `--muted` | `oklch(0.58 0.012 240)` | meta 信息 |
| `--faint` | `oklch(0.72 0.008 240)` | 分隔符/占位 |

### 1.2 强调色（jade）
| Token | 值 | 用途 |
|---|---|---|
| `--accent` | `oklch(0.58 0.10 175)` | 主按钮、激活态填充 |
| `--accent-ink` | `oklch(0.32 0.07 175)` | jade 软色背景上的深色文字 |
| `--accent-soft` | `oklch(0.95 0.025 175)` | 选中行、激活胶囊背景 |
| `--accent-line` | `oklch(0.85 0.04 175)` | 软色块的描边 |

`--accent-h: 175` 是可调 hue 变量；如保留多主题色，备选 hue：
- cobalt 240 / amber 70 / plum 320 / slate 240

### 1.3 状态色
| 系列 | base | soft | line | 用途 |
|---|---|---|---|---|
| warn | `oklch(0.72 0.13 75)` | `oklch(0.96 0.04 75)` | `oklch(0.88 0.07 75)` | 警告（未广泛使用） |
| danger | `oklch(0.58 0.14 25)` | `oklch(0.96 0.025 25)` | `oklch(0.88 0.05 25)` | 删除按钮、错误 |
| info | `oklch(0.55 0.12 240)` | `oklch(0.96 0.025 240)` | `oklch(0.86 0.04 240)` | 通知条 |
| ok | `oklch(0.60 0.11 155)` | `oklch(0.95 0.03 155)` | `oklch(0.86 0.05 155)` | 在线点、成功 flash |

### 1.4 暗色模式（`:root[data-theme="dark"]`）
所有上述 token 都有对应暗色映射，**保持相对感知亮度一致**。详见 styles.css。要点：
- `--bg` 深至 `oklch(0.16 0.012 240)`
- `--surface` `oklch(0.21 0.014 240)`
- `--accent` 提亮到 `oklch(0.72 0.12 175)`（深底上更易读）

### 1.5 文件类型 hue 表（`EXT_STYLES` in app.jsx）
| 类型 | hue | label |
|---|---|---|
| pdf, rs | 25 | PDF / RS |
| ppt, pptx | 30 | PPT |
| html | 35 | HTML |
| amber 系 | 70 | — |
| js | 80 | JS |
| xls, csv, vue | 150-155 | XLS / CSV / VUE |
| json, xml, yaml, toml, ini, sql | 180 | JSON 等 |
| go | 195 | GO |
| jpg/png/gif/svg/webp | 210 | image 系 |
| doc, txt, log, md, ts, css, py, iso, dmg | 240 | 文档/代码 |
| zip, rar, 7z, tar, gz, mp4, mov, mkv | 290 | 归档/视频 |
| exe, msi, mp3, wav, flac | 350 | 可执行/音频 |

色块生成规则：
```css
color: oklch(0.42 0.10 <hue>);
background: oklch(0.96 0.025 <hue>);
border: oklch(0.88 0.05 <hue>);
```

---

## 2. 字体

| 用途 | 字体栈 |
|---|---|
| 西文/中文/标题/正文 | `"Manrope", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", system-ui, sans-serif` |
| 数字/路径/时间戳/技术信息 | `"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace` |

来自 Google Fonts：
```html
<link href="https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
```

### 字号阶梯
| 元素 | size / weight / letter-spacing |
|---|---|
| H1 标题 | `28px / 700 / -0.02em` |
| 区段小标题 | `11px / 600 / 0.08em` 大写 |
| Body | `13.5-14px / 400-500` |
| Meta / 时间戳 | `11.5-12.5px / mono / 500-600` |
| Ext 徽标（行） | `10px / 700 / mono / uppercase` |
| Ext 徽标（grid） | `18px / 800 / mono / uppercase` |
| KBD（⌘K） | `11px / mono` |

---

## 3. 间距 / 半径

### 半径
| Token | 值 | 用途 |
|---|---|---|
| `--r-sm` | 8px | 复选框、小图标按钮 |
| `--r-md` | 12px | 通用容器（搜索框、卡片） |
| `--r-lg` | 16px | 列表卡片、tile |
| `--r-xl` | 22px | 大胶囊 |
| `999px` | 圆胶囊 | 用户胶囊、设备胶囊、批量栏 |

### 行高
| Token | 值 |
|---|---|
| `--row-h` cozy | 56px（默认） |
| `--row-h` compact | 44px |
| `--row-pad-x` | 18px |
| `--row-pad-y` | 12px / 8px |

### 阴影
| Token | 值 |
|---|---|
| `--shadow-sm` | `0 1px 0 oklch(0.93 0.005 200)` |
| `--shadow-md` | `0 1px 2px oklch(0.55 0.01 240 / 0.04), 0 6px 18px oklch(0.55 0.01 240 / 0.06)` |
| bulkbar | `0 10px 30px oklch(0.20 0.018 240 / 0.30)` |
| flash | `0 10px 30px oklch(0.55 0.01 240 / 0.18)` |

---

## 4. 响应式断点

| 断点 | 行为 |
|---|---|
| `≤ 980px` | 隐藏侧边栏；顶栏按钮收起为图标；隐藏设备时长、用户角色文字、品牌副标题、H1 副胶囊 |
| `≤ 760px` | 列表表头隐藏；行降级为单列；大小/时间列隐藏 |

---

## 5. 关键背景

`.app` 的双径向渐变：
```css
background:
  radial-gradient(1200px 600px at -10% -10%, var(--accent-soft) 0%, transparent 55%),
  radial-gradient(1000px 700px at 120% 20%, oklch(0.95 0.02 220) 0%, transparent 50%),
  var(--bg);
```
（暗色模式右上的色块改用 `oklch(0.24 0.02 220)`）
