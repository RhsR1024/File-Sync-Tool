<script setup lang="ts">
import { Copy } from 'lucide-vue-next';
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import { pushToast } from '../../composables/useToast';

defineOptions({ name: 'SubnetCalcTab' });

const { t } = useI18n();

interface SubnetResult {
  networkAddr: string;
  broadcastAddr: string;
  subnetMask: string;
  wildcardMask: string;
  firstHost: string;
  lastHost: string;
  hostCount: number;
}

const ipInput = ref('192.168.1.0');
const cidr = ref(24);
const result = ref<SubnetResult | null>(null);
const ipError = ref('');
let recalcTimer: ReturnType<typeof setTimeout> | null = null;

const QUICK_CIDRS = [8, 16, 24, 25, 26, 27, 28, 30, 32];

function ipToNum(ip: string): number {
  const parts = ip.split('.').map(Number);
  return ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0;
}

function numToIp(num: number): string {
  return [
    (num >>> 24) & 0xff,
    (num >>> 16) & 0xff,
    (num >>> 8) & 0xff,
    num & 0xff,
  ].join('.');
}

function toBinary(ip: string): string {
  return ip
    .split('.')
    .map((n) => parseInt(n, 10).toString(2).padStart(8, '0'))
    .join('.');
}

function isValidIp(ip: string): boolean {
  const parts = ip.split('.');
  if (parts.length !== 4) return false;
  return parts.every((p) => {
    if (!/^\d+$/.test(p)) return false;
    const n = parseInt(p, 10);
    return n >= 0 && n <= 255;
  });
}

function calculate(ip: string, prefix: number): SubnetResult {
  const ipNum = ipToNum(ip);
  const mask = prefix === 0 ? 0 : (~0 << (32 - prefix)) >>> 0;
  const wildcard = (~mask) >>> 0;
  const network = (ipNum & mask) >>> 0;
  const broadcast = (network | wildcard) >>> 0;
  const firstHost = prefix >= 31 ? network : network + 1;
  const lastHost = prefix >= 31 ? broadcast : broadcast - 1;
  const hostCount =
    prefix >= 31
      ? prefix === 32
        ? 1
        : 2
      : 2 ** (32 - prefix) - 2;

  return {
    networkAddr: numToIp(network),
    broadcastAddr: numToIp(broadcast),
    subnetMask: numToIp(mask),
    wildcardMask: numToIp(wildcard),
    firstHost: numToIp(firstHost),
    lastHost: numToIp(lastHost),
    hostCount,
  };
}

function runCalculate() {
  if (!isValidIp(ipInput.value)) {
    ipError.value = t('networkTools.subnet.ipError');
    result.value = null;
    return;
  }
  ipError.value = '';
  result.value = calculate(ipInput.value, cidr.value);
}

function selectCidr(value: number) {
  cidr.value = value;
  runCalculate();
}

async function copyValue(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    pushToast(t('networkTools.copy.copied'), 'success', { ttlMs: 1800 });
  } catch (error) {
    pushToast(t('networkTools.copy.failed', { error: String(error) }), 'error', { ttlMs: 3600 });
  }
}

watch([ipInput, cidr], () => {
  if (recalcTimer !== null) {
    clearTimeout(recalcTimer);
  }
  recalcTimer = setTimeout(() => {
    if (!isValidIp(ipInput.value)) {
      return;
    }
    ipError.value = '';
    result.value = calculate(ipInput.value, cidr.value);
  }, 200);
});

runCalculate();
</script>

