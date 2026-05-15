<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Sliders } from 'lucide-vue-next';
import type {
  FileSharePermissionPreset,
  FileSharePermissionSet,
  FileShareRoot,
  FileShareUserRootPermissions,
} from '@/lib/tauri';
import CustomPermissionsDialog from './CustomPermissionsDialog.vue';

defineOptions({ name: 'RootAccessList' });

interface UserLike {
  username: string;
  root_permissions: FileShareUserRootPermissions[];
}

const props = defineProps<{
  user: UserLike;
  roots: FileShareRoot[];
  disabled?: boolean;
  userLabel?: string;
}>();

const { t } = useI18n();

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

const clonePerms = (v: FileSharePermissionSet): FileSharePermissionSet => ({ ...v });
const permsForPreset = (preset: FileSharePermissionPreset) =>
  preset === 'read_write' ? readWrite() : readOnly();

const permsEqual = (a: FileSharePermissionSet, b: FileSharePermissionSet) =>
  (Object.keys(a) as (keyof FileSharePermissionSet)[]).every((k) => a[k] === b[k]);

const detectPreset = (perms: FileSharePermissionSet): FileSharePermissionPreset => {
  if (permsEqual(perms, readOnly())) return 'read_only';
  if (permsEqual(perms, readWrite())) return 'read_write';
  return 'custom';
};

const rootName = (root: FileShareRoot) => {
  if (root.alias.trim()) return root.alias.trim();
  const trimmed = root.path.trim().replace(/[\\/]+$/g, '');
  if (!trimmed) return root.id;
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? root.id;
};

interface Row {
  root: FileShareRoot;
  entry: FileShareUserRootPermissions | null;
}

const rows = computed<Row[]>(() =>
  props.roots.map((root) => ({
    root,
    entry: props.user.root_permissions.find((p) => p.root_id === root.id) ?? null,
  })),
);

const enabledCount = computed(() => props.user.root_permissions.length);
const showBulk = computed(() => enabledCount.value >= 2 && !props.disabled);

const toggleRoot = (root: FileShareRoot, granted: boolean) => {
  if (props.disabled) return;
  const existing = props.user.root_permissions.find((p) => p.root_id === root.id);
  if (granted) {
    if (existing) return;
    props.user.root_permissions.push({
      root_id: root.id,
      preset: 'read_only',
      permissions: readOnly(),
    });
  } else if (existing) {
    props.user.root_permissions = props.user.root_permissions.filter((p) => p.root_id !== root.id);
  }
};

const pickPreset = (entry: FileShareUserRootPermissions, preset: FileSharePermissionPreset) => {
  if (props.disabled) return;
  if (preset === 'custom') {
    openCustom(entry);
    return;
  }
  entry.preset = preset;
  entry.permissions = permsForPreset(preset);
};

const applyBulk = (preset: FileSharePermissionPreset) => {
  if (props.disabled) return;
  for (const entry of props.user.root_permissions) {
    entry.preset = preset;
    entry.permissions = permsForPreset(preset);
  }
};

const dialogOpen = ref(false);
const dialogRootId = ref<string | null>(null);
const dialogRootName = ref('');
const dialogDraft = ref<FileSharePermissionSet>(readOnly());

const openCustom = (entry: FileShareUserRootPermissions) => {
  const root = props.roots.find((r) => r.id === entry.root_id);
  if (!root) return;
  dialogRootId.value = entry.root_id;
  dialogRootName.value = rootName(root);
  dialogDraft.value = clonePerms(entry.permissions);
  dialogOpen.value = true;
};

const closeDialog = () => {
  dialogOpen.value = false;
  dialogRootId.value = null;
};

const onDialogDone = (perms: FileSharePermissionSet) => {
  if (!dialogRootId.value) {
    closeDialog();
    return;
  }
  const entry = props.user.root_permissions.find((p) => p.root_id === dialogRootId.value);
  if (entry) {
    entry.permissions = clonePerms(perms);
    entry.preset = detectPreset(entry.permissions);
  }
  closeDialog();
};
</script>

