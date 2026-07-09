# 远程产品包替换（Remote Package Patch）— Design

**Date**: 2026-07-09
**Status**: Draft / awaiting user review
**Scope**: 新 Rust 模块 `src-tauri/src/remote_package_patch/`（backend）、`src/pages/RemotePackagePatchPage.vue` + `src/components/remote-package-patch/`（frontend）、`src/lib/tauri.ts`（types）、工具注册四件套（router / sidebarNavigation / Sidebar 图标 / ToolsHub 卡片）、`src/locales/messages.ts`。
**Branching**: 按项目惯例直接在 `main` 上开发。
**上游文档**: `.trellis/tasks/07-09-remote-package-patch/{prd.md,design.md,implement.md}`。本 spec 是对 trellis design 的评审修订与细化，冲突处以本 spec 为准。

---

## 1. Problem

运维需要替换 Linux 安装包（`*.tar.gz → *.tar → 组件目录 + *.tar.zst → tar 内容`，zst 解开可达 5-6.x GB）内的单个库文件并更新 md5 清单。拉回 Windows 改再传回去网络与磁盘成本都不可接受。本工具在 Windows 端做 UI/连接/上传替换文件/日志，重活（解包、重写、压缩、md5）全部通过 SSH 在包所在的 Linux 服务器本机完成。

## 2. 评审修订（相对 trellis design.md 的缺口修复）

评审 GPT 生成的 PRD/design/implement 后确认以下缺口，本 spec 逐条修复，实施以本节为准：

1. **缺少包内扫描/浏览命令**。trellis design 定义了 `PackageInternalEntry`/`PackageTargetCandidate` 结构但没有产生它们的命令。修复：新增 `remote_package_scan_package` 命令，一次远程扫描输出全部层级的完整清单（inventory），前端缓存后同时服务于"按文件名匹配候选"和"包内目录浏览"两级选择——浏览不触发二次扫描。
2. **磁盘空间预检缺失**。改包需要在包所在文件系统上产生约 4 倍包体积的临时文件（outer.tar、解开的 inner.tar、重压的 zst、新 outer.tar.gz）。ENOSPC 是最可能的现场失败。修复：patch 脚本 preflight 阶段用 `df -Pk` 硬性校验（见 §7.2），扫描脚本同理（约 3 倍）。
3. **md5 级联规则语义澄清 + tar.zst 自身行更新缺失**。PRD 只写了"目标行 + 上层清单中引用下层 md5 文件的行"，漏掉了被重写的 `*.tar.zst` / 内层 `*.tar` 自身在其所在层清单中的 md5 行。修复：统一为**"被重写文件集合"规则**——每一层（每个 tar 归档内部按目录深度、跨归档层按包裹关系）维护 pending 集合 = 本层被重写的成员（目标文件、更深层清单文件、重压的归档成员），清单文件按路径**深度从深到浅**处理，每更新一个清单文件它自身也进入 pending，向上级联。同名未选中文件永不触碰（路径全匹配，不按 basename）。
4. **取消能力**：明确 **MVP 不做执行中取消**。理由：关闭 SSH channel 不能可靠杀死远端脚本（需要 PID 管理/PTY SIGHUP），而原包在整个流程中只读、失败无损，风险可控。UI 在执行期间禁用全部输入并展示阶段进度。后续版本可加 PID 文件 + kill。
5. **私钥认证细节**：认证方式为**密码**或**私钥文件路径**（Windows 本地路径，`rfd` 选择）+ 可选 passphrase，走 `ssh2::Session::userauth_pubkey_file`。**凭据仅会话内存使用，MVP 不持久化到 config.json**；可从已保存的 DeployServer 下拉预填 host/port/user/password（前端读 `get_config` 即可，不新增后端）。
6. **并发守卫**：扫描与补丁执行共用模块级 `static PATCH_BUSY: AtomicBool`（一次只允许一个重型远程操作），`test_connection` / `list_dir` 不受限。不复用 `is_scanning`（互不相干的业务）。

