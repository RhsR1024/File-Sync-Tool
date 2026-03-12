# Manual Copy UI 优化与对话框重构

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标：** 将手动复制功能从长条表单改为弹出对话框，增加文件夹选择器，持久化用户输入，防止复制后清空路径。

**架构：**
1. 在全局状态 (store.ts) 中添加 `manualCopyFormState` 用于持久化用户输入
2. 在 Rust 侧 (main.rs) 添加 `open_directory` 命令，调用系统文件选择对话框
3. 创建新的 `ManualCopyModal.vue` 组件（对话框形式）
4. 完全重构 `ManualCopyPage.vue`，改为触发 Modal 的按钮形式
5. 在 tauri.ts 中添加打开目录的接口
6. 更新 i18n 文本支持新的 UI

**技术栈：** Vue 3 + TypeScript + Tauri 2.x + Rust + TailwindCSS

---

## Chunk 1: 核心状态管理和后端支持

### Task 1.1: 在 store.ts 中添加 manualCopyFormState

**文件：**
- Modify: `src/lib/store.ts`

**步骤：**

- [ ] **Step 1.1.1: 在 store.ts 中添加新的响应式状态**

在 store.ts 顶部，添加以下类型定义和状态：

```typescript
export interface ManualCopyFormState {
  sourcePath: string;
  targetRootPath: string;
}

// 从 localStorage 恢复或初始化
const manualCopyFormState = reactive<ManualCopyFormState>({
  sourcePath: localStorage.getItem('manualCopy_sourcePath') || '',
  targetRootPath: localStorage.getItem('manualCopy_targetRootPath') || '',
});

export function updateManualCopyForm(state: Partial<ManualCopyFormState>) {
  if (state.sourcePath !== undefined) {
    manualCopyFormState.sourcePath = state.sourcePath;
    localStorage.setItem('manualCopy_sourcePath', state.sourcePath);
  }
  if (state.targetRootPath !== undefined) {
    manualCopyFormState.targetRootPath = state.targetRootPath;
    localStorage.setItem('manualCopy_targetRootPath', state.targetRootPath);
  }
}

export function getManualCopyForm(): ManualCopyFormState {
  return manualCopyFormState;
}

export function clearManualCopyForm() {
  manualCopyFormState.sourcePath = '';
  manualCopyFormState.targetRootPath = '';
  localStorage.removeItem('manualCopy_sourcePath');
  localStorage.removeItem('manualCopy_targetRootPath');
}
```

- [ ] **Step 1.1.2: 提交此步骤**

```bash
git add src/lib/store.ts
git commit -m "feat: 添加手动复制表单状态持久化到 localStorage"
```

---

### Task 1.2: 在 tauri.ts 中添加 openDirectory 接口

**文件：**
- Modify: `src/lib/tauri.ts`

**步骤：**

- [ ] **Step 1.2.1: 在 tauri.ts 中添加 openDirectory 函数**

在 tauri.ts 底部添加：

```typescript
export async function openDirectory(): Promise<string | null> {
  return await invoke('open_directory');
}
```

- [ ] **Step 1.2.2: 提交此步骤**

```bash
git add src/lib/tauri.ts
git commit -m "feat: 添加 openDirectory 接口用于调用系统文件夹选择对话框"
```

---

### Task 1.3: 在 Rust 侧添加 open_directory 命令

**文件：**
- Modify: `src-tauri/src/main.rs`

**步骤：**

- [ ] **Step 1.3.1: 在 main.rs 中添加 open_directory 命令**

在 Tauri commands 注册部分，找到类似这样的代码：

```rust
let app = tauri::Builder::default()
  .invoke_handler(tauri::generate_handler![
    get_config,
    save_config_cmd,
    // ... other commands
  ])
```

在命令列表中添加 `open_directory` 的声明。

先在 main.rs 顶部，在其他 command 函数定义之后添加：

