<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { sendWol, type WolResult, type WolDevice } from '../../lib/tauri';

defineOptions({ name: 'WakeOnLanTab' });

const { t } = useI18n();

// ── LocalStorage helpers ────────────────────────────────────────────────────

const STORAGE_KEY = 'networkTools.wolDevices';

function loadDevicesFromLocalStorage(): WolDevice[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed as WolDevice[];
  } catch {
    // ignore parse errors
  }
  return [];
}

function saveDevicesToLocalStorage(devs: WolDevice[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(devs));
}

// ── State ───────────────────────────────────────────────────────────────────

const macAddress = ref('');
const broadcast = ref('255.255.255.255');
const port = ref(9);
const isLoading = ref(false);
const lastResult = ref<WolResult | null>(null);
const devices = ref<WolDevice[]>(loadDevicesFromLocalStorage());
const showSaveForm = ref(false);
const newDeviceName = ref('');

let resultTimer: ReturnType<typeof setTimeout> | null = null;

// ── MAC Validation ──────────────────────────────────────────────────────────

function isValidMac(mac: string): boolean {
  // With separators: AA:BB:CC:DD:EE:FF or AA-BB-CC-DD-EE-FF
  if (/^([0-9A-Fa-f]{2}[:\-]){5}[0-9A-Fa-f]{2}$/.test(mac)) return true;
  // 12 hex chars without separator
  if (/^[0-9A-Fa-f]{12}$/.test(mac)) return true;
  return false;
}

const macError = ref('');

function validateMac() {
  if (macAddress.value && !isValidMac(macAddress.value)) {
    macError.value = t('networkTools.wol.macError');
  } else {
    macError.value = '';
  }
}

// ── Result display ──────────────────────────────────────────────────────────

function showResult(result: WolResult) {
  lastResult.value = result;
  if (resultTimer !== null) clearTimeout(resultTimer);
  resultTimer = setTimeout(() => {
    lastResult.value = null;
    resultTimer = null;
  }, 5000);
}

// ── Send WOL ────────────────────────────────────────────────────────────────

async function handleSendWol() {
  validateMac();
  if (macError.value || !macAddress.value) return;
  isLoading.value = true;
  lastResult.value = null;
  try {
    const result = await sendWol({
      mac: macAddress.value,
      broadcastIp: broadcast.value || undefined,
      port: port.value || undefined,
    });
    showResult(result);
  } catch (err) {
    showResult({ mac: macAddress.value, success: false, message: String(err) });
  } finally {
    isLoading.value = false;
  }
}

// ── Wake saved device ────────────────────────────────────────────────────────

async function wakeDevice(device: WolDevice) {
  isLoading.value = true;
  lastResult.value = null;
  try {
    const result = await sendWol({
      mac: device.mac,
      broadcastIp: device.broadcast || undefined,
      port: device.port || undefined,
    });
    showResult(result);
  } catch (err) {
    showResult({ mac: device.mac, success: false, message: String(err) });
  } finally {
    isLoading.value = false;
  }
}

// ── Save device ─────────────────────────────────────────────────────────────

function toggleSaveForm() {
  showSaveForm.value = !showSaveForm.value;
  if (showSaveForm.value) {
    newDeviceName.value = '';
  }
}

function saveDevice() {
  if (!newDeviceName.value.trim()) return;
  validateMac();
  if (macError.value || !macAddress.value) return;

  const device: WolDevice = {
    name: newDeviceName.value.trim(),
    mac: macAddress.value,
    broadcast: broadcast.value || '255.255.255.255',
    port: port.value || 9,
  };
  devices.value = [...devices.value, device];
  saveDevicesToLocalStorage(devices.value);
  showSaveForm.value = false;
  newDeviceName.value = '';
}

function deleteDevice(index: number) {
  devices.value = devices.value.filter((_, i) => i !== index);
  saveDevicesToLocalStorage(devices.value);
}

// ── Load device into form ────────────────────────────────────────────────────