另有两点实现级陷阱写入契约：**验证阶段的流式管道会触发 SIGPIPE**（`tar -xO` 提前退出使上游 gzip 得 141，`set -o pipefail` 下会误杀脚本），验证管道必须局部 `set +o pipefail`；**大文件入 tar 用硬链接暂存**（`ln` 失败退回 `cp`），避免 GB 级 zst 再复制一份。

**遗留确认项（不阻塞实施，Task 12 手动验证时用真实包核对）**：md5 清单的实际文件名与行格式。本 spec 按通用启发式设计（§7.4），若真实包不符只需调整一个正则常量。

## 3. UI Shape（工作台，非落地页）

页面 `/tools/remote-package-patch`，自上而下四个区块，遵循现有工具页密度（参考 `EnableApplianceSshPage.vue` / `DiskCacheCleanupPage.vue`）：

1. **连接面板**：host / port(22) / user / 认证方式（密码｜私钥文件+passphrase）/ 从已存部署服务器预填的下拉 / 测试连接按钮。连接成功前其余区块禁用。
2. **远程包浏览器**（`RemoteDirBrowser.vue`）：路径栏（可直接输入回车跳转）、上级/刷新按钮、表格（名称/类型/大小/修改时间），目录双击或回车进入，`*.tar.gz` 行高亮并可选中为目标包。
3. **替换设置**：本地替换文件选择（rfd）→ "扫描包结构"按钮 → 三级目标选择：
   - L1 自动候选：inventory 中 basename 等于替换文件名的常规文件列表，单选；
   - L2 包内目录浏览：按 inventory 构建的目录树中选目录，文件名默认取替换文件名、可编辑；
   - L3 手动输入完整包内路径（兜底，无需扫描也可用，远端执行时搜索定位）。
   输出策略：默认生成 `原名.patched.tar.gz`（路径可改）；覆盖模式需勾选 + 二次确认弹窗（展示原包/新包/备份路径与风险文案）。
4. **执行面板**：阶段清单（§7.1 stage 列表逐项打勾/高亮/失败标红）、滚动日志、上传进度条、结果区（输出路径/备份路径/新 md5，可复制）。

## 4. Commands（Tauri 契约）

全部注册在 `main.rs` 的 `generate_handler!`。serde 统一 `#[serde(rename_all = "camelCase")]`。

| Command | 签名（TS 视角） | 说明 |
|---|---|---|
| `remote_package_test_connection` | `(config: RemoteSshConfig) => Promise<string>` | 连接+认证，成功返回远端 `uname -sr` 摘要 |
| `remote_package_list_dir` | `(config, path: string) => Promise<RemoteDirListing>` | SFTP readdir，条目按 目录优先+名称 排序 |
| `remote_package_scan_package` | `(config, packagePath: string) => Promise<PackageInventory>` | 远端执行扫描脚本，流式转发 stage/log 事件，结束返回全量 inventory |
| `remote_package_pick_local_file` | `(kind: 'replacement' \| 'privateKey') => Promise<PickedLocalFile \| null>` | rfd 主线程文件选择（复用 `pick_directory_on_main_thread_with` 模式，见 `.trellis/spec/backend/tauri-native-dialogs.md`） |
| `remote_package_start_patch` | `(request: PackagePatchRequest) => Promise<PackagePatchResult>` | 上传替换文件+脚本、远端执行、流式事件；命令在补丁完成/失败时才 resolve |

`scan_package` 与 `start_patch` 内部 `spawn_blocking`，进入时 CAS `PATCH_BUSY`，退出时释放；被占用直接返回错误字符串（前端 toast）。

## 5. Types（`src/lib/tauri.ts` ↔ Rust）

