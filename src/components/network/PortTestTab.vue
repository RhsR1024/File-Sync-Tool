<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { testPorts, type PortTestRequest, type PortTestResult, type SinglePortResult, type PortPreset } from '../../lib/tauri';

defineOptions({ name: 'PortTestTab' });

const { t } = useI18n();

// ─── Port parsing ────────────────────────────────────────────────────────────

function parsePorts(input: string): number[] {
  const parts = input.split(',').map(s => s.trim()).filter(Boolean);
  const ports: Set<number> = new Set();
  for (const part of parts) {
    if (part.includes('-')) {
      const [startStr, endStr] = part.split('-');
      const start = parseInt(startStr), end = parseInt(endStr);
      if (!isNaN(start) && !isNaN(end) && start >= 1 && end <= 65535 && start <= end) {
        for (let i = start; i <= end; i++) ports.add(i);
      }
    } else {
      const p = parseInt(part);
      if (!isNaN(p) && p >= 1 && p <= 65535) ports.add(p);
    }
  }
  return Array.from(ports).sort((a, b) => a - b);
}

// ─── localStorage presets ────────────────────────────────────────────────────

const STORAGE_KEY = 'networkTools.portPresets';

function loadPresetsFromLocalStorage(): PortPreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as PortPreset[];
  } catch {
    return [];
  }
}

function savePresetsToLocalStorage(presets: PortPreset[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(presets));
}

// ─── Built-in presets ────────────────────────────────────────────────────────

const builtinPresets: PortPreset[] = [
  { name: 'Web', ports: '80,443' },
  { name: 'SSH', ports: '22' },
  { name: 'Database', ports: '3306,5432,6379' },
  { name: 'Common', ports: '22,80,443,3306,5432,6379,8080,8443' },
];

// ─── State ───────────────────────────────────────────────────────────────────

const host = ref('');
const portsInput = ref('');
const timeoutMs = ref(3000);
const isLoading = ref(false);
const result = ref<PortTestResult | null>(null);
const customPresets = ref<PortPreset[]>(loadPresetsFromLocalStorage());

const showPresetForm = ref(false);
const editingPreset = ref<number | null>(null); // index into customPresets
const presetName = ref('');
const presetPorts = ref('');

const errorMsg = ref('');

// ─── Preset interactions ─────────────────────────────────────────────────────

function applyPreset(ports: string): void {
  portsInput.value = ports;
}

function openAddPreset(): void {
  editingPreset.value = null;
  presetName.value = '';
  presetPorts.value = '';
  showPresetForm.value = true;
}

function openEditPreset(index: number): void {
  editingPreset.value = index;
  presetName.value = customPresets.value[index].name;
  presetPorts.value = customPresets.value[index].ports;
  showPresetForm.value = true;
}

function cancelPresetForm(): void {
  showPresetForm.value = false;
  editingPreset.value = null;
  presetName.value = '';
  presetPorts.value = '';
}

function savePreset(): void {
  const name = presetName.value.trim();
  const ports = presetPorts.value.trim();
  if (!name || !ports) return;

  if (editingPreset.value !== null) {
    customPresets.value[editingPreset.value] = { name, ports };
  } else {
    customPresets.value.push({ name, ports });
  }
  savePresetsToLocalStorage(customPresets.value);
  cancelPresetForm();
}

function deletePreset(index: number): void {
  customPresets.value.splice(index, 1);
  savePresetsToLocalStorage(customPresets.value);
}

// ─── Test logic ───────────────────────────────────────────────────────────────

async function startTest(): Promise<void> {
  errorMsg.value = '';
  const h = host.value.trim();
  if (!h) {
    errorMsg.value = t('networkTools.port.hostError');
    return;
  }
  const ports = parsePorts(portsInput.value);
  if (ports.length === 0) {
    errorMsg.value = t('networkTools.port.portsError');
    return;
  }
  if (ports.length > 1000) {
    errorMsg.value = t('networkTools.port.tooManyPorts');
    return;
  }

  isLoading.value = true;
  result.value = null;
  try {
    const request: PortTestRequest = {
      host: h,
      ports,
      timeoutMs: timeoutMs.value,
    };
    result.value = await testPorts(request);
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    isLoading.value = false;
  }
}
</script>

