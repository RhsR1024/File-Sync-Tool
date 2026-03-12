# 框架默认密码修改 + 侧边栏菜单增强 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在侧边栏新增"其他工具"菜单，实现修改框架默认密码功能，支持批量 IP 下发。

**Architecture:**
- 前端：侧边栏改为树形菜单结构，新建修改密码页面
- 后端：实现 HTTP 客户端调用，完成两步登录→修改密码流程
- 类型层：统一定义请求/响应结构

**Tech Stack:** Vue 3, TypeScript, Tauri 2.x, Rust (reqwest), Tailwind CSS

---

## Chunk 1: 类型定义和路由配置

### Task 1: 添加类型定义到 tauri.ts

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 在 tauri.ts 末尾添加密码修改相关类型定义**

在 `src/lib/tauri.ts` 最后添加以下代码：

```typescript
// Framework password management types
export interface FrameworkPasswordResult {
  ip: string;
  success: boolean;
  message: string;
  stage: 'login' | 'changePasswd'; // 失败在哪一步
}

export interface ChangeFrameworkPasswordResponse {
  results: FrameworkPasswordResult[];
}

// Internal: Login API response
export interface LoginResponse {
  code: number;
  message: string;
  data?: {
    firstLogin: boolean;
    token: string;
  };
}

// Internal: ChangePasswd API response
export interface ChangePasswdResponse {
  code: number;
  message: string;
}
```

- [ ] **Step 2: 在 tauri.ts 中添加新 Command 的 invoke 函数**

在文件最后添加：

```typescript
export async function changeFrameworkPassword(ips: string[]): Promise<FrameworkPasswordResult[]> {
  return invoke<FrameworkPasswordResult[]>('change_framework_password', { ips });
}
```

- [ ] **Step 3: 提交改动**

```bash
git add src/lib/tauri.ts
git commit -m "types: 添加框架默认密码修改的类型定义"
```

---

### Task 2: 添加路由配置

**Files:**
- Modify: `src/router/index.ts`

- [ ] **Step 1: 查看当前路由结构**

```bash
head -50 c:/WorkSpace/Copy/src/router/index.ts
```

- [ ] **Step 2: 添加新路由**

在路由数组中添加以下路由（插入在其他路由之后）：

```typescript
{
  path: '/tools/framework-password',
  component: () => import('../pages/FrameworkPasswordPage.vue'),
},
```

完整示例（假设路由文件结构）：编辑 `src/router/index.ts`，在 routes 数组中找到其他页面路由，之后添加上述路由。

- [ ] **Step 3: 提交改动**

```bash
git add src/router/index.ts
git commit -m "feat: 添加框架默认密码修改路由"
```

---

### Task 3: 添加 i18n 翻译

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: 查看现有翻译结构**

打开 `src/locales/messages.ts` 查看现有结构。

- [ ] **Step 2: 添加新翻译文本**

在消息对象中添加以下中英翻译。假设现有结构如下，需要在侧边栏和工具页面的翻译中添加：

```typescript
export const messages = {
  en: {
    // ... existing translations ...
    sidebar: {
      // ... existing items ...
      tools: 'Other Tools',
      frameworkPassword: 'Framework Password',
      codeStatistics: 'Code Statistics',
    },
    tools: {
      frameworkPassword: {
        title: 'Change Framework Default Password',
        ipLabel: 'IP Addresses',
        ipPlaceholder: 'Enter IP addresses (one per line)',
        oldPasswordLabel: 'Old Password Hash',
        newPasswordLabel: 'New Password Hash',
        executeButton: 'Start Change',
        results: 'Results',
        status: 'Status',
        message: 'Message',
        success: 'Success',
        failed: 'Failed',
        loginFailed: 'Login failed: {message}',
        changePasswdFailed: 'Change password failed: {message}',
        invalidIp: 'Invalid IP address: {ip}',
        noIps: 'Please enter at least one IP address',
        progress: 'Processing: {current}/{total}',
      },
    },
  },
  zh: {
    // ... existing translations ...
    sidebar: {
      // ... existing items ...
      tools: '其他工具',
      frameworkPassword: '修改框架默认密码',
      codeStatistics: '代码统计',
    },
    tools: {
      frameworkPassword: {
        title: '修改框架默认密码',
        ipLabel: 'IP 地址',
        ipPlaceholder: '输入 IP 地址（每行一个）',
        oldPasswordLabel: '旧密码哈希',
        newPasswordLabel: '新密码哈希',
        executeButton: '开始修改',
        results: '结果',
        status: '状态',
        message: '信息',
        success: '成功',
        failed: '失败',
        loginFailed: '登录失败：{message}',
        changePasswdFailed: '修改密码失败：{message}',
        invalidIp: '无效的 IP 地址：{ip}',
        noIps: '请输入至少一个 IP 地址',
        progress: '处理中：{current}/{total}',
      },
    },
  },
};
```

