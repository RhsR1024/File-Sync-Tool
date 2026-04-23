# 硬盘缓存清理多源扩展（本地盘 Windows/Linux + IPSAN）— 设计文档

**日期**: 2026-04-23  
**范围**: 将现有“硬盘缓存清理”从“仅支持 Linux 本地盘”扩展为同时支持：

- `Linux 本地盘`
- `Windows 本地盘`
- `IPSAN`

**实现分支**: `main`  
**关联文档**:

- 继承并替代 [2026-04-22-disk-cache-cleanup-design.md](./2026-04-22-disk-cache-cleanup-design.md) 中与“硬盘缓存清理”相关的设计部分
- 2026-04-22 文档中关于“三个旧工具最近使用持久化”的内容保持不变，不在本次设计内重写

---

## 1. 结论先行

本次设计采用单页双区域工作台：

1. 页面顶部保留统一查询入口：接入 IP、本地盘类型 Tab、API 超时、`获取缓存状态`
2. 内容区固定同时显示两个区域：
   - `本地盘`
   - `IPSAN`
3. `本地盘` 区域内部使用横向胶囊 Tab 切换：
   - `Windows 本地盘`
   - `Linux 本地盘`
4. `IPSAN` 区域不区分系统类型，不使用 Tab
5. 切换本地盘 Tab 时，只刷新本地盘区域，`IPSAN` 保留上次结果
6. 顶部统一查询时，同时查询：
   - 当前选中的本地盘类型
   - `IPSAN`
7. 各资源缓存 key 规则固定为：
   - Linux 本地盘：`Storage:{storageId}`
   - Windows 本地盘：`Storage:{partitionGUID}`
   - IPSAN：`Storage:{IPSANId}`

---

## 2. 方案对比与选型

### 方案 A：单页双区域 + 本地盘内嵌 Tab（采用）

**结构**:

- 顶部统一输入与触发
- 下方两个结果区块：`本地盘`、`IPSAN`
- `本地盘` 内部用 Tab 切 `Windows / Linux`

**优点**:

- 同一台接入服务器上的两类资源可同时查看
- 本地盘类型切换成本低，符合工具型页面节奏
- `IPSAN` 不受本地盘类型切换影响，区域边界清晰

**缺点**:

- 页面状态比单列表复杂，需要拆分区域级 loading / error / redis 状态

### 方案 B：单页双区域 + 折叠式本地盘类型切换（未采用）

**原因**:

- `Windows 本地盘` 和 `Linux 本地盘` 会长期占用同一级视觉层级
- 页面会比 Tab 方案更散，密度更高但秩序更差

### 方案 C：向导式分步骤查询（未采用）

**原因**:

- 与“缓存检查 / 点一下就清理”的工具心智不匹配
- 用户操作链过长，不适合频繁切换主机排查

---

## 3. 页面信息架构

页面分为三层：

### 3.1 顶部统一入口

字段与控件：

- `接入 IP`
- `本地盘类型 Tab`
  - `Windows 本地盘`
  - `Linux 本地盘`
- `API 超时`
- 主按钮：`获取缓存状态`
- 只读说明：`Redis 目标`

顶部入口只负责条件输入与统一触发，不承担结果展示。

### 3.2 本地盘区域

独立卡片，标题固定为 `本地盘`，副标题根据当前 Tab 动态显示：

- `Windows 本地盘`
- `Linux 本地盘`

区域右侧操作：

- `刷新本地盘`
- `清理本地盘全部命中`

### 3.3 IPSAN 区域

独立卡片，标题固定为 `IPSAN`，无系统类型切换。

区域右侧操作：

- `刷新 IPSAN`
- `清理 IPSAN 全部命中`

---

## 4. 数据源与 Redis key 规则

### 4.1 Linux 本地盘

HTTP 接口：

- `POST /openAPI/system/v1/disk/server/list`
- `POST /openAPI/system/v1/disk/list`

数据主键：

- 行级资源主键：`storageId`

Redis key：

- `Storage:{storageId}`

按钮挂载层级：

- 磁盘行

### 4.2 Windows 本地盘

HTTP 接口：

- `POST /openAPI/system/v1/raw-disk/list`

数据结构：

- 磁盘层：`diskId / diskNumber / diskName / totalCapacity`
- 分区层：`partitionList[]`

数据主键：

- 行级资源主键：`partitionGUID`

Redis key：

- `Storage:{partitionGUID}`

按钮挂载层级：

- 分区行

**关键结论**: Windows 缓存清理粒度是“分区”，不是整盘。  
因此页面必须显示“磁盘分组 + 分区子表”，而不是只显示磁盘列表。

### 4.3 IPSAN

HTTP 接口：

- `POST /openAPI/system/v1/IPSAN/list`

数据主键：

- 行级资源主键：`IPSANId`

Redis key：

- `Storage:{IPSANId}`

按钮挂载层级：

- IPSAN 行

---

## 5. 统一查询与刷新规则

### 5.1 顶部统一查询

点击 `获取缓存状态` 后，并发触发两条链路：

1. 本地盘区域按当前 Tab 类型查询
2. `IPSAN` 区域查询 `IPSAN/list`