function loadDevice(device: WolDevice) {
  macAddress.value = device.mac;
  broadcast.value = device.broadcast;
  port.value = device.port;
  macError.value = '';
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

onMounted(() => {
  devices.value = loadDevicesFromLocalStorage();
});
</script>

<template>
  <div class="space-y-5">
    <!-- Input area -->
    <div class="space-y-4">
      <!-- MAC Address -->
      <div class="space-y-1">
        <label class="block text-sm font-medium text-slate-700">
          {{ t('networkTools.wol.macAddress') }}
        </label>
        <input
          v-model="macAddress"
          type="text"
          :placeholder="t('networkTools.wol.macPlaceholder')"
          @blur="validateMac"
          @input="macError = ''"
          class="w-full px-3 py-2 text-sm border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors"
          :class="macError ? 'border-red-400 bg-red-50' : 'border-slate-300 bg-white'"
        />
        <p v-if="macError" class="text-xs text-red-500">{{ macError }}</p>
      </div>

      <!-- Broadcast and Port row -->
      <div class="flex gap-3">
        <div class="flex-1 space-y-1">
          <label class="block text-sm font-medium text-slate-700">
            {{ t('networkTools.wol.broadcast') }}
          </label>
          <input
            v-model="broadcast"
            type="text"
            :placeholder="t('networkTools.wol.broadcastPlaceholder')"
            class="w-full px-3 py-2 text-sm border border-slate-300 rounded-lg bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors"
          />
        </div>
        <div class="w-28 space-y-1">
          <label class="block text-sm font-medium text-slate-700">
            {{ t('networkTools.wol.port') }}
          </label>
          <input
            v-model.number="port"
            type="number"
            min="1"
            max="65535"
            class="w-full px-3 py-2 text-sm border border-slate-300 rounded-lg bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors"
          />
        </div>
      </div>

      <!-- Action buttons row -->
      <div class="flex items-center gap-2">
        <button
          @click="handleSendWol"
          :disabled="isLoading"
          class="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <span v-if="isLoading" class="flex items-center gap-1.5">
            <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z" />
            </svg>
            {{ t('networkTools.wol.sendWol') }}
          </span>
          <span v-else>{{ t('networkTools.wol.sendWol') }}</span>
        </button>

        <button
          @click="toggleSaveForm"
          class="px-4 py-2 text-sm font-medium text-slate-600 bg-slate-100 rounded-lg hover:bg-slate-200 transition-colors"
        >
          {{ t('networkTools.wol.saveCurrent') }}
        </button>
      </div>

      <!-- Inline save form -->
      <div v-if="showSaveForm" class="flex items-center gap-2 p-3 bg-slate-50 border border-slate-200 rounded-lg">
        <input
          v-model="newDeviceName"
          type="text"
          :placeholder="t('networkTools.wol.deviceNamePlaceholder')"
          @keydown.enter="saveDevice"
          class="flex-1 px-3 py-1.5 text-sm border border-slate-300 rounded-md bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors"
        />
        <button
          @click="saveDevice"
          :disabled="!newDeviceName.trim()"
          class="px-3 py-1.5 text-sm font-medium text-white bg-emerald-600 rounded-md hover:bg-emerald-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {{ t('networkTools.wol.deviceName') }}
        </button>
        <button
          @click="showSaveForm = false"
          class="px-3 py-1.5 text-sm font-medium text-slate-600 bg-white border border-slate-300 rounded-md hover:bg-slate-50 transition-colors"
        >
          ✕
        </button>
      </div>
    </div>

    <!-- Result banner -->
    <div
      v-if="lastResult"
      class="px-4 py-3 rounded-lg border text-sm font-medium transition-all"
      :class="lastResult.success
        ? 'bg-emerald-50 border border-emerald-200 text-emerald-700'
        : 'bg-red-50 border border-red-200 text-red-700'"
    >
      {{ lastResult.message }}
    </div>

    <!-- Saved Devices -->
    <div class="space-y-2">
      <h3 class="text-sm font-semibold text-slate-700">{{ t('networkTools.wol.savedDevices') }}</h3>

      <!-- Empty state -->
      <div
        v-if="devices.length === 0"
        class="py-8 text-center text-sm text-slate-400 border border-dashed border-slate-200 rounded-lg"
      >
        {{ t('networkTools.wol.noDevices') }}
      </div>

      <!-- Devices table -->
      <div v-else class="border border-slate-200 rounded-lg overflow-hidden">
        <table class="w-full text-sm">
          <thead>
            <tr class="bg-slate-50 border-b border-slate-200">
              <th class="px-4 py-2.5 text-left font-medium text-slate-600">
                {{ t('networkTools.wol.deviceName') }}
              </th>
              <th class="px-4 py-2.5 text-left font-medium text-slate-600">
                {{ t('networkTools.wol.macAddress') }}
              </th>
              <th class="px-4 py-2.5 text-left font-medium text-slate-600">
                {{ t('networkTools.wol.broadcast') }}
              </th>
              <th class="px-4 py-2.5 text-right font-medium text-slate-600 w-32"></th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(device, index) in devices"
              :key="index"
              class="border-b border-slate-100 last:border-b-0 hover:bg-slate-50 transition-colors"
            >
              <td
                class="px-4 py-2.5 text-slate-800 font-medium cursor-pointer"
                @click="loadDevice(device)"
                :title="device.name"
              >
                {{ device.name }}
              </td>
              <td class="px-4 py-2.5 text-slate-600 font-mono text-xs">
                {{ device.mac }}
              </td>
              <td class="px-4 py-2.5 text-slate-500 text-xs">
                {{ device.broadcast }}:{{ device.port }}
              </td>
              <td class="px-4 py-2.5 text-right">
                <div class="flex items-center justify-end gap-1.5">
                  <button
                    @click="wakeDevice(device)"
                    :disabled="isLoading"
                    class="px-2.5 py-1 text-xs font-medium text-blue-600 bg-blue-50 border border-blue-200 rounded hover:bg-blue-100 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {{ t('networkTools.wol.wake') }}
                  </button>
                  <button
                    @click="deleteDevice(index)"
                    class="px-2.5 py-1 text-xs font-medium text-red-500 bg-red-50 border border-red-200 rounded hover:bg-red-100 transition-colors"
                  >
                    {{ t('networkTools.wol.delete') }}
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
