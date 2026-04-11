# 文件共享 — 按用户独立配置可见目录与权限

## Goal

每个用户拥有独立的"目录访问列表"，每条记录绑定该用户对该目录的权限（preset + 细粒度）。
`root_permissions` 为空 → 看不到任何目录（严格模式）。
**无需向后兼容旧配置**，功能目前无人使用，直接破坏性变更。

## Requirements

### Rust — 数据模型 (`model.rs`)

1. 新增 `UserRootPermissions { root_id: String, preset: PermissionPreset, permissions: FileSharePermissionSet }`
2. `PersistedFileShareUser` 删除全局 `preset` 和 `permissions` 字段，替换为 `root_permissions: Vec<UserRootPermissions>`
3. 对应更新 `FileShareUserView`（view struct）
4. 对应更新 `FileShareUserSaveRequest`（save request struct）
5. `default_guest_account()` 改为 `root_permissions: vec![]`

### Rust — 认证层 (`auth.rs`)

6. `ResolvedPrincipal` 新增 `root_permissions: Vec<UserRootPermissions>`，`permissions` 字段改为由所有 root_permissions 的 OR 合并计算（用于 session 响应全局展示）
7. 新增辅助方法：
   ```rust
   fn permissions_for_root(&self, root_id: &str) -> Option<FileSharePermissionSet>
   // root_id 在列表中 → Some(permissions)
   // 不在列表 → None（禁止访问）
   ```

### Rust — 运行时 & HTTP (`mod.rs`, `http.rs`)

8. `principal_for_user`：填充 `root_permissions`，`permissions` 改为 OR 合并
9. `build_home_tree_response`：用 `principal.root_permissions` 过滤 roots，仅渲染有权限的
10. `find_root` / `resolve_node` / `resolve_parent_directory_node`：每次 lookup 后校验 `permissions_for_root`，无权则返回 404（不用 403，避免暴露目录存在）
11. 所有文件操作 handler（create_dir / create_text / rename / delete / upload_files / upload_dir / download / archive / preview）：用 `permissions_for_root(root_id)` 替代 `principal.permissions` 做权限判断
12. `handler_tree_search` 全局搜索：仅在 `principal.root_permissions` 中的 root_id 对应的 roots 范围内执行

### Rust — 配置持久化 (`persist.rs`)

13. `build_persisted_user`：接受并保存 `root_permissions`，删除旧的 `permissions` / `preset` 字段处理
14. `normalize_persisted_user`：更新为新字段（`permissions_for_preset` 逻辑移至 per-root 层）
15. `permissions_for_preset` 仍保留，在 per-root 规范化时使用

### TypeScript — 类型定义 (`tauri.ts`)

16. 新增 `FileShareUserRootPermissions { root_id: string; preset: FileSharePermissionPreset; permissions: FileSharePermissionSet }`
17. `FileShareUserView` 删除全局 `preset` / `permissions`，加 `root_permissions: FileShareUserRootPermissions[]`
18. `FileShareUserSaveRequest` 同上

### 前端 — 设置 UI (`FileSharePage.vue`)

19. 用户编辑器（guest 和 account）改版：
    - 删除全局 preset 选择器和全局 permission checkboxes
    - 新增"目录访问"区块：渲染所有全局 roots 作为行列表
      - 每行：勾选框（是否授权此目录）+ 目录别名
      - 选中时展开：preset 下拉（只读 / 读写 / 自定义）
      - preset = 自定义时展开 12 个 permission checkbox（与现有样式保持一致）
    - 新用户/guest 默认 `root_permissions: []`
    - roots 列表为空时显示提示："请先在上方添加共享目录"

### i18n (`messages.ts`)

20. 补充新的 en / zh 翻译 key（目录访问区块标题、空状态提示等）

## Acceptance Criteria

- [ ] 新创建用户默认 root_permissions 为空，看不到任何目录
- [ ] 分配目录后，用户只能看到被分配的目录（首页、搜索均过滤）
- [ ] 对未被分配的 node_id 发起任何操作返回 404
- [ ] 不同目录的权限独立生效（A 只读、B 读写）
- [ ] guest 账户同样适用 per-root 权限
- [ ] session 响应 `permissions` 为所有 root 权限的 OR 合并
- [ ] `cargo clippy` 无报错，TypeScript typecheck 通过

## Definition of Done

- `cargo clippy` + `cargo fmt` 无报错
- TypeScript typecheck 通过
- 现有 http.rs 测试更新以适配新 model（test_state 构造函数）

## Technical Approach

### 核心权限查找伪代码

```rust
impl ResolvedPrincipal {
    pub fn permissions_for_root(&self, root_id: &str) -> Option<FileSharePermissionSet> {
        self.root_permissions
            .iter()
            .find(|r| r.root_id == root_id)
            .map(|r| r.permissions.clone())
    }
}

// OR 合并（用于 session 响应）
fn merge_permissions(root_perms: &[UserRootPermissions]) -> FileSharePermissionSet {
    root_perms.iter().fold(
        FileSharePermissionSet::deny_all(),
        |acc, r| acc.or(&r.permissions),
    )
}
```

### Handler 改造模式（统一）

```rust
// 旧
require_request_permission(&state, &headers, addr.ip(), Permission::Delete, false)?;
let node = resolve_node(&state, &node_id)?;

// 新
let principal = resolve_request_principal_only(&state, &headers, addr.ip())?;
let node = find_allowed_node(&state, &principal, &node_id)?;  // 已含 404 check
let perms = principal.permissions_for_root(&node.root.id)
    .ok_or_else(|| 404_response)?;
require_permission_set(&perms, Permission::Delete)?;
```

### 前端 UI 展开式列表结构

```
[ 账户设置区块 ]
  用户名 / 密码 / 启用

[ 目录访问区块 ]
  ── /projects（ProjectX）──────── [✓ 勾选]
     权限：[ 只读 ▼ ]              ← preset 选择
  ── /releases（Release）──────── [✓ 勾选]
     权限：[ 自定义 ▼ ]
       □ 浏览  ✓ 下载文件  □ 下载压缩包
       □ 上传  □ 创建目录  □ 重命名  □ 删除
       ...
  ── /logs（Logs）──────────────── [ ] （未授权，折叠）
```

## Out of Scope

- 子目录级权限控制
- 权限组 / 角色继承
- 运行时不重启即热更新（现有机制已覆盖）

## Technical Notes

### 关键文件位置
- `model.rs` — `PersistedFileShareUser:134`, `FileSharePermissionSet:43`, `default_guest_account:266`
- `auth.rs` — `ResolvedPrincipal:45`, `require_permission:109`
- `mod.rs` — `principal_for_user:760`, `find_root:734`, `resolve_request_principal:828`
- `http.rs` — `build_home_tree_response:897`, `resolve_node:1313`, `find_root(state):734`
- `persist.rs` — `build_persisted_user:234`, `normalize_persisted_user:361`, `permissions_for_preset:261`

### `FileSharePermissionSet` 新增方法
- `fn deny_all() -> Self` — 所有字段 false
- `fn or(&self, other: &Self) -> Self` — 逐字段 OR

### 现有测试适配点
- `http.rs:test_state_with_roots` — 需更新 guest_account 使用新 model
- `http.rs:write_saved_config` — 需更新
- `auth.rs:tests` — `ResolvedPrincipal` 构造需加 `root_permissions`