<template>
  <div class="space-y-5">
    <!-- Input area -->
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
      <!-- Host -->
      <div class="sm:col-span-1">
        <label class="block text-xs font-medium text-slate-600 mb-1">
          {{ t('networkTools.port.targetHost') }}
        </label>
        <input
          v-model="host"
          type="text"
          :placeholder="t('networkTools.port.targetPlaceholder')"
          class="w-full border border-slate-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>

      <!-- Ports -->
      <div class="sm:col-span-1">
        <label class="block text-xs font-medium text-slate-600 mb-1">
          {{ t('networkTools.port.ports') }}
        </label>
        <input
          v-model="portsInput"
          type="text"
          :placeholder="t('networkTools.port.portsPlaceholder')"
          class="w-full border border-slate-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>

      <!-- Timeout -->
      <div class="sm:col-span-1">
        <label class="block text-xs font-medium text-slate-600 mb-1">
          {{ t('networkTools.port.timeoutMs') }}
        </label>
        <div class="flex gap-2">
          <input
            v-model.number="timeoutMs"
            type="number"
            min="100"
            max="30000"
            class="flex-1 border border-slate-300 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
          <button
            @click="startTest"
            :disabled="isLoading"
            class="px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 text-white text-sm font-medium rounded-lg transition-colors whitespace-nowrap"
          >
            {{ isLoading ? t('networkTools.port.testing') : t('networkTools.port.startTest') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Error message -->
    <div v-if="errorMsg" class="text-sm text-red-600 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
      {{ errorMsg }}
    </div>

    <!-- Presets area -->
    <div class="space-y-3">
      <!-- Built-in presets -->
      <div>
        <p class="text-xs font-medium text-slate-500 mb-2">{{ t('networkTools.port.presets') }}</p>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="preset in builtinPresets"
            :key="preset.name"
            @click="applyPreset(preset.ports)"
            class="rounded-full px-3 py-1 text-xs border cursor-pointer transition-colors border-slate-300 text-slate-600 hover:bg-blue-100 hover:border-blue-400 hover:text-blue-700"
          >
            {{ preset.name }}
          </button>
        </div>
      </div>

      <!-- Custom presets -->
      <div>
        <div class="flex items-center gap-2 mb-2">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.port.customPresets') }}</p>
          <button
            v-if="!showPresetForm"
            @click="openAddPreset"
            class="text-xs text-blue-600 hover:text-blue-800 font-medium transition-colors"
          >
            + {{ t('networkTools.port.addPreset') }}
          </button>
        </div>

        <!-- Custom preset pills -->
        <div class="flex flex-wrap gap-2">
          <div
            v-for="(preset, index) in customPresets"
            :key="index"
            class="group relative flex items-center gap-1 rounded-full px-3 py-1 text-xs border cursor-pointer transition-colors border-slate-300 text-slate-600 hover:bg-blue-50 hover:border-blue-400 hover:text-blue-700"
          >
            <span @click="applyPreset(preset.ports)">{{ preset.name }}</span>
            <!-- Edit/Delete buttons shown on hover -->
            <span class="hidden group-hover:flex items-center gap-0.5 ml-1">
              <button
                @click.stop="openEditPreset(index)"
                class="text-blue-500 hover:text-blue-700 px-0.5"
                :title="t('networkTools.port.editPreset')"
              >
                ✎
              </button>
              <button
                @click.stop="deletePreset(index)"
                class="text-red-500 hover:text-red-700 px-0.5"
                :title="t('networkTools.port.deletePreset')"
              >
                ×
              </button>
            </span>
          </div>

          <span v-if="customPresets.length === 0 && !showPresetForm" class="text-xs text-slate-400 italic">
            —
          </span>
        </div>

        <!-- Inline add/edit form -->
        <div v-if="showPresetForm" class="mt-3 flex flex-wrap items-end gap-2 p-3 bg-slate-50 border border-slate-200 rounded-lg">
          <div>
            <label class="block text-xs font-medium text-slate-600 mb-1">{{ t('networkTools.port.presetName') }}</label>
            <input
              v-model="presetName"
              type="text"
              placeholder="e.g. My Ports"
              class="border border-slate-300 rounded-md px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-slate-600 mb-1">{{ t('networkTools.port.presetPorts') }}</label>
            <input
              v-model="presetPorts"
              type="text"
              placeholder="80,443,8080"
              class="border border-slate-300 rounded-md px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
          <div class="flex gap-2">
            <button
              @click="savePreset"
              class="px-3 py-1 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded-md transition-colors"
            >
              {{ t('networkTools.port.save') }}
            </button>
            <button
              @click="cancelPresetForm"
              class="px-3 py-1 bg-slate-200 hover:bg-slate-300 text-slate-700 text-xs font-medium rounded-md transition-colors"
            >
              {{ t('networkTools.port.cancel') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Results table -->
    <div v-if="result" class="space-y-2">
      <!-- Host resolved info -->
      <div class="text-sm text-slate-600">
        <span class="font-medium">{{ result.host }}</span>
        <span v-if="result.resolvedIp" class="text-slate-400 ml-2">({{ result.resolvedIp }})</span>
      </div>

      <div class="border border-slate-200 rounded-lg overflow-hidden">
        <table class="w-full text-sm">
          <thead>
            <tr class="bg-slate-50 border-b border-slate-200">
              <th class="text-left px-4 py-2.5 text-xs font-semibold text-slate-600 uppercase tracking-wide">
                {{ t('networkTools.port.port') }}
              </th>
              <th class="text-left px-4 py-2.5 text-xs font-semibold text-slate-600 uppercase tracking-wide">
                {{ t('networkTools.port.service') }}
              </th>
              <th class="text-left px-4 py-2.5 text-xs font-semibold text-slate-600 uppercase tracking-wide">
                {{ t('networkTools.port.status') }}
              </th>
              <th class="text-left px-4 py-2.5 text-xs font-semibold text-slate-600 uppercase tracking-wide">
                {{ t('networkTools.port.latency') }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in result.results"
              :key="row.port"
              :class="index % 2 === 0 ? 'bg-white' : 'bg-slate-50'"
              class="border-b border-slate-100 last:border-0"
            >
              <td class="px-4 py-2.5 font-mono text-slate-800">{{ row.port }}</td>
              <td class="px-4 py-2.5 text-slate-600">{{ row.name || '—' }}</td>
              <td class="px-4 py-2.5">
                <span
                  :class="row.open
                    ? 'bg-green-100 text-green-700 border border-green-200'
                    : 'bg-red-100 text-red-700 border border-red-200'"
                  class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium"
                >
                  {{ row.open ? t('networkTools.port.open') : t('networkTools.port.closed') }}
                </span>
              </td>
              <td class="px-4 py-2.5 text-slate-600 font-mono text-xs">
                {{ row.latencyMs !== null ? `${row.latencyMs} ms` : '—' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