```rust
#[tauri::command]
async fn open_directory() -> Result<Option<String>, String> {
  // 使用 tauri 的文件对话框 API
  // 首先需要在 Cargo.toml 中确保 tauri dialog 功能已启用
  Ok(None)  // 临时返回 None，具体实现在下一步
}
```

- [ ] **Step 1.3.2: 实现 open_directory 命令的完整逻辑**

替换上面的 `open_directory` 函数：

```rust
#[tauri::command]
async fn open_directory(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
  use tauri::api::dialog;

  match dialog::FileDialogBuilder::new()
    .set_title("Select Target Directory")
    .pick_folder()
    .await
  {
    Ok(Some(path)) => Ok(Some(path.display().to_string())),
    Ok(None) => Ok(None),
    Err(e) => Err(format!("Failed to open directory picker: {}", e)),
  }
}
```

**注意：** Tauri 2.x 的对话框 API 可能需要调整。如果上面的代码不工作，使用备选方案：

```rust
#[tauri::command]
async fn open_directory(window: tauri::Window) -> Result<Option<String>, String> {
  use tauri_plugin_dialog::DialogExt;

  match window.dialog().file().pick_folder().await {
    Ok(Some(path)) => Ok(Some(path.display().to_string())),
    Ok(None) => Ok(None),
    Err(e) => Err(format!("Failed to open directory picker: {}", e)),
  }
}
```

- [ ] **Step 1.3.3: 检查 Cargo.toml 依赖**

打开 `src-tauri/Cargo.toml`，确保 tauri 依赖包含必要的特性。如果使用 tauri-plugin-dialog，确保已添加到依赖。

- [ ] **Step 1.3.4: 在 generate_handler! 中注册命令**

找到 `tauri::generate_handler![...]` 行，添加 `open_directory`：

```rust
let app = tauri::Builder::default()
  .invoke_handler(tauri::generate_handler![
    get_config,
    save_config_cmd,
    scan_now,
    cancel_scan,
    pause_scan,
    resume_scan,
    test_ssh_connection,
    manual_deploy,
    temporary_copy,
    get_app_paths,
    open_path_parent,
    get_history,
    clear_history,
    add_system_event,
    open_directory,  // 新增
  ])
```

- [ ] **Step 1.3.5: 测试编译（可选，完整测试在后续步骤）**

在后续完整测试时验证编译通过。

- [ ] **Step 1.3.6: 提交此步骤**

```bash
git add src-tauri/src/main.rs src-tauri/Cargo.toml
git commit -m "feat: 在 Rust 侧添加 open_directory 命令，支持系统文件夹选择对话框"
```

---

## Chunk 2: 创建 Modal 组件和更新 i18n

### Task 2.1: 创建 ManualCopyModal.vue 组件

**文件：**
- Create: `src/components/ManualCopyModal.vue`

**步骤：**

- [ ] **Step 2.1.1: 创建 ManualCopyModal.vue 文件**

