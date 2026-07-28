# EnhanceAnyLexer Matcher Presets Design

- **Date**: 2026-07-27
- **Status**: Approved
- **Owner**: claude-agent
- **Scope**: 在 Notepad++ 扩展中心的 EnhanceAnyLexer 高亮配置中，为规则引入预设匹配器层，把裸正则降级为逃生舱。

---

## Goal

当前每条高亮规则只能手写正则（`EnhanceAnyLexerRule.pattern`）。对多数使用场景（日志关键字、错误级别整行标色、引号内容）来说门槛过高。

目标是在正则之上加一层**预设匹配器**：

1. 常见需求通过表单完成，不接触正则语法。
2. 预设编译成正则后落盘，正则始终是唯一真相来源。
3. 预设不满足时可单向切换到裸正则模式。
4. 用户在 Notepad++ 中手工修改过的规则不被静默覆盖。

不改变插件本身，也不改变 ini 文件格式的可读性。

---

## Engine Facts

以下事实通过手写 ini 在真实 Notepad++ + EnhanceAnyLexer 1.4.1 上实测确认（两轮探针，共 16 条规则）。编译器的全部输出形态以此为准。

| 能力 | 结论 |
| --- | --- |
| 正则引擎 | Boost.Regex，Perl 语法，与 Notepad++ 查找对话框同一套 |
| 交替 `\|` | 支持 |
| 非捕获组 `(?:)` | 支持 |
| 词边界 `\b` | 支持，且对 CJK 字符按词字符正确处理 |
| 词边界 `\<` `\>` | 支持，但不使用（`\b` 更通用） |
| 内联标志 `(?i)` `(?-i)` | 支持 |
| **默认大小写** | **忽略大小写**（插件未设置 `SCFIND_MATCHCASE`） |
| 锚点 `^` `$` | 按行生效，`^.*X.*$` 可实现整行匹配 |
| 数字类 `\d`、量词 `+` `{n,m}` | 支持 |
| 字符类否定 `[^"]` | 支持，多段内容各自独立着色 |
| **规则优先级** | **后定义的覆盖先定义的**，逐字符覆盖 |

两条最关键、且与直觉相反的结论：

- **默认忽略大小写**。因此横切开关必须是「区分大小写」而不是「忽略大小写」，开启时编译成 `(?-i)` 前缀。
- **列表越靠下优先级越高**。插件用单个 indicator 逐字符写入，最后写入的规则赢。这与常见的「先匹配者胜」相反。

### CJK 注意事项

`\b` 对 CJK 生效，但语义上通常不是用户想要的：中文没有词间空格，`\b检查\b` 只会命中孤立的「检查」，不会命中「请检查配置」中的「检查」。

因此含 CJK 字符的词条启用整词匹配时，UI 必须给出提示。采用**提示而非强制关闭**：输入过程中自动改动开关状态会造成困惑。

---

## Data Model

### 匹配器

```rust
pub enum EnhanceMatcherKind {
    Words,   // 关键字列表
    Line,    // 整行
    Between, // 包围内容
    Preset,  // 内置语义
    Regex,   // 原始正则（默认）
}

pub struct EnhanceMatcher {
    pub kind: EnhanceMatcherKind,
    pub terms: Vec<String>,   // words / line
    pub open: String,         // between
    pub close: String,        // between
    pub preset: String,       // preset id
    pub whole_word: bool,     // 横切
    pub case_sensitive: bool, // 横切
    pub line_start: bool,     // 横切
}
```

`EnhanceAnyLexerRule` 新增 `matcher: EnhanceMatcher` 字段，带 `#[serde(default)]`，默认 `kind = Regex`。旧配置与旧前端负载在不带该字段时行为完全不变。

### 编译模板

编译顺序固定为：词条转义 → 交替合并 → 整词包裹 → 按 kind 组装 → 大小写前缀。

| kind | 模板 | 示例 |
| --- | --- | --- |
| `words` | `<core>`，`line_start` 时前置 `^` | `\b(?:ERROR\|FATAL)\b` |
| `line` | `^.*<core>.*$` | `^.*\bERROR\b.*$` |
| `between` | `<open>[^<close>]*<close>`（close 为单字符）或 `<open>.*?<close>` | `"[^"]*"` |
| `preset` | 预设表中的固定正则 | `\b(?:\d{1,3}\.){3}\d{1,3}\b` |
| `regex` | 不编译，`pattern` 原样使用 | — |

其中 `core` 是词条转义后用 `(?:a|b|c)` 合并的结果，单个词条时省略 `(?:)`；`whole_word` 时两侧包 `\b`。

横切选项：

- `case_sensitive` → 整体前置 `(?-i)`，必须在最外层最前面。
- `whole_word` → `core` 两侧包 `\b`；`preset` 与 `between` 忽略此项（预设自带边界，包围符两侧加边界无意义）。
- `line_start` → `words` 前置 `^`；`line` 忽略此项（已含 `^`）。

### 预设表

| id | 正则 |
| --- | --- |
| `ipv4` | `\b(?:\d{1,3}\.){3}\d{1,3}\b` |
| `number` | `\b\d+(?:\.\d+)?\b` |
| `hex` | `\b0[xX][0-9a-fA-F]+\b` |
| `version` | `\bv?\d+(?:\.\d+){1,3}\b` |
| `url` | `\bhttps?://[^\s"'<>]+` |
| `win_path` | `\b[A-Za-z]:\\[^\s"'<>\|]*` |
| `timestamp` | `\b\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}\b` |
| `guid` | `\b[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}\b` |
| `mac` | `\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b` |

