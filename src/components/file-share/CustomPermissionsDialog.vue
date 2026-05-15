<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { X, RotateCcw, Eye, Download, Upload, Settings, Search } from 'lucide-vue-next';
import type { FileSharePermissionSet } from '@/lib/tauri';

defineOptions({ name: 'CustomPermissionsDialog' });

const props = defineProps<{
  open: boolean;
  perms: FileSharePermissionSet;
  userLabel: string;
  rootLabel: string;
}>();

const emit = defineEmits<{
  cancel: [];
  done: [perms: FileSharePermissionSet];
}>();

const { t } = useI18n();

type PermKey = keyof FileSharePermissionSet;

const readOnly = (): FileSharePermissionSet => ({
  browse: true,
  download_file: true,
  download_archive: true,
  upload_file: false,
  upload_directory: false,
  create_directory: false,
  create_text: false,
  rename: false,
  delete: false,
  preview_image: true,
  search_current: true,
  search_global: true,
});

const readWrite = (): FileSharePermissionSet => ({
  browse: true,
  download_file: true,
  download_archive: true,
  upload_file: true,
  upload_directory: true,
  create_directory: true,
  create_text: true,
  rename: true,
  delete: true,
  preview_image: true,
  search_current: true,
  search_global: true,
});

const draft = ref<FileSharePermissionSet>({ ...props.perms });

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      draft.value = { ...props.perms };
    }
  },
  { immediate: false },
);

const groups = computed(() => [
  {
    key: 'view',
    icon: Eye,
    label: t('tools.fileShare.permissionGroups.view'),
    items: ['browse', 'preview_image'] as PermKey[],
  },
  {
    key: 'download',
    icon: Download,
    label: t('tools.fileShare.permissionGroups.download'),
    items: ['download_file', 'download_archive'] as PermKey[],
  },
  {
    key: 'upload',
    icon: Upload,
    label: t('tools.fileShare.permissionGroups.upload'),
    items: ['upload_file', 'upload_directory', 'create_directory', 'create_text'] as PermKey[],
  },
  {
    key: 'manage',
    icon: Settings,
    label: t('tools.fileShare.permissionGroups.manage'),
    items: ['rename', 'delete'] as PermKey[],
  },
  {
    key: 'search',
    icon: Search,
    label: t('tools.fileShare.permissionGroups.search'),
    items: ['search_current', 'search_global'] as PermKey[],
  },
]);

const permLabel = (key: PermKey) => t(`tools.fileShare.permissions.${key}`);

const resetTo = (preset: 'read_only' | 'read_write') => {
  draft.value = preset === 'read_only' ? readOnly() : readWrite();
};

const onCancel = () => emit('cancel');
const onDone = () => emit('done', { ...draft.value });

const onBackdrop = (event: MouseEvent) => {
  if (event.target === event.currentTarget) onCancel();
};

const onKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') onCancel();
};
</script>