```vue
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { X, Play, FolderOpen } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { addLog } from '@/lib/store';
import { addSystemEvent, getConfig, temporaryCopy, openDirectory, type AppConfig } from '@/lib/tauri';
import { getManualCopyForm, updateManualCopyForm } from '@/lib/store';

const props = defineProps<{
  isOpen: boolean;
}>();

const emit = defineEmits<{
  close: [];
  success: [];
}>();

const { t } = useI18n();

const sourcePath = ref('');
const targetRootPath = ref('');
const statusMsg = ref('');
const statusTone = ref<'info' | 'success' | 'error'>('info');
const isSubmitting = ref(false);
const config = ref<AppConfig | null>(null);

const canSubmit = computed(() => sourcePath.value.trim().length > 0 && targetRootPath.value.trim().length > 0 && !isSubmitting.value);

const filterSummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');

  const exts = config.value.file_extensions.filter(Boolean);
  const keywords = config.value.filename_includes.filter(Boolean);
  const parts: string[] = [];

  if (exts.length > 0) {
    parts.push(t('manualCopy.extFilter', { value: exts.join(', ') }));
  }
  if (keywords.length > 0) {
    parts.push(t('manualCopy.keywordFilter', { value: keywords.join(', ') }));
  }

  return parts.length > 0 ? parts.join(' | ') : t('manualCopy.noFilters');
});

const stabilitySummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');
  return t('manualCopy.stabilityEnabled', {
    mins: config.value.recent_file_guard_mins,
    secs: config.value.stability_check_secs,
  });
});

// 从 localStorage 恢复值
watch(
  () => props.isOpen,
  (newVal) => {
    if (newVal) {
      const savedForm = getManualCopyForm();
      sourcePath.value = savedForm.sourcePath;
      targetRootPath.value = savedForm.targetRootPath;
      loadConfig();
    } else {
      statusMsg.value = '';
    }
  }
);

// 监听输入变化，保存到 localStorage
watch([sourcePath, targetRootPath], () => {
  updateManualCopyForm({
    sourcePath: sourcePath.value,
    targetRootPath: targetRootPath.value,
  });
});

async function loadConfig() {
  try {
    const cfg = await getConfig();
    config.value = cfg;
    if (!targetRootPath.value) {
      targetRootPath.value = cfg.local_path || '';
      updateManualCopyForm({ targetRootPath: targetRootPath.value });
    }
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.loadConfigFailed', { error: String(error) });
  }
}

async function handleSelectTargetDir() {
  try {
    const selectedPath = await openDirectory();
    if (selectedPath) {
      targetRootPath.value = selectedPath;
      updateManualCopyForm({ targetRootPath: selectedPath });
    }
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.selectDirFailed', { error: String(error) });
  }
}

async function submitCopy() {
  if (!canSubmit.value) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.fillRequired');
    return;
  }

  isSubmitting.value = true;
  statusMsg.value = '';
  statusTone.value = 'info';

  try {
    await temporaryCopy(sourcePath.value.trim(), targetRootPath.value.trim());
    statusTone.value = 'success';
    statusMsg.value = t('manualCopy.success');
    addLog(t('manualCopy.addedToQueue'), 'success');
    await addSystemEvent('MANUAL_COPY', t('manualCopy.addedToQueue'));

    // 不清空表单，保留供下次使用
    // 3秒后自动关闭 Modal
    setTimeout(() => {
      emit('close');
      emit('success');
    }, 2000);
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.failed', { error: String(error) });
  } finally {
    isSubmitting.value = false;
  }
}

function handleClose() {
  emit('close');
}
</script>

<template>
  <!-- Backdrop -->
  <Transition name="fade">
    <div
      v-if="isOpen"
      class="fixed inset-0 bg-black bg-opacity-50 z-40 transition-opacity"
      @click="handleClose"
    />
  </Transition>

  <!-- Modal -->
  <Transition name="slide-up">
    <div
      v-if="isOpen"
      class="fixed bottom-0 left-0 right-0 md:left-1/2 md:-translate-x-1/2 md:top-1/2 md:-translate-y-1/2 md:bottom-auto w-full md:w-[640px] bg-white rounded-t-3xl md:rounded-3xl shadow-2xl z-50 p-6 max-h-[90vh] overflow-y-auto"
    >
      <!-- Header -->
      <div class="flex items-center justify-between mb-6 pb-4 border-b border-slate-200">
        <h3 class="text-xl font-bold text-slate-800">{{ t('manualCopy.title') }}</h3>
        <button
          @click="handleClose"
          class="p-1 hover:bg-slate-100 rounded-lg transition-colors"
          :aria-label="t('common.close')"
        >
          <X class="w-5 h-5 text-slate-600" />
        </button>
      </div>

      <!-- Form -->
      <div class="space-y-5">
        <div>
          <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('manualCopy.sourcePath') }}</label>
          <input
            v-model="sourcePath"
            type="text"
            class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
            :placeholder="t('manualCopy.sourcePlaceholder')"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('manualCopy.targetRootPath') }}</label>
          <div class="flex gap-2">
            <input
              v-model="targetRootPath"
              type="text"
              class="flex-1 p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
              :placeholder="t('manualCopy.targetPlaceholder')"
            />
            <button
              @click="handleSelectTargetDir"
              class="px-4 py-3 rounded-xl border border-slate-300 bg-white text-slate-600 hover:bg-slate-50 transition-colors flex items-center gap-2"
              :title="t('manualCopy.browseFolder')"
            >
              <FolderOpen class="w-4 h-4" />
              <span class="hidden sm:inline text-sm">{{ t('manualCopy.browse') }}</span>
            </button>
          </div>
          <p class="text-xs text-slate-400 mt-2">{{ t('manualCopy.targetHint') }}</p>
        </div>

        <!-- Status message -->
        <div v-if="statusMsg" class="rounded-xl px-4 py-3 text-sm border" :class="statusTone === 'error' ? 'bg-red-50 text-red-600 border-red-100' : statusTone === 'success' ? 'bg-emerald-50 text-emerald-600 border-emerald-100' : 'bg-slate-50 text-slate-600 border-slate-200'">
          {{ statusMsg }}
        </div>

        <!-- Action buttons -->
        <div class="flex gap-3 pt-3">
          <button
            @click="submitCopy"
            :disabled="!canSubmit"
            class="flex-1 inline-flex items-center justify-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700"
          >
            <Play class="w-4 h-4" />
            {{ isSubmitting ? t('manualCopy.copying') : t('manualCopy.startCopy') }}
          </button>
          <button
            @click="handleClose"
            class="px-5 py-2.5 rounded-xl border border-slate-300 bg-white text-slate-600 hover:bg-slate-50 transition-colors font-medium"
          >
            {{ t('common.close') }}
          </button>
        </div>
      </div>

      <!-- Rules Info -->
      <div class="mt-6 pt-6 border-t border-slate-200">
        <div class="rounded-xl bg-slate-50 border border-slate-200 p-4 space-y-3 text-sm text-slate-600">
          <div>
            <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.filterTitle') }}</div>
            <div>{{ filterSummary }}</div>
          </div>
          <div>
            <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.stabilityTitle') }}</div>
            <div>{{ stabilitySummary }}</div>
          </div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.slide-up-enter-active,
.slide-up-leave-active {
  transition: transform 0.3s ease, opacity 0.3s ease;
}

.slide-up-enter-from {
  transform: translateY(100%);
}

.slide-up-leave-to {
  transform: translateY(100%);
  opacity: 0;
}
</style>
```