- [ ] **Step 3: 提交改动**

```bash
git add src/locales/messages.ts
git commit -m "i18n: 添加框架密码修改和侧边栏工具菜单翻译"
```

---

## Chunk 2: 后端实现

### Task 4: 添加 reqwest 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 查看 Cargo.toml 现有依赖**

```bash
grep -A 20 "\[dependencies\]" c:/WorkSpace/Copy/src-tauri/Cargo.toml
```

- [ ] **Step 2: 添加 reqwest 依赖**

编辑 `src-tauri/Cargo.toml`，在 `[dependencies]` 部分添加：

```toml
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1", features = ["full"] }
serde_json = "1.0"
```

确保 tokio 已经存在且包含 "full" 特性。

- [ ] **Step 3: 提交改动**

```bash
git add src-tauri/Cargo.toml
git commit -m "deps: 添加 reqwest 用于 HTTP 请求"
```

---

### Task 5: 实现后端密码修改命令

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: 在 main.rs 中添加密码修改命令**

找到所有 Command 注册的位置（通常在 `fn main()` 或 `invoke_handler` 中），添加以下函数（在 main.rs 中任意位置）：

```rust
#[tauri::command]
async fn change_framework_password(ips: Vec<String>) -> Result<Vec<PasswordChangeResult>, String> {
    use serde_json::json;

    const OLD_PASSWORD_HASH: &str = "8d969eef6ecad3c29a3a629280e686cf0c3f5d5a86aff3ca12020c923adc6c92";
    const NEW_PASSWORD_HASH: &str = "4d5c5f61bb3d2c299d3211c2992a28a7849b6ce933919c399ce24903c1715d45";

    let mut results = Vec::new();
    let client = reqwest::Client::new();

    for ip in ips.iter() {
        let ip = ip.trim();
        if ip.is_empty() {
            continue;
        }

        // Validate IP format (basic check)
        if !validate_ip(ip) {
            results.push(PasswordChangeResult {
                ip: ip.to_string(),
                success: false,
                message: format!("Invalid IP address: {}", ip),
                stage: "login".to_string(),
            });
            continue;
        }

        // Step 1: Login
        let login_url = format!("http://{}:21900/openAPI/userMgr/v1/login", ip);
        let login_body = json!({
            "userName": "admin",
            "userPasswd": OLD_PASSWORD_HASH,
            "isUnlockLogin": false
        });

        let token = match client
            .post(&login_url)
            .header("Authorization", "ab94186a-165b-4a18-9337-a9e33809d592")
            .header("content-type", "application/json")
            .json(&login_body)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        // Validate response structure
                        if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                            if let Some(token) = json.get("data").and_then(|d| d.get("token")).and_then(|t| t.as_str()) {
                                token.to_string()
                            } else {
                                results.push(PasswordChangeResult {
                                    ip: ip.to_string(),
                                    success: false,
                                    message: "Login response missing token".to_string(),
                                    stage: "login".to_string(),
                                });
                                continue;
                            }
                        } else {
                            let msg = json
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error");
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: false,
                                message: format!("Login failed: {}", msg),
                                stage: "login".to_string(),
                            });
                            continue;
                        }
                    }
                    Err(e) => {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Login response parse error: {}", e),
                            stage: "login".to_string(),
                        });
                        continue;
                    }
                }
            }
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("Login request failed: {}", e),
                    stage: "login".to_string(),
                });
                continue;
            }
        };

        // Step 2: Change Password
        let change_passwd_url = format!("http://{}:21900/openAPI/userMgr/v1/changePasswd", ip);
        let change_passwd_body = json!({
            "userName": "admin",
            "oldUserPasswd": OLD_PASSWORD_HASH,
            "newUserPasswd": NEW_PASSWORD_HASH
        });

        match client
            .post(&change_passwd_url)
            .header("Authorization", &token)
            .header("content-type", "application/json")
            .json(&change_passwd_body)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        // Validate response structure
                        if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: true,
                                message: "Success".to_string(),
                                stage: "changePasswd".to_string(),
                            });
                        } else {
                            let msg = json
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error");
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: false,
                                message: format!("Change password failed: {}", msg),
                                stage: "changePasswd".to_string(),
                            });
                        }
                    }
                    Err(e) => {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Change password response parse error: {}", e),
                            stage: "changePasswd".to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("Change password request failed: {}", e),
                    stage: "changePasswd".to_string(),
                });
            }
        }
    }

    Ok(results)
}

// Helper function to validate IP address format
fn validate_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        part.parse::<u8>().is_ok()
    })
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PasswordChangeResult {
    pub ip: String,
    pub success: bool,
    pub message: String,
    pub stage: String,
}
```