<template>
  <Teleport to="body">
    <Transition name="cpd-fade">
      <div
        v-if="open"
        class="fixed inset-0 z-[60] flex items-center justify-center bg-slate-900/45 backdrop-blur-sm px-4"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cpd-title"
        @click="onBackdrop"
        @keydown="onKeydown"
      >
        <div class="cpd-card" @click.stop>
          <header class="cpd-header">
            <div class="min-w-0">
              <h3 id="cpd-title" class="cpd-title">{{ t('tools.fileShare.customizeDialogTitle') }}</h3>
              <p class="cpd-subtitle">
                <span class="cpd-pill">{{ userLabel || '—' }}</span>
                <span class="cpd-divider" aria-hidden="true">·</span>
                <span class="cpd-pill cpd-pill-root">{{ rootLabel || '—' }}</span>
              </p>
            </div>
            <button
              type="button"
              class="cpd-icon-btn"
              :aria-label="t('tools.fileShare.customizeDialogCancel')"
              @click="onCancel"
            >
              <X class="h-4 w-4" />
            </button>
          </header>

          <div class="cpd-quick">
            <button type="button" class="cpd-quick-btn" @click="resetTo('read_only')">
              <RotateCcw class="h-3.5 w-3.5" />
              {{ t('tools.fileShare.resetToReadOnly') }}
            </button>
            <button type="button" class="cpd-quick-btn" @click="resetTo('read_write')">
              <RotateCcw class="h-3.5 w-3.5" />
              {{ t('tools.fileShare.resetToReadWrite') }}
            </button>
          </div>

          <div class="cpd-body">
            <section v-for="group in groups" :key="group.key" class="cpd-group">
              <div class="cpd-group-head">
                <component :is="group.icon" class="h-3.5 w-3.5" />
                <span>{{ group.label }}</span>
              </div>
              <div class="cpd-group-grid">
                <label
                  v-for="key in group.items"
                  :key="key"
                  class="cpd-perm"
                  :class="{ 'cpd-perm-on': draft[key] }"
                >
                  <input
                    v-model="draft[key]"
                    type="checkbox"
                    class="cpd-perm-check"
                  />
                  <span>{{ permLabel(key) }}</span>
                </label>
              </div>
            </section>
          </div>

          <footer class="cpd-footer">
            <button type="button" class="cpd-btn cpd-btn-ghost" @click="onCancel">
              {{ t('tools.fileShare.customizeDialogCancel') }}
            </button>
            <button type="button" class="cpd-btn cpd-btn-primary" @click="onDone">
              {{ t('tools.fileShare.customizeDialogDone') }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.cpd-card {
  position: relative;
  width: 100%;
  max-width: 36rem;
  max-height: 88vh;
  display: flex;
  flex-direction: column;
  border-radius: 1rem;
  background: #fff;
  box-shadow: 0 24px 64px rgb(15 23 42 / 0.22);
  border: 1px solid rgb(226 232 240);
  overflow: hidden;
}
.cpd-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  padding: 1rem 1.25rem;
  border-bottom: 1px solid rgb(226 232 240);
  background: linear-gradient(180deg, #fff 0%, rgb(248 250 252) 100%);
}
.cpd-title {
  font-size: 1rem;
  font-weight: 700;
  color: rgb(15 23 42);
}
.cpd-subtitle {
  margin-top: 0.35rem;
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
}
.cpd-pill {
  display: inline-flex;
  align-items: center;
  padding: 0.15rem 0.55rem;
  border-radius: 9999px;
  border: 1px solid rgb(186 230 253);
  background: rgb(240 253 250);
  color: rgb(15 118 110);
  font-weight: 600;
  white-space: nowrap;
  max-width: 14rem;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cpd-pill-root {
  border-color: rgb(226 232 240);
  background: rgb(248 250 252);
  color: rgb(51 65 85);
}
.cpd-divider {
  color: rgb(148 163 184);
}
.cpd-icon-btn {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 2rem;
  height: 2rem;
  border-radius: 0.55rem;
  border: 1px solid rgb(226 232 240);
  background: #fff;
  color: rgb(100 116 139);
  transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.cpd-icon-btn:hover {
  background: rgb(240 253 250);
  border-color: rgb(153 246 228);
  color: rgb(13 148 136);
}
.cpd-quick {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  padding: 0.75rem 1.25rem 0;
}
.cpd-quick-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.4rem 0.75rem;
  border: 1px solid rgb(226 232 240);
  border-radius: 9999px;
  background: #fff;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(51 65 85);
  transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.cpd-quick-btn:hover {
  background: rgb(240 253 250);
  border-color: rgb(153 246 228);
  color: rgb(13 118 110);
}
.cpd-body {
  padding: 0.75rem 1.25rem 0.25rem;
  overflow-y: auto;
}
.cpd-group {
  padding: 0.75rem 0;
  border-bottom: 1px dashed rgb(226 232 240 / 0.8);
}
.cpd-group:last-child {
  border-bottom: none;
}
.cpd-group-head {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  margin-bottom: 0.55rem;
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: rgb(100 116 139);
}
.cpd-group-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.45rem;
}
@media (max-width: 480px) {
  .cpd-group-grid {
    grid-template-columns: 1fr;
  }
}
.cpd-perm {
  display: inline-flex;
  align-items: center;
  gap: 0.55rem;
  padding: 0.55rem 0.75rem;
  border: 1px solid rgb(226 232 240);
  border-radius: 0.65rem;
  background: #fff;
  font-size: 0.8125rem;
  color: rgb(51 65 85);
  cursor: pointer;
  transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.cpd-perm:hover {
  border-color: rgb(186 230 253);
}
.cpd-perm-on {
  background: rgb(240 253 250);
  border-color: rgb(153 246 228);
  color: rgb(15 118 110);
  font-weight: 600;
}
.cpd-perm-check {
  flex-shrink: 0;
  border-radius: 0.25rem;
  border: 1px solid rgb(203 213 225);
}
.cpd-footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  padding: 0.85rem 1.25rem;
  border-top: 1px solid rgb(226 232 240);
  background: rgb(248 250 252);
}
.cpd-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 2.25rem;
  padding: 0 1rem;
  border-radius: 0.65rem;
  font-size: 0.8125rem;
  font-weight: 600;
  transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.cpd-btn-ghost {
  background: #fff;
  border: 1px solid rgb(226 232 240);
  color: rgb(71 85 105);
}
.cpd-btn-ghost:hover {
  border-color: rgb(203 213 225);
  background: rgb(248 250 252);
}
.cpd-btn-primary {
  background: linear-gradient(135deg, rgb(13 148 136), rgb(8 145 178));
  color: #fff;
  border: 1px solid transparent;
  box-shadow: 0 6px 16px rgb(13 148 136 / 0.22);
}
.cpd-btn-primary:hover {
  box-shadow: 0 10px 22px rgb(13 148 136 / 0.28);
}

.cpd-fade-enter-active,
.cpd-fade-leave-active {
  transition: opacity 0.18s ease;
}
.cpd-fade-enter-active .cpd-card,
.cpd-fade-leave-active .cpd-card {
  transition: transform 0.18s ease, opacity 0.18s ease;
}
.cpd-fade-enter-from,
.cpd-fade-leave-to {
  opacity: 0;
}
.cpd-fade-enter-from .cpd-card,
.cpd-fade-leave-to .cpd-card {
  opacity: 0;
  transform: scale(0.97);
}

@media (prefers-reduced-motion: reduce) {
  .cpd-fade-enter-active,
  .cpd-fade-leave-active,
  .cpd-fade-enter-active .cpd-card,
  .cpd-fade-leave-active .cpd-card {
    transition-duration: 80ms;
  }
  .cpd-fade-enter-from .cpd-card,
  .cpd-fade-leave-to .cpd-card {
    transform: none;
  }
}
</style>