两个区域各自维护 loading / success / error / redis 状态，不互相覆盖。

### 5.2 本地盘区域刷新

#### Windows Tab

流程：

1. 调 `/raw-disk/list`
2. 展平全部 `partitionGUID`
3. Pipeline 检查 `Storage:{partitionGUID}`
4. 按“磁盘分组 + 分区行”渲染

#### Linux Tab

流程：

1. 调 `/disk/server/list`
2. 若返回子机列表为空，则显示空态
3. 若有子机：
   - 优先保留当前已选 `serverIp`
   - 若当前值失效，则默认选第一台
4. 调 `/disk/list`
5. 提取 `storageId`
6. Pipeline 检查 `Storage:{storageId}`

#### Tab 切换

- 只刷新本地盘区域
- `IPSAN` 区域保持上次成功结果

#### Linux 子机切换

- 只刷新 Linux 本地盘表格和该区域 Redis 状态

### 5.3 IPSAN 区域刷新

- 顶部统一查询时刷新一次
- `刷新 IPSAN` 只刷新本区域
- 单条清理成功或批量清理成功后，只重查本区域

---

## 6. 页面展示与字段布局

### 6.1 顶部统一入口

展示内容：

- 接入 IP 输入框
- 本地盘类型 Tab（胶囊式）
- API 超时下拉
- `获取缓存状态`
- `Redis 目标: {host}:6379`

### 6.2 Windows 本地盘视图

每块磁盘使用一个分组卡片：

- 磁盘头字段：
  - `diskNumber`
  - `diskName`
  - `totalCapacity`

磁盘下方为分区子表，列如下：

- `分区序号`
- `Partition GUID`
- `容量`
- `用途`
- `状态`
- `缓存`
- `操作`

补充说明：

- `用途` 来自 `partitionList[].usage`
- `状态` 来自 `partitionList[].partitionStatus`
- 若某分区对应 Redis key 存在，则显示 `清理缓存`
- 若 Redis key 不存在，则不显示该按钮

### 6.3 Linux 本地盘视图

展示结构：

1. 子机选择条
2. 磁盘表格

保留现有偏紧凑表格风格，列如下：

- `槽位`
- `设备`
- `容量`
- `用途`
- `状态`
- `缓存`
- `操作`

### 6.4 IPSAN 视图

独立表格列如下：

- `IPSAN 名称 / IP`
- `IPSANId`
- `状态`
- `总容量`
- `用途`
- `缓存`
- `操作`

补充说明：

- `用途` 来自 `IPSANInfoList[].usage`
- `状态` 来自 `IPSANInfoList[].IPSANStatus`
- 若 Redis key 存在，则显示 `清理缓存`
- 若 Redis key 不存在，则不显示该按钮

### 6.5 状态字段兜底策略

由于 Windows `partitionStatus` 与 IPSAN `IPSANStatus` 的完整枚举合同未在当前需求中给全，MVP 显示规则固定为：

- 优先按已知映射显示文字
- 未知状态码统一显示 `状态 {code}`

这保证页面不会因为后端枚举未补齐而阻塞上线。

---

## 7. 按钮显隐与动作范围

### 7.1 行级按钮

规则：

- 仅当 Redis 中存在对应 key 时，显示 `清理缓存`
- 不命中时不显示按钮，不展示禁用态

适用对象：

- Linux：磁盘行
- Windows：分区行
- IPSAN：IPSAN 行

### 7.2 区域级批量按钮

区域级批量按钮始终存在，但遵循以下规则：

- 当前区域存在可清理 key 时可点击
- 当前区域 Redis 不可用时置灰
- 当前区域没有命中 key 时置灰

批量按钮按区域拆分，不做跨区域总清理：

- `清理本地盘全部命中`
- `清理 IPSAN 全部命中`

**不做** 顶部“一键清理全页面全部资源”的总按钮，避免误删范围过大。

---

## 8. 错误、告警与加载体验

### 8.1 接入 IP 校验

- 接入 IP 为空时，顶部主按钮禁用
- 输入框下方显示轻量校验提示

### 8.2 本地盘区域错误

出现以下情况时，仅在本地盘区域显示红色错误条：

- Windows 接口失败
- Linux `/disk/server/list` 失败
- Linux `/disk/list` 失败
- Tab 选错类型导致接口失败

规则：

- 不自动切换 Tab
- 不自动 fallback 到另一套流程
- 不影响 `IPSAN` 区域已成功结果

### 8.3 IPSAN 区域错误

`IPSAN/list` 失败时，仅在 `IPSAN` 区域显示红色错误条。

### 8.4 Redis 告警

哪个区域的 Redis 检查失败，就只在那个区域显示黄色告警条，并且：

- 该区域行级清理按钮隐藏
- 该区域批量按钮置灰
- 另一块区域不受影响

### 8.5 刷新时的旧数据保留策略

采用“保留旧数据 + 区域头部 loading + 结果表半透明覆盖”的策略：

- 避免刷新时整块闪空
- 尤其适合 `IPSAN` 与本地盘互相独立的双区域布局

---

## 9. 前后端边界与推荐命令模型