```typescript
export type RemoteAuth =
  | { kind: 'password'; password: string }
  | { kind: 'keyFile'; keyPath: string; passphrase: string | null };

export interface RemoteSshConfig {
  host: string;
  port: number;
  username: string;
  auth: RemoteAuth;
}

export interface RemoteDirEntry {
  name: string;
  path: string;             // 绝对路径
  kind: 'dir' | 'file' | 'symlink' | 'other';
  size: number;
  modifiedMs: number | null;
}
export interface RemoteDirListing { path: string; entries: RemoteDirEntry[] }

export type InternalLayer =
  | { kind: 'middle' }                      // 中层 tar（outer.tar.gz 内的 *.tar）
  | { kind: 'zst'; zstPath: string };       // 某个 *.tar.zst 展开后的内层 tar

export interface PackageEntry {
  layer: InternalLayer;
  path: string;             // tar 成员名逐字保留（可能带 "./" 前缀）
  kind: 'file' | 'dir' | 'symlink' | 'other';
  size: number;
  permsText: string;        // 如 "-rw-r--r--"
  ownerText: string;        // 如 "root/root"
  mtimeText: string;        // 如 "2026-01-02 03:04"
}
export interface PackageInventory {
  packagePath: string;
  middleTarPath: string;    // outer 内那个唯一 *.tar 的成员名
  entries: PackageEntry[];
}

export interface PickedLocalFile { path: string; name: string; size: number }

export type PatchOutputPolicy =
  | { mode: 'newFile'; outputPath: string }
  | { mode: 'overwrite' };  // 远端自动生成 `<pkg>.bak-<ts>` 备份

export interface PackagePatchRequest {
  config: RemoteSshConfig;
  packagePath: string;
  replacementLocalPath: string;
  targetInternalPath: string;         // 逐字 tar 成员名（L1/L2 来源）或用户输入（L3）
  targetLayer: InternalLayer | null;  // null = L3 手动路径，远端搜索定位
  output: PatchOutputPolicy;
}

export interface PackagePatchResult {
  outputPath: string;
  backupPath: string | null;
  targetMd5: string;
  workdir: string;
  updatedManifests: string[];
}

export interface RemotePackagePatchEvent {
  kind: 'stage' | 'log' | 'result' | 'uploadProgress';
  stage?: string;           // kind=stage
  level?: 'info' | 'warn' | 'error';
  message?: string;         // kind=log
  key?: string; value?: string;   // kind=result
  sent?: number; total?: number;  // kind=uploadProgress
}
```

事件名：`remote-package-patch-event`（`app_handle.emit`，同 deploy.rs 风格）。

## 6. 后端模块边界

```
src-tauri/src/remote_package_patch/
├── mod.rs        # 5 个 command、事件发射、PATCH_BUSY 守卫、编排
├── ssh.rs        # 连接（密码/私钥）、keepalive(15s)、exec 流式读、SFTP 上传（64KB 块+进度）
├── script.rs     # sh_quote、扫描/补丁脚本模板（@TOKEN@ 占位替换，值先经 sh_quote）
├── inventory.rs  # `tar -tv` 行解析 → PackageEntry；##RAW 行汇聚 → PackageInventory
└── protocol.rs   # 脚本输出行协议解析（##STAGE/##LOG/##RESULT/##ERROR/##RAW/裸行）
```

纯逻辑（quote、模板、解析）全部单测；ssh.rs 只有参数校验可单测，连接路径靠 Task 12 手动验证。

## 7. 远端执行契约

### 7.1 行协议与阶段

脚本 stdout 逐行解析：