- [ ] **Step 2: 在 tauri builder 中注册命令**

找到 tauri app 的 invoke_handler 或 .invoke_handler()，添加 `change_framework_password`。通常看起来像：

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_config_cmd,
    scan_now,
    // ... other commands ...
    change_framework_password, // 添加这一行
])
```

- [ ] **Step 3: 添加必要的 imports**

在 main.rs 顶部添加（如果还没有）：

```rust
use serde_json::json;
```

- [ ] **Step 4: 验证编译**

```bash
cd c:/WorkSpace/Copy
cargo check
```

期望：编译无错误和警告。

- [ ] **Step 5: 提交改动**

```bash
git add src-tauri/src/main.rs src-tauri/Cargo.toml
git commit -m "feat: 实现 change_framework_password 命令"
```

---

## Chunk 3: 前端 UI 实现

### Task 6: 更新侧边栏组件支持树形菜单

**Files:**
- Modify: `src/components/Sidebar.vue`

- [ ] **Step 1: 读取当前侧边栏代码**

查看完整的 `src/components/Sidebar.vue` 文件（已在前面读过）。

- [ ] **Step 2: 改造侧边栏支持树形菜单**

替换整个 `src/components/Sidebar.vue` 为以下代码：

```vue
<script setup lang="ts">
import { Settings, Activity, Server, ShieldCheck, History, ListChecks, ChevronDown } from 'lucide-vue-next';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { computed, ref } from 'vue';

const route = useRoute();
const { t } = useI18n();
const expandedMenus = ref<Record<string, boolean>>({ tools: false });

interface MenuItem {
  name: string;
  path?: string;
  icon?: any;
  children?: MenuItem[];
  id?: string;
}

const menuItems = computed<MenuItem[]>(() => [
  { name: t('sidebar.tasks'), path: '/tasks', icon: ListChecks },
  { name: t('sidebar.console'), path: '/', icon: Activity },
  { name: t('sidebar.history'), path: '/history', icon: History },
  { name: t('sidebar.settings'), path: '/settings', icon: Settings },
  {
    id: 'tools',
    name: t('sidebar.tools'),
    icon: Server,
    children: [
      { name: t('sidebar.frameworkPassword'), path: '/tools/framework-password' },
      { name: t('sidebar.codeStatistics'), path: '/tools/code-statistics' },
    ],
  },
]);

const toggleMenu = (id: string) => {
  expandedMenus.value[id] = !expandedMenus.value[id];
};

const isRouteActive = (path?: string) => {
  if (!path) return false;
  return route.path === path || route.path.startsWith(path + '/');
};
</script>