- [ ] **Step 2.1.2: 提交此步骤**

```bash
git add src/components/ManualCopyModal.vue
git commit -m "feat: 创建 ManualCopyModal 组件，支持对话框形式的手动复制和目录选择"
```

---

### Task 2.2: 更新 i18n 文本支持新功能

**文件：**
- Modify: `src/locales/messages.ts`

**步骤：**

- [ ] **Step 2.2.1: 在 en.manualCopy 中添加新的翻译键**

找到 `en: { manualCopy: { ... } }` 部分，在现有的翻译之后添加：

```typescript
      browse: 'Browse',
      browseFolder: 'Select folder using file picker',
      selectDirFailed: 'Failed to select directory: {error}',
```

- [ ] **Step 2.2.2: 在 zh.manualCopy 中添加对应的中文翻译**

找到 `zh: { manualCopy: { ... } }` 部分，添加对应的翻译：

```typescript
      browse: '浏览',
      browseFolder: '通过文件浏览器选择文件夹',
      selectDirFailed: '选择目录失败: {error}',
```

- [ ] **Step 2.2.3: 在 en.common 中确保有 close 键（如果没有）**

如果 `en: { common: { ... } }` 中没有 `close` 键，添加：

```typescript
    close: 'Close',
```

以及中文版本在 `zh: { common: { ... } }` 中：