- `##STAGE:<id>` — 阶段切换。补丁阶段 id 固定集合：`preflight, upload(由 Rust 侧发出), unpack_outer, extract_middle, resolve_target, extract_inner, replace_member, update_md5, repack_inner, repack_middle, repack_outer, compress_outer, verify, backup_overwrite, finalize, cleanup`。扫描阶段：`scan_preflight, scan_outer, scan_middle, scan_inner, scan_done`。
- `##LOG:<level>:<msg>` — 结构化日志（level ∈ info/warn/error）。
- `##RESULT:<key>=<value>` — 结果值（`output_path, backup_path, replacement_md5, workdir, resolved_target, resolved_layer, updated_manifest`，其中 updated_manifest 可多次出现）。
- `##ERROR:<msg>` — 致命错误说明（脚本随后非零退出）。
- `##RAW:<layer>\t<tar -tv 原始行>` — 仅扫描脚本；`<layer>` 为 `outer` / `middle` / `zst:<zst成员路径>`。Rust 解析入 inventory，**不**作为日志事件转发（十万行级）。
- 其余行 → info 日志透传；stderr 全部按 error 日志透传。

### 7.2 补丁脚本（`#!/bin/bash`，`set -euo pipefail`，`export LC_ALL=C`）

工作目录 `<pkgdir>/.file-sync-tool-patch-<ts>/`（Rust 生成，SFTP mkdir 后先上传替换文件与脚本，再 `bash <script>` 执行）。要点：

- **preflight**：包存在且可读；`newFile` 模式下输出路径不存在（存在即失败，不静默覆盖）；`zstd` 可用；`df -Pk <pkgdir>` 可用空间 ≥ `4 × 包文件KB`，不足即失败并打印两个数字；`md5sum` 计算替换文件 md5 → `##RESULT:replacement_md5`。
- **unpack_outer**：`gzip -dc "$PKG" > "$WORK/outer.tar"`。
- **extract_middle**：outer 内 `*.tar` 成员必须恰好 1 个（0 或 >1 都报错并列出），提取到 `$WORK/m/`。
- **resolve_target**：显式 layer（middle/zst）→ 在对应 listing 中精确匹配成员名（容忍 `./` 前缀差异，取 listing 中的逐字形式）；`auto`（L3 手动输入）→ 先查 middle listing，再逐个 zst 流式列表查找；命中 0 → 报错，命中 >1 → 报错并列出全部命中让用户改用显式选择。`##RESULT:resolved_target` / `resolved_layer`。
- **replace_member**（对任一 tar 的通用函数）：`tar -tvf` 取原成员行 → 必须是常规文件（`-` 开头）→ 解析 perms/owner/group → 暂存目录按成员路径摆放替换文件（优先 `ln` 硬链接，失败 `cp`）→ `tar --delete` → `tar --append -C <stage> --owner=… --group=… --mode=<symbolic>`（owner/group 为纯数字时用 `--owner=:UID` 形式；mode 由 9 位 perms 转 `u=…,g=…,o=…`）。
- **update_md5**（§7.4 规则，逐归档层调用）。
- **repack**：inner.tar → `zstd -T0 -f`（默认级别 3，重压后体积可能与原包不同，可接受）→ 替换回 middle tar → 替换回 outer.tar → `gzip -c` 出 `$WORK/output.tar.gz`。
- **verify**：从 `$WORK/output.tar.gz` 全链路流式抽出目标成员 `md5sum`，必须等于替换文件 md5；每个被更新的清单成员重新抽出，`grep -F` 到新 md5。**此段管道包裹 `set +o pipefail` / `set -o pipefail`**（SIGPIPE 141 陷阱）。
- **backup_overwrite**（仅 overwrite 模式，验证通过后）：`cp -p "$PKG" "$BACKUP"` → `mv -f "$WORK/output.tar.gz" "$PKG"`；失败保留备份并打印恢复命令。newFile 模式：`mv "$WORK/output.tar.gz" "$OUTPUT"`。
- **cleanup**（trap EXIT）：成功 `rm -rf "$WORK"`；失败保留并 `##LOG:warn:` 打印路径（PRD R10）。原包全程只读。

### 7.3 扫描脚本