<template>
  <div class="space-y-5">
    <div class="space-y-4">
      <div class="flex gap-3 items-end">
        <div class="flex-1 space-y-1">
          <label class="block text-sm font-medium text-slate-700">
            {{ t('networkTools.subnet.ipAddress') }}
          </label>
          <input
            v-model="ipInput"
            type="text"
            :placeholder="t('networkTools.subnet.ipPlaceholder')"
            @blur="runCalculate"
            @input="ipError = ''"
            class="w-full px-3 py-2 text-sm border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors"
            :class="ipError ? 'border-red-400 bg-red-50' : 'border-slate-300 bg-white'"
          >
          <p v-if="ipError" class="text-xs text-red-500">{{ ipError }}</p>
        </div>

        <div class="w-36 space-y-1">
          <label class="block text-sm font-medium text-slate-700">
            {{ t('networkTools.subnet.cidr') }}
          </label>
          <div class="flex items-center border border-slate-300 rounded-lg bg-white overflow-hidden focus-within:ring-2 focus-within:ring-blue-500 focus-within:border-transparent transition-colors">
            <span class="px-2.5 py-2 text-sm font-medium text-slate-500 select-none border-r border-slate-200 bg-slate-50">/</span>
            <input
              v-model.number="cidr"
              type="number"
              min="0"
              max="32"
              class="flex-1 px-2.5 py-2 text-sm bg-white focus:outline-none"
            >
          </div>
        </div>

        <button
          type="button"
          @click="runCalculate"
          class="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 transition-colors"
        >
          {{ t('networkTools.subnet.calculate') }}
        </button>
      </div>

      <div class="flex items-center gap-2 flex-wrap">
        <span class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.commonCidr') }}:</span>
        <button
          v-for="c in QUICK_CIDRS"
          :key="c"
          type="button"
          @click="selectCidr(c)"
          class="px-2.5 py-1 text-xs font-medium rounded-full border transition-colors"
          :class="
            cidr === c
              ? 'bg-blue-600 text-white border-blue-600'
              : 'bg-white text-slate-600 border-slate-300 hover:bg-slate-50 hover:border-slate-400'
          "
        >
          /{{ c }}
        </button>
      </div>
    </div>

    <div v-if="result" class="space-y-4">
      <div class="grid grid-cols-2 gap-3">
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.networkAddr') }}</p>
              <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.networkAddr }}</p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.networkAddr') })"
              @click="copyValue(result.networkAddr)"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.broadcastAddr') }}</p>
              <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.broadcastAddr }}</p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.broadcastAddr') })"
              @click="copyValue(result.broadcastAddr)"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.subnetMask') }}</p>
              <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.subnetMask }}</p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.subnetMask') })"
              @click="copyValue(result.subnetMask)"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.wildcardMask') }}</p>
              <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.wildcardMask }}</p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.wildcardMask') })"
              @click="copyValue(result.wildcardMask)"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.ipRange') }}</p>
              <p class="text-sm font-semibold text-slate-800 font-mono">
                {{ result.firstHost }} - {{ result.lastHost }}
              </p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.ipRange') })"
              @click="copyValue(`${result.firstHost} - ${result.lastHost}`)"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="bg-blue-50 border border-blue-200 rounded-lg p-3 space-y-0.5">
          <div class="flex items-start justify-between gap-3">
            <div>
              <p class="text-xs font-medium text-blue-500">{{ t('networkTools.subnet.hostCount') }}</p>
              <p class="text-xl font-bold text-blue-700">{{ result.hostCount.toLocaleString() }}</p>
            </div>
            <button
              type="button"
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-blue-200 bg-white text-blue-600 transition-colors hover:bg-blue-100 hover:text-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
              :title="t('networkTools.copy.field', { field: t('networkTools.subnet.hostCount') })"
              @click="copyValue(String(result.hostCount))"
            >
              <Copy class="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      <div class="border border-slate-200 rounded-lg p-3 space-y-2 bg-white">
        <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.binary') }}</p>
        <div class="font-mono text-xs space-y-1.5">
          <div class="flex items-baseline gap-2">
            <span class="w-10 text-slate-400 shrink-0">{{ t('networkTools.subnet.ipLabel') }}</span>
            <span class="break-all leading-relaxed">
              <template v-for="(octet, oi) in toBinary(result.networkAddr).split('.')" :key="oi">
                <span v-if="oi > 0" class="text-slate-300">.</span>
                <template v-for="(bit, bi) in octet.split('')" :key="bi">
                  <span :class="oi * 8 + bi < cidr ? 'text-blue-600' : 'text-slate-400'">{{ bit }}</span>
                </template>
              </template>
            </span>
          </div>

          <div class="flex items-baseline gap-2">
            <span class="w-10 text-slate-400 shrink-0">{{ t('networkTools.subnet.maskLabel') }}</span>
            <span class="break-all leading-relaxed">
              <template v-for="(octet, oi) in toBinary(result.subnetMask).split('.')" :key="oi">
                <span v-if="oi > 0" class="text-slate-300">.</span>
                <template v-for="(bit, bi) in octet.split('')" :key="bi">
                  <span :class="oi * 8 + bi < cidr ? 'text-blue-600' : 'text-slate-400'">{{ bit }}</span>
                </template>
              </template>
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