<template>
  <div class="space-y-3">
    <div v-if="roots.length === 0" class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-4 text-center text-sm text-slate-500">
      {{ t('tools.fileShare.noRootsForPermissions') }}
    </div>

    <template v-else>
      <div v-if="showBulk" class="ral-bulk">
        <span class="ral-bulk-label">{{ t('tools.fileShare.bulkApplyLabel') }}</span>
        <div class="ral-seg ral-seg-bulk" role="group">
          <button
            type="button"
            class="ral-seg-btn"
            :disabled="disabled"
            @click="applyBulk('read_only')"
          >
            {{ t('tools.fileShare.presetReadOnly') }}
          </button>
          <button
            type="button"
            class="ral-seg-btn"
            :disabled="disabled"
            @click="applyBulk('read_write')"
          >
            {{ t('tools.fileShare.presetReadWrite') }}
          </button>
        </div>
      </div>

      <ul class="ral-list">
        <li
          v-for="row in rows"
          :key="row.root.id"
          class="ral-row"
          :class="{ 'ral-row-off': !row.entry }"
        >
          <label class="ral-name">
            <input
              type="checkbox"
              :checked="!!row.entry"
              :disabled="disabled"
              class="ral-check"
              @change="(e) => toggleRoot(row.root, (e.target as HTMLInputElement).checked)"
            />
            <span class="ral-name-text" :title="row.root.path">{{ rootName(row.root) }}</span>
          </label>

          <div v-if="row.entry" class="ral-controls">
            <div class="ral-seg" role="group">
              <button
                type="button"
                class="ral-seg-btn"
                :class="{ 'ral-seg-active': row.entry.preset === 'read_only' }"
                :disabled="disabled"
                @click="pickPreset(row.entry, 'read_only')"
              >
                {{ t('tools.fileShare.presetReadOnly') }}
              </button>
              <button
                type="button"
                class="ral-seg-btn"
                :class="{ 'ral-seg-active': row.entry.preset === 'read_write' }"
                :disabled="disabled"
                @click="pickPreset(row.entry, 'read_write')"
              >
                {{ t('tools.fileShare.presetReadWrite') }}
              </button>
              <button
                type="button"
                class="ral-seg-btn ral-seg-custom"
                :class="{ 'ral-seg-active': row.entry.preset === 'custom' }"
                :disabled="disabled"
                :title="t('tools.fileShare.customizeOpenAria')"
                :aria-label="t('tools.fileShare.customizeOpenAria')"
                @click="pickPreset(row.entry, 'custom')"
              >
                <Sliders class="h-3.5 w-3.5" />
                <span>{{ t('tools.fileShare.presetCustom') }}</span>
                <span v-if="row.entry.preset === 'custom'" class="ral-custom-dot" aria-hidden="true"></span>
              </button>
            </div>
          </div>
          <span v-else class="ral-off-hint">— {{ t('tools.fileShare.rootDisabledHint') }} —</span>
        </li>
      </ul>
    </template>

    <CustomPermissionsDialog
      :open="dialogOpen"
      :perms="dialogDraft"
      :user-label="userLabel ?? user.username"
      :root-label="dialogRootName"
      @cancel="closeDialog"
      @done="onDialogDone"
    />
  </div>
</template>

<style scoped>
.ral-list {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  margin: 0;
  padding: 0;
  list-style: none;
}
.ral-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  min-height: 2.5rem;
  padding: 0.45rem 0.75rem;
  border: 1px solid rgb(226 232 240 / 0.9);
  border-radius: 0.75rem;
  background: #fff;
  transition: background-color 0.15s ease, border-color 0.15s ease;
}
.ral-row-off {
  background: rgb(248 250 252);
  border-color: rgb(226 232 240 / 0.7);
}
.ral-name {
  display: inline-flex;
  align-items: center;
  gap: 0.6rem;
  min-width: 0;
  flex: 1 1 auto;
  font-size: 0.875rem;
  font-weight: 600;
  color: rgb(15 23 42);
  cursor: pointer;
}
.ral-row-off .ral-name {
  color: rgb(100 116 139);
  font-weight: 500;
}
.ral-name-text {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ral-check {
  flex-shrink: 0;
  border-radius: 0.25rem;
  border: 1px solid rgb(203 213 225);
}
.ral-controls {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
}
.ral-off-hint {
  font-size: 0.75rem;
  color: rgb(148 163 184);
  white-space: nowrap;
  flex-shrink: 0;
}
.ral-seg {
  display: inline-flex;
  align-items: stretch;
  border: 1px solid rgb(203 213 225);
  border-radius: 0.65rem;
  background: rgb(248 250 252);
  padding: 2px;
  gap: 2px;
}
.ral-seg-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 0.3rem;
  min-height: 1.85rem;
  padding: 0 0.7rem;
  border-radius: 0.45rem;
  font-size: 0.8125rem;
  font-weight: 600;
  color: rgb(71 85 105);
  background: transparent;
  transition: background-color 0.15s ease, color 0.15s ease, box-shadow 0.15s ease;
  white-space: nowrap;
}
.ral-seg-btn:hover:not(:disabled) {
  color: rgb(15 118 110);
  background: rgb(240 253 250);
}
.ral-seg-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.ral-seg-active {
  background: #fff;
  color: rgb(13 148 136);
  box-shadow: 0 1px 3px rgb(15 23 42 / 0.08);
}
.ral-seg-custom {
  position: relative;
}
.ral-custom-dot {
  position: absolute;
  top: 4px;
  right: 6px;
  width: 6px;
  height: 6px;
  border-radius: 9999px;
  background: rgb(13 148 136);
}
.ral-bulk {
  display: inline-flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0.3rem 0.4rem 0.3rem 0.75rem;
  border: 1px dashed rgb(186 230 253);
  border-radius: 0.75rem;
  background: rgb(240 253 250 / 0.5);
}
.ral-bulk-label {
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(15 118 110);
  letter-spacing: 0.04em;
}
.ral-seg-bulk {
  background: #fff;
}
@media (max-width: 640px) {
  .ral-row {
    flex-wrap: wrap;
  }
  .ral-controls {
    margin-left: 1.85rem;
  }
}
</style>