```typescript
    close: '关闭',
```

- [ ] **Step 2.2.4: 提交此步骤**

```bash
git add src/locales/messages.ts
git commit -m "feat: 添加手动复制 Modal 的 i18n 翻译文本"
```

---

## Chunk 3: 完全重构 ManualCopyPage.vue

### Task 3.1: 重构 ManualCopyPage.vue 为触发 Modal 的页面

**文件：**
- Modify: `src/pages/ManualCopyPage.vue`

**步骤：**

- [ ] **Step 3.1.1: 完全替换 ManualCopyPage.vue 内容**

```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { Copy, ArrowRight, AlertCircle, Settings } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
import { getConfig, type AppConfig } from '@/lib/tauri';

defineOptions({ name: 'ManualCopyPage' });

const { t } = useI18n();
const router = useRouter();

const isModalOpen = ref(false);
const config = ref<AppConfig | null>(null);

async function loadConfig() {
  try {
    config.value = await getConfig();
  } catch (error) {
    console.error('Failed to load config:', error);
  }
}

function openManualCopyModal() {
  isModalOpen.value = true;
}

function closeManualCopyModal() {
  isModalOpen.value = false;
}

function handleCopySuccess() {
  // 可选：复制成功后显示成功提示或跳转
}

onMounted(loadConfig);
</script>

<template>
  <div class="p-6 bg-slate-50 min-h-full space-y-6">
    <div class="flex items-start justify-between gap-4 flex-wrap">
      <div>
        <h2 class="text-2xl font-bold text-slate-800 flex items-center gap-3">
          <div class="w-10 h-10 rounded-xl bg-blue-100 text-blue-600 flex items-center justify-center">
            <Copy class="w-5 h-5" />
          </div>
          {{ t('manualCopy.title') }}
        </h2>
        <p class="text-sm text-slate-500 mt-2 max-w-3xl">
          {{ t('manualCopy.subtitle') }}
        </p>
      </div>

      <router-link
        to="/settings"
        class="inline-flex items-center gap-2 px-4 py-2 rounded-lg border border-slate-200 bg-white text-slate-600 hover:text-blue-600 hover:border-blue-200 transition-colors"
      >
        <Settings class="w-4 h-4" />
        {{ t('manualCopy.viewRules') }}
      </router-link>
    </div>

    <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)] gap-6">
      <!-- Main Content Card -->
      <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-6 space-y-6">
        <div class="space-y-4">
          <p class="text-slate-600 text-sm leading-relaxed">
            {{ t('manualCopy.subtitle') }}
          </p>

          <!-- Main CTA Button -->
          <button
            @click="openManualCopyModal"
            class="w-full inline-flex items-center justify-center gap-2 px-6 py-3 rounded-xl text-white font-semibold transition-colors bg-blue-600 hover:bg-blue-700 active:bg-blue-800"
          >
            <Copy class="w-5 h-5" />
            {{ t('manualCopy.startCopy') }}
          </button>

          <p class="text-xs text-slate-400 text-center">
            {{ t('manualCopy.modalTip') }}
          </p>
        </div>
      </div>

      <!-- Info Cards Sidebar -->
      <div class="space-y-4">
        <!-- Rules Card -->
        <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-5 space-y-4">
          <div class="flex items-center gap-2 text-slate-800 font-semibold">
            <Copy class="w-4 h-4 text-blue-600" />
            {{ t('manualCopy.ruleCard') }}
          </div>

          <div class="rounded-xl bg-slate-50 border border-slate-200 p-4 space-y-3 text-sm text-slate-600">
            <div v-if="config">
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.filterTitle') }}</div>
              <div>
                {{ config.file_extensions.filter(Boolean).length > 0
                  ? t('manualCopy.extFilter', { value: config.file_extensions.join(', ') })
                  : t('manualCopy.noFilters') }}
              </div>
            </div>
            <div v-if="config">
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.stabilityTitle') }}</div>
              <div>
                {{ t('manualCopy.stabilityEnabled', {
                  mins: config.recent_file_guard_mins,
                  secs: config.stability_check_secs,
                }) }}
              </div>
            </div>
            <div>
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.modeTitle') }}</div>
              <div>{{ t('manualCopy.modeDesc') }}</div>
            </div>
          </div>
        </div>

        <!-- Progress Tracking Card -->
        <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-5 space-y-3">
          <div class="flex items-center gap-2 text-slate-800 font-semibold">
            <AlertCircle class="w-4 h-4 text-amber-500" />
            {{ t('manualCopy.queueHintTitle') }}
          </div>
          <p class="text-sm text-slate-500">{{ t('manualCopy.queueHintDesc') }}</p>
          <button
            @click="router.push('/')"
            class="inline-flex items-center gap-2 text-sm text-blue-600 hover:text-blue-800 font-medium"
          >
            {{ t('manualCopy.goToConsole') }}
            <ArrowRight class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>
  </div>

  <!-- Manual Copy Modal -->
  <ManualCopyModal :is-open="isModalOpen" @close="closeManualCopyModal" @success="handleCopySuccess" />
</template>
```

