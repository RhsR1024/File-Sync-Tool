<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'SubnetCalcTab' });

const { t } = useI18n();

// ── Types ────────────────────────────────────────────────────────────────────

interface SubnetResult {
  networkAddr: string;
  broadcastAddr: string;
  subnetMask: string;
  wildcardMask: string;
  firstHost: string;
  lastHost: string;
  hostCount: number;
}

// ── State ────────────────────────────────────────────────────────────────────

const ipInput = ref('192.168.1.0');
const cidr = ref(24);
const result = ref<SubnetResult | null>(null);
const ipError = ref('');

const QUICK_CIDRS = [8, 16, 24, 25, 26, 27, 28, 30, 32];

// ── Calculation Logic ────────────────────────────────────────────────────────

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
    .map((n) => parseInt(n).toString(2).padStart(8, '0'))
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

// ── Auto-calculate on valid input ─────────────────────────────────────────────

watch([ipInput, cidr], () => {
  if (isValidIp(ipInput.value)) {
    ipError.value = '';
    result.value = calculate(ipInput.value, cidr.value);
  }
});

// ── Initial calculation ───────────────────────────────────────────────────────

runCalculate();
</script>

<template>
  <div class="space-y-5">
    <!-- Input Area -->
    <div class="space-y-4">
      <!-- IP + CIDR row -->
      <div class="flex gap-3 items-end">
        <!-- IP Address -->
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
          />
          <p v-if="ipError" class="text-xs text-red-500">{{ ipError }}</p>
        </div>

        <!-- CIDR Prefix -->
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
            />
          </div>
        </div>

        <!-- Calculate Button -->
        <button
          @click="runCalculate"
          class="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 transition-colors"
        >
          {{ t('networkTools.subnet.calculate') }}
        </button>
      </div>

      <!-- Quick CIDR Buttons -->
      <div class="flex items-center gap-2 flex-wrap">
        <span class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.commonCidr') }}:</span>
        <button
          v-for="c in QUICK_CIDRS"
          :key="c"
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

    <!-- Results Grid -->
    <div v-if="result" class="space-y-4">
      <div class="grid grid-cols-2 gap-3">
        <!-- Network Address -->
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.networkAddr') }}</p>
          <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.networkAddr }}</p>
        </div>

        <!-- Broadcast Address -->
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.broadcastAddr') }}</p>
          <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.broadcastAddr }}</p>
        </div>

        <!-- Subnet Mask -->
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.subnetMask') }}</p>
          <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.subnetMask }}</p>
        </div>

        <!-- Wildcard Mask -->
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.wildcardMask') }}</p>
          <p class="text-sm font-semibold text-slate-800 font-mono">{{ result.wildcardMask }}</p>
        </div>

        <!-- Usable IP Range -->
        <div class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.ipRange') }}</p>
          <p class="text-sm font-semibold text-slate-800 font-mono">
            {{ result.firstHost }} — {{ result.lastHost }}
          </p>
        </div>

        <!-- Usable Hosts — highlighted card -->
        <div class="bg-blue-50 border border-blue-200 rounded-lg p-3 space-y-0.5">
          <p class="text-xs font-medium text-blue-500">{{ t('networkTools.subnet.hostCount') }}</p>
          <p class="text-xl font-bold text-blue-700">{{ result.hostCount.toLocaleString() }}</p>
        </div>
      </div>

      <!-- Binary Representation -->
      <div class="border border-slate-200 rounded-lg p-3 space-y-2 bg-white">
        <p class="text-xs font-medium text-slate-500">{{ t('networkTools.subnet.binary') }}</p>
        <div class="font-mono text-xs space-y-1.5">
          <!-- IP Binary -->
          <div class="flex items-baseline gap-2">
            <span class="w-10 text-slate-400 shrink-0">{{ t('networkTools.subnet.ipLabel') }}</span>
            <span class="break-all leading-relaxed">
              <template v-for="(octet, oi) in toBinary(result.networkAddr).split('.')" :key="oi">
                <span v-if="oi > 0" class="text-slate-300">.</span>
                <template v-for="(bit, bi) in octet.split('')" :key="bi">
                  <span
                    :class="oi * 8 + bi < cidr ? 'text-blue-600' : 'text-slate-400'"
                  >{{ bit }}</span>
                </template>
              </template>
            </span>
          </div>

          <!-- Mask Binary -->
          <div class="flex items-baseline gap-2">
            <span class="w-10 text-slate-400 shrink-0">{{ t('networkTools.subnet.maskLabel') }}</span>
            <span class="break-all leading-relaxed">
              <template v-for="(octet, oi) in toBinary(result.subnetMask).split('.')" :key="oi">
                <span v-if="oi > 0" class="text-slate-300">.</span>
                <template v-for="(bit, bi) in octet.split('')" :key="bi">
                  <span
                    :class="oi * 8 + bi < cidr ? 'text-blue-600' : 'text-slate-400'"
                  >{{ bit }}</span>
                </template>
              </template>
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