<template>
  <div class="w-56 bg-[#0f172a] text-white h-screen flex flex-col border-r border-slate-800 shadow-xl z-10">
    <div class="p-6 border-b border-slate-800 bg-slate-900/50">
      <h1 class="text-lg font-bold flex items-center gap-3 tracking-tight">
        <div class="w-8 h-8 bg-blue-600 rounded-md flex items-center justify-center shadow-lg shadow-blue-500/20 shrink-0">
          <Server class="w-5 h-5 text-white" />
        </div>
        <span class="bg-gradient-to-r from-blue-400 to-cyan-300 bg-clip-text text-transparent truncate">
          {{ t('sidebar.title') }}
        </span>
      </h1>
    </div>

    <nav class="flex-1 p-4 space-y-2 overflow-y-auto">
      <template v-for="item in menuItems" :key="item.path || item.id">
        <!-- Regular menu item (no children) -->
        <router-link
          v-if="!item.children"
          :to="item.path!"
          class="group flex items-center gap-3 px-4 py-3 rounded-md transition-all duration-200 border border-transparent"
          :class="isRouteActive(item.path)
            ? 'bg-blue-600/10 text-blue-400 border-blue-500/20 shadow-sm'
            : 'text-slate-400 hover:bg-slate-800/50 hover:text-slate-200'"
        >
          <component
            :is="item.icon"
            class="w-5 h-5 transition-transform group-hover:scale-110"
            :class="isRouteActive(item.path) ? 'text-blue-400' : 'text-slate-500 group-hover:text-slate-300'"
          />
          <span class="font-medium tracking-wide">{{ item.name }}</span>
          <div v-if="isRouteActive(item.path)" class="ml-auto w-1.5 h-1.5 rounded-full bg-blue-400"></div>
        </router-link>

        <!-- Expandable menu item (with children) -->
        <div v-else class="space-y-1">
          <button
            @click="toggleMenu(item.id!)"
            class="w-full group flex items-center gap-3 px-4 py-3 rounded-md transition-all duration-200 border border-transparent text-slate-400 hover:bg-slate-800/50 hover:text-slate-200"
          >
            <component
              :is="item.icon"
              class="w-5 h-5 transition-transform group-hover:scale-110 text-slate-500 group-hover:text-slate-300"
            />
            <span class="font-medium tracking-wide">{{ item.name }}</span>
            <ChevronDown
              class="ml-auto w-4 h-4 transition-transform"
              :class="{ 'rotate-180': expandedMenus[item.id!] }"
            />
          </button>

          <!-- Children items -->
          <transition name="slide">
            <div v-show="expandedMenus[item.id!]" class="pl-2 space-y-1">
              <router-link
                v-for="child in item.children"
                :key="child.path"
                :to="child.path!"
                class="group flex items-center gap-3 px-4 py-2 rounded-md transition-all duration-200 border border-transparent text-sm"
                :class="isRouteActive(child.path)
                  ? 'bg-blue-600/10 text-blue-400 border-blue-500/20 shadow-sm'
                  : 'text-slate-400 hover:bg-slate-800/50 hover:text-slate-200'"
              >
                <div class="w-1.5 h-1.5 rounded-full bg-current"></div>
                <span class="font-medium tracking-wide">{{ child.name }}</span>
                <div v-if="isRouteActive(child.path)" class="ml-auto w-1 h-1 rounded-full bg-blue-400"></div>
              </router-link>
            </div>
          </transition>
        </div>
      </template>
    </nav>

    <div class="p-6 border-t border-slate-800 bg-slate-900/30">
      <div class="flex items-center gap-3 text-xs text-slate-500 font-mono">
        <ShieldCheck class="w-4 h-4" />
        <span>{{ t('sidebar.version') }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.slide-enter-active,
.slide-leave-active {
  transition: all 0.3s ease;
}

.slide-enter-from {
  opacity: 0;
  max-height: 0;
}

.slide-leave-to {
  opacity: 0;
  max-height: 0;
}
</style>
```

- [ ] **Step 3: 提交改动**

```bash
git add src/components/Sidebar.vue
git commit -m "feat: 升级侧边栏支持树形展开菜单"
```

---

### Task 7: 创建修改框架密码页面

**Files:**
- Create: `src/pages/FrameworkPasswordPage.vue`

- [ ] **Step 1: 创建新页面组件**

创建文件 `src/pages/FrameworkPasswordPage.vue`，内容如下：

```vue
<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, CheckCircle2, Loader } from 'lucide-vue-next';
import { changeFrameworkPassword } from '../lib/tauri';
import type { FrameworkPasswordResult } from '../lib/tauri';

const { t } = useI18n();

const ipInput = ref<string>('');
const isLoading = ref<boolean>(false);
const results = ref<FrameworkPasswordResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);

const OLD_PASSWORD_HASH = '8d969eef6ecad3c29a3a629280e686cf0c3f5d5a86aff3ca12020c923adc6c92';
const NEW_PASSWORD_HASH = '4d5c5f61bb3d2c299d3211c2992a28a7849b6ce933919c399ce24903c1715d45';