同一 workdir 约定（`.fst-scan-<ts>`，无论成败结束即删——扫描对包只读）：`df` 预检 ≥ 3× 包体积 → `gzip -dc | tar -tv` 出 outer 层 `##RAW:outer` → 流式抽出 middle tar 到磁盘一次 → `tar -tvf` 出 `##RAW:middle` → 对每个 `*.tar.zst` 成员 `tar -xOf | zstd -dc | tar -tv` 出 `##RAW:zst:<path>`（awk 前缀，避免 sed 特殊字符转义）。

### 7.4 md5 清单规则（"被重写文件集合"）

- **清单识别**：常规文件成员，basename 匹配（大小写不敏感）`^(.+\.)?md5(sum)?(\.txt)?$`。
- **行格式**：`^<32位hex><分隔符><路径>$`，分隔符 = 空格/制表符/`*` 的连续串，逐字保留；路径可含空格。
- **行匹配**：pending 条目路径 p 与行内路径相等，比较集合 = { p, `./`+p, p 去掉清单所在目录前缀后的相对路径, 及其 `./` 变体 }。**绝不按 basename 匹配**（同名保护，PRD R6）。
- **层内级联**：清单按路径深度**从深到浅**处理；清单文件被改写后，其自身 (路径, 新md5) 加入 pending，供更浅清单更新（覆盖 PRD"上层清单记录下层 md5 文件"的场景）。
- **跨归档级联**：inner.tar 重压出的 zst、重写的 middle.tar 在各自上层归档中作为普通被重写成员进入该层 pending（修复评审缺口 3）。
- 某 pending 条目在所有清单中 0 命中 → `##LOG:warn:` 并继续（允许无清单的包）；有命中则记 `##RESULT:updated_manifest`。

## 8. 安全与默认值汇总

- 默认输出 `X.patched.tar.gz`（`X.tar.gz` 在 `.tar.gz` 前插入 `.patched`；非 `.tar.gz` 结尾则直接追加）。备份名 `X.tar.gz.bak-<YYYYMMDDHHmmss>`。
- 覆盖模式默认关闭，勾选后执行前必须通过确认弹窗（列出三个路径 + 风险文案）。
- 凭据不落盘；日志与事件中不得出现密码/passphrase。
- 所有嵌入脚本的值经 `sh_quote`（POSIX 单引号包裹 + `'\''` 转义），模板占位符 `@TOKEN@` 全大写、golden 测试断言替换完整。
- 目标成员必须是常规文件；symlink/目录/硬链接目标直接报错。
- 仅支持 middle 层与 zst 层目标；outer 层直接文件（通常只有那个 middle tar）不支持，报错提示。

## 9. Testing

- Rust 单测：`sh_quote`（空格/单引号/中文/空串）、`tar -tv` 解析（普通/空格路径/symlink `->`/`./`前缀/数字 owner）、行协议解析、两个脚本构建器 golden 断言（占位符全消除、关键结构存在：`set -euo pipefail`、`set +o pipefail` 于 verify 段、trap、df 校验、深到浅排序、`--delete`/`--append` 序列、overwrite 分支）。
- 前端 node --test（`src/lib/remotePackagePatch.test.mjs`）：候选过滤、目录树构建、目标路径拼接与校验（拒绝 `..`/空/尾斜杠）、默认输出路径推导、层显示文案键。
- `pnpm check`（vue-tsc）、`pnpm lint`。
- Task 12 手动验证：提供合成 fixture 生成脚本（构造 `tar.gz→tar→tar.zst→tar` + 双层 md5 清单的小包），在任一 Linux 主机跑通全流程 + 核对真实包的 md5 清单命名（§2 遗留确认项）。

## 10. Out of Scope（MVP）

- 执行中取消、断点续传、临时目录自定义、zstd 压缩级别选项。
- Windows 本地改包、缺 zstd 兜底、任意压缩格式通用编辑。
- 凭据持久化与私钥内容粘贴输入。
- 自动安装产品包。