未知 preset id 视为配置错误，保存时报 `enhance_preset_unknown`。

### 转义

词条与包围符需要转义 Perl 元字符：`\ ^ $ . | ? * + ( ) [ ] { }`。

不使用 Boost 的 `\Q…\E`：词条自身包含 `\E` 时会破坏结构。逐字符转义没有这个问题。

---

## Persistence

沿用现有的 `; FST-` 注释机制（`; FST-NAME`、`; FST-DISABLED`），新增一行承载匹配器：

```ini
; FST-NAME 日志级别
; FST-MATCH {"kind":"words","terms":["ERROR","FATAL"],"whole_word":true}
#FF5555 = \b(?:ERROR|FATAL)\b
```

写入规则：

- `kind = Regex` 时不写 `; FST-MATCH`，保持 ini 干净。
- JSON 序列化后不得含换行；含换行时视为异常，跳过该注释。

读取规则（**降级逻辑**，本设计的核心）：

1. 解析 `; FST-MATCH`，得到候选 matcher。
2. 用候选 matcher 重新编译出正则。
3. 与紧随其后的实际正则逐字符比较。
4. 一致 → 采用该 matcher，规则以表单形态呈现。
5. **不一致或解析失败 → 丢弃 matcher，降级为 `kind = Regex`**，以文件中的实际正则为准。

这保证用户在 Notepad++ 里直接改 ini 后，改动不会被下次保存静默还原。

---

## Save Normalization

保存前对配置做一次归一化，位置在 `validate_enhance_config` 之前：

- 对每条 `kind != Regex` 的规则重新编译，用编译结果**覆盖** `rule.pattern`。
- 编译失败则整次保存失败，返回具体错误码。

这样 `pattern` 永远与 `matcher` 自洽，渲染层不需要再判断谁是权威。

新增错误码：

- `enhance_matcher_terms_empty`
- `enhance_matcher_delimiter_empty`
- `enhance_preset_unknown`

---

## Command

新增 `notepad_extensions_compile_matcher(matcher) -> Result<String, String>`。

前端规则卡片实时显示「生成的正则」时调用，带防抖。编译逻辑只在 Rust 侧存在一份，避免前后端两套实现漂移。

---

## UI

### 规则卡片

顶部模式选择器（`words` / `line` / `between` / `preset` / `regex`），下方表单随模式切换：

- `words`、`line`：词条输入（逗号分隔）
- `between`：起始符、结束符
- `preset`：预设下拉
- `regex`：正则输入框

横切开关三个：区分大小写、整词匹配（`preset`/`between`/`regex` 下隐藏）、仅行首（`line`/`regex` 下隐藏）。

非 `regex` 模式下，卡片底部固定显示**只读的编译结果**，旁边一个「转为正则」按钮。

「转为正则」**单向不可逆**：把当前编译结果写入 `pattern` 并切到 `regex` 模式。不实现正则反解成预设——反解只在读 ini 时按 `; FST-MATCH` 注释做，不在交互中做。

模式选择器切到 `regex` 时必须走同一条路径，否则用户在预设表单里填的内容会随着模式切换凭空消失。

### 优先级表达

- 规则区说明文案改为明确表述「列表越靠下优先级越高，后面的规则会覆盖前面的颜色」。
- 每张规则卡片增加上移 / 下移按钮。
- `kind = line` 且 `index > 0` 时，卡片内显示提示：该整行规则会覆盖上方规则的颜色，建议移到列表顶部。
- 含 CJK 词条且启用整词匹配时，卡片内显示 CJK 提示。

### 预览修正

现有预览有两处与实测语义不符，一并修正：

1. **大小写**：现在无条件使用 `i` 标志。该行为恰好等于插件默认值，但加入「区分大小写」开关后会失效。改为解析 `pattern` 开头的 `(?i)` / `(?-i)` 内联标志决定 JS 标志位，无标志时默认忽略大小写。JS 不支持内联标志，必须先剥离再传入 `RegExp`。
2. **优先级**：现在遇到第一条命中的规则就返回，等于「先匹配者胜」。实测是**后定义者逐字符覆盖**。改为按字符维护颜色数组，顺序应用所有启用规则，后者覆盖前者，最后合并为渲染片段。

### 预览的已知偏差

预览用浏览器 `RegExp`（ECMAScript），与 Boost.Regex 存在无法消除的差异，最主要的一处：

- JS 的 `\b` 基于 `\w = [A-Za-z0-9_]`，**对 CJK 不生效**；Boost 则生效。含中文词条且启用整词匹配时，预览结果会与 Notepad++ 中的实际效果不同。

预览定位是「近似预览」（现有 `previewHint` 文案已如此声明），不追求引擎级一致。

---

## Non-Goals

- 不支持加粗、斜体等字体样式。EnhanceAnyLexer 基于 Scintilla indicator，只能覆盖前景色，这是插件的硬限制。
- 不实现正则反解成预设。
- 不改动 ini 文件的既有格式与 `[global]` 字段。
- 不引入新的插件或新的配置文件。
- 不处理 `line` 模式与 `excluded_styles` 的交互。整行匹配横跨多种 style 时着色可能断裂，属于插件行为，待实际遇到再评估。

---

## Cross-Layer Checklist

字段跨层新增，需同步确认：

- `src-tauri/src/notepad_extensions.rs`：结构体、`#[serde(default)]`、编译器、解析、渲染、校验、命令
- `src-tauri/src/main.rs`：命令注册
- `src/lib/tauri.ts`：TS 类型、API 封装
- `src/pages/NotepadExtensionsPage.vue`：规则卡片、预览、排序
- `src/locales/messages.ts`：中英文文案同步