const ips = computed(() => {
  return ipInput.value
    .split(/[\n,]/)
    .map(ip => ip.trim())
    .filter(ip => ip.length > 0);
});

const isFormValid = computed(() => {
  return ips.value.length > 0 && !isLoading.value;
});

const handleExecute = async () => {
  if (ips.value.length === 0) {
    alert(t('tools.frameworkPassword.noIps'));
    return;
  }

  isLoading.value = true;
  results.value = [];

  try {
    const ipList = ips.value;
    currentProgress.value = { current: 0, total: ipList.length };

    const response = await changeFrameworkPassword(ipList);
    results.value = response;
    currentProgress.value = { current: ipList.length, total: ipList.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = ips.value.map(ip => ({
      ip,
      success: false,
      message: `Error: ${errorMessage}`,
      stage: 'login',
    }));
  } finally {
    isLoading.value = false;
    currentProgress.value = null;
  }
};

const successCount = computed(() => results.value.filter(r => r.success).length);
const failureCount = computed(() => results.value.filter(r => !r.success).length);
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-900 via-slate-800 to-slate-900 p-8">
    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-white mb-2">{{ t('tools.frameworkPassword.title') }}</h1>
      <p class="text-slate-400">{{ t('tools.frameworkPassword.description', 'Modify the default password of the framework') }}</p>
    </div>

    <!-- Main Content -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-8">
      <!-- Input Section -->
      <div class="lg:col-span-2 space-y-6">
        <!-- IP Input Card -->
        <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-6 backdrop-blur-sm hover:border-slate-600 transition-colors">
          <label class="block text-sm font-semibold text-white mb-3">
            {{ t('tools.frameworkPassword.ipLabel') }}
          </label>
          <textarea
            v-model="ipInput"
            :placeholder="t('tools.frameworkPassword.ipPlaceholder')"
            :disabled="isLoading"
            class="w-full h-32 bg-slate-900/50 border border-slate-600 rounded px-4 py-2 text-white placeholder-slate-500 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 disabled:opacity-50 disabled:cursor-not-allowed font-mono"
          />
          <div class="mt-2 text-xs text-slate-400">
            {{ ips.length }} IP {{ ips.length === 1 ? 'address' : 'addresses' }}
          </div>
        </div>

        <!-- Password Info Cards -->
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-4 backdrop-blur-sm">
            <label class="block text-xs font-semibold text-white mb-2">
              {{ t('tools.frameworkPassword.oldPasswordLabel') }}
            </label>
            <div class="bg-slate-900/50 border border-slate-600 rounded px-3 py-2 font-mono text-xs text-slate-300 break-all">
              {{ OLD_PASSWORD_HASH }}
            </div>
          </div>
          <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-4 backdrop-blur-sm">
            <label class="block text-xs font-semibold text-white mb-2">
              {{ t('tools.frameworkPassword.newPasswordLabel') }}
            </label>
            <div class="bg-slate-900/50 border border-slate-600 rounded px-3 py-2 font-mono text-xs text-slate-300 break-all">
              {{ NEW_PASSWORD_HASH }}
            </div>
          </div>
        </div>

        <!-- Execute Button -->
        <button
          @click="handleExecute"
          :disabled="!isFormValid"
          class="w-full px-6 py-3 bg-gradient-to-r from-blue-600 to-cyan-600 text-white font-semibold rounded-lg hover:from-blue-700 hover:to-cyan-700 focus:outline-none focus:ring-2 focus:ring-blue-500/50 disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2"
        >
          <Loader v-if="isLoading" class="w-5 h-5 animate-spin" />
          <span>{{ isLoading ? 'Processing...' : t('tools.frameworkPassword.executeButton') }}</span>
        </button>
      </div>

      <!-- Stats Card -->
      <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-6 backdrop-blur-sm h-fit sticky top-8">
        <h3 class="text-sm font-semibold text-white mb-4">{{ t('tools.frameworkPassword.results') }}</h3>

        <div class="space-y-3">
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Total:</span>
            <span class="text-white font-semibold">{{ results.length }}</span>
          </div>
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Success:</span>
            <span class="text-green-400 font-semibold">{{ successCount }}</span>
          </div>
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Failed:</span>
            <span class="text-red-400 font-semibold">{{ failureCount }}</span>
          </div>

          <div v-if="currentProgress" class="mt-6 pt-4 border-t border-slate-700">
            <div class="text-xs text-slate-400 mb-2">
              {{ t('tools.frameworkPassword.progress', `Processing: ${currentProgress.current}/${currentProgress.total}`) }}
            </div>
            <div class="w-full bg-slate-900/50 rounded-full h-2">
              <div
                class="bg-gradient-to-r from-blue-500 to-cyan-500 h-2 rounded-full transition-all duration-300"
                :style="{ width: `${(currentProgress.current / currentProgress.total) * 100}%` }"
              ></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Results Table -->
    <div v-if="results.length > 0" class="mt-8">
      <div class="bg-slate-800/50 border border-slate-700 rounded-lg overflow-hidden backdrop-blur-sm">
        <div class="overflow-x-auto">
          <table class="w-full">
            <thead>
              <tr class="border-b border-slate-700 bg-slate-900/50">
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">IP</th>
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">{{ t('tools.frameworkPassword.status') }}</th>
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">{{ t('tools.frameworkPassword.message') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="result in results" :key="result.ip" class="border-b border-slate-700 hover:bg-slate-700/20 transition-colors">
                <td class="px-6 py-3 text-sm font-mono text-white">{{ result.ip }}</td>
                <td class="px-6 py-3">
                  <div class="flex items-center gap-2">
                    <component
                      :is="result.success ? CheckCircle2 : AlertCircle"
                      :class="result.success ? 'text-green-400' : 'text-red-400'"
                      class="w-4 h-4"
                    />
                    <span :class="result.success ? 'text-green-400' : 'text-red-400'" class="text-sm font-semibold">
                      {{ result.success ? t('tools.frameworkPassword.success') : t('tools.frameworkPassword.failed') }}
                    </span>
                  </div>
                </td>
                <td class="px-6 py-3 text-sm text-slate-300">{{ result.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 2: 提交改动**

```bash
git add src/pages/FrameworkPasswordPage.vue
git commit -m "feat: 创建修改框架默认密码页面"
```

---

## Chunk 4: 集成和验证

### Task 8: 验证整体集成

- [ ] **Step 1: 验证前端项目能够编译**

```bash
cd c:/WorkSpace/Copy
pnpm install
pnpm run dev
```

期望：应用无错误启动，可以看到新增的"其他工具"菜单项。

- [ ] **Step 2: 验证路由能够访问**

在浏览器中导航到 `http://localhost:5173/tools/framework-password`（或相应的开发端口），应该看到新页面。

- [ ] **Step 3: 验证后端编译**

```bash
cd c:/WorkSpace/Copy
cargo check
```

期望：无编译错误。

- [ ] **Step 4: 验证完整构建**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

期望：构建成功，生成 `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe` 文件。

- [ ] **Step 5: 手动测试密码修改功能**

（如果有测试环境）
- 启动应用
- 点击侧边栏"其他工具" → "修改框架默认密码"
- 输入 IP 地址（可输入多个）
- 点击"开始修改"
- 观察结果表格显示正确的成功/失败状态

- [ ] **Step 6: 最终提交**

```bash
git add -A
git commit -m "feat: 完成框架密码修改和侧边栏菜单增强

- 升级侧边栏支持树形展开菜单
- 新增'其他工具'菜单项（可扩展）
- 实现修改框架默认密码页面（支持批量 IP）
- 后端实现两步 API 流程（登录→修改密码）
- 添加完整的错误处理和进度显示
- 更新 i18n 翻译和路由配置"
```

---

## 计划总结

| 任务 | 文件 | 功能 |
|------|------|------|
| 1 | tauri.ts | 类型定义 + invoke 函数 |
| 2 | router/index.ts | 添加路由 |
| 3 | locales/messages.ts | i18n 翻译 |
| 4 | Cargo.toml | 添加 reqwest 依赖 |
| 5 | main.rs | 实现 change_framework_password 命令 |
| 6 | Sidebar.vue | 树形菜单支持 |
| 7 | FrameworkPasswordPage.vue | 新页面（IP 输入、结果显示） |
| 8 | 集成验证 | 编译、路由、构建、测试 |

**预期工作量：** 2-3 小时（包括测试）

**关键风险：**
- HTTP 请求超时：需要添加超时配置
- 响应校验不严格：目前仅检查 `code: 0`，可根据实际 API 文档调整