### 9.1 前端共享状态

- `hostIp`
- `localDiskTab`
- `timeoutSecs`

### 9.2 本地盘区域状态

- Linux:
  - `linuxServerList`
  - `selectedLinuxServerIp`
  - `linuxDisks`
- Windows:
  - `windowsDisks`
- 通用：
  - `localPresentCacheKeys`
  - `localLoading`
  - `localError`
  - `localRedisAvailable`
  - `localRedisError`

### 9.3 IPSAN 区域状态

- `ipsans`
- `ipsanPresentCacheKeys`
- `ipsanLoading`
- `ipsanError`
- `ipsanRedisAvailable`
- `ipsanRedisError`

### 9.4 推荐 Tauri Command 边界

为避免继续把所有资源类型耦合在一个“只懂 storageId”的接口里，推荐命令边界调整为：

| Command | 入参 | 返回 |
|---|---|---|
| `disk_cleanup_list_linux_servers` | `host, timeout_secs` | `Vec<LinuxServerItem>` |
| `disk_cleanup_list_linux_disks` | `host, server_ip, timeout_secs` | `Vec<LinuxDiskItem>` |
| `disk_cleanup_list_windows_disks` | `host, timeout_secs` | `Vec<WindowsDiskItem>` |
| `disk_cleanup_list_ipsans` | `host, timeout_secs` | `Vec<IpsanItem>` |
| `disk_cleanup_check_cache_keys` | `host, keys` | `CacheKeyCheckResult` |
| `disk_cleanup_delete_cache_keys` | `host, keys` | `CacheKeyDeleteResult` |

推荐将 Redis 检查/删除收敛为“直接处理完整 key 列表”，原因：

- Linux / Windows / IPSAN 三类资源主键不同
- Redis 层只需要处理 `Storage:*`
- 前端可以清晰地把“当前行对应的完整 key”传给后端

后端需要校验：

- key 去空、去重
- 只允许 `Storage:` 前缀

---

## 10. i18n 与视觉语言

### 10.1 视觉分区

- 本地盘区域：延续蓝灰 / 青蓝色系
- IPSAN 区域：使用琥珀 / 橙色标题条或浅色背景
- 危险动作统一红色

### 10.2 使用体验约束

- 本地盘和 IPSAN 纵向堆叠，不做常驻双栏
- 原因：Windows 分区表和 Linux 磁盘表都偏宽，双栏会挤压可读性
- 窄屏允许横向滚动表格，不重做移动端卡片列表

### 10.3 i18n 增量

需要补充以下字典：

- 顶部统一入口文案
- `Windows 本地盘 / Linux 本地盘 / IPSAN`
- Windows 分区列头
- IPSAN 列头
- 区域级刷新与批量清理文案
- 区域级错误与 Redis 告警文案

---

## 11. 文件改动范围

### 11.1 前端

- `src/pages/DiskCacheCleanupPage.vue`
- `src/lib/tauri.ts`
- `src/locales/messages.ts`

### 11.2 后端

- `src-tauri/src/disk_cleanup.rs`
- `src-tauri/src/main.rs`

### 11.3 配置

- `src-tauri/src/config.rs`
- `src/lib/tauri.ts`

### 11.4 导航（若现有入口保持不变则只微调文案）

- `src/router/index.ts`
- `src/lib/sidebarNavigation.ts`
- `src/pages/ToolsHubPage.vue`

---

## 12. 手测路径

1. Linux 本地盘：
   - 输入接入 IP
   - 切到 `Linux 本地盘`
   - 获取缓存状态
   - 选择子机
   - 验证磁盘行级 `清理缓存`
2. Windows 本地盘：
   - 输入接入 IP
   - 切到 `Windows 本地盘`
   - 获取缓存状态
   - 验证“磁盘分组 + 分区子表”
   - 验证分区级 `清理缓存`
3. IPSAN：
   - 统一查询后直接查看 IPSAN 区域
   - 验证行级 `清理缓存`
4. Tab 切换：
   - 切换 `Windows / Linux`
   - 验证仅本地盘区域刷新
   - 验证 IPSAN 保留上次结果
5. 类型选错：
   - 切到错误 Tab
   - 验证仅本地盘区域报错
   - IPSAN 正常显示
6. Redis 不可用：
   - 模拟 Redis 无法连接
   - 验证对应区域告警和批量按钮置灰
7. 批量清理：
   - 分别验证本地盘区域、IPSAN 区域
   - 成功后只重查所属区域

---

## 13. 不做

- 不自动探测 Windows / Linux
- 不在设置页新增服务器 OS 类型字段
- 不做跨区域总清理按钮
- 不做 Redis 密码可配置
- 不做本地盘与 IPSAN 的自动轮询刷新
- 不在本次补全所有未知状态码的语义映射

---

## 14. 实施顺序建议

1. 先收敛后端命令边界，支持 Windows / Linux / IPSAN 三类查询
2. 再收敛 Redis 检查/删除为完整 key 处理
3. 更新前端类型定义与区域级状态模型
4. 重构 `DiskCacheCleanupPage.vue` 为“统一入口 + 双区域”
5. 最后补 i18n、验证与回归