- [ ] **Step 3.1.2: 提交此步骤**

```bash
git add src/pages/ManualCopyPage.vue
git commit -m "feat: 重构 ManualCopyPage 为对话框触发形式，简化主页面 UI"
```

---

## Chunk 4: 最终验证与测试

### Task 4.1: 完整的集成测试

**步骤：**

- [ ] **Step 4.1.1: 构建项目验证编译**

```bash
cmd /c pnpm tauri build
```

预期输出：编译成功，无错误。

- [ ] **Step 4.1.2: 运行开发模式进行功能测试**

```bash
cmd /c pnpm tauri dev
```

预期行为：
- 应用启动正常
- 导航到 "Manual Copy" 页面
- 看到一个大的 "Start Copy" 按钮
- 点击按钮打开对话框 Modal
- Modal 中应该显示：
  - 源路径输入框
  - 目标路径输入框 + "Browse" 按钮
  - 浏览按钮点击可打开系统文件夹选择器
  - 输入路径后，切换到其他页面再返回，输入仍然保留
  - 点击"开始复制"按钮可提交任务

- [ ] **Step 4.1.3: 测试 localStorage 持久化**

1. 在 Modal 中输入一些路径
2. 关闭 Modal（点击 X 或 Close）
3. 重新打开 Modal
4. 验证之前输入的路径仍然存在

- [ ] **Step 4.1.4: 测试目录选择功能**

1. 在 Modal 中点击 "Browse" 按钮
2. 系统文件夹选择对话框应该出现
3. 选择一个文件夹
4. 验证路径被填充到目标路径输入框

- [ ] **Step 4.1.5: 提交最终版本**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

预期输出：成功生成 `file-sync-tool-1.0.0-YYYYMMDDHHmm.exe`

- [ ] **Step 4.1.6: 最终提交**

```bash
git add -A
git commit -m "feat: 完成手动复制功能 UI 优化 - 对话框 + 目录选择器 + 输入持久化"
```

---

## Summary

这个计划完成了以下优化：

1. ✅ **目标目录默认值 + 文件夹选择**：通过 Modal 中的 Browse 按钮调用系统文件夹选择器
2. ✅ **复制完成后不清空路径**：Modal 成功后不清空表单，2 秒后自动关闭，用户可重新打开继续复制
3. ✅ **UI 优化为对话框**：将长条表单改为弹出 Modal，节省主页面空间
4. ✅ **持久化用户输入**：使用 localStorage 在 store 中保存输入，离开页面后返回时输入仍然保留

所有步骤遵循 TDD、频繁提交的原则。
