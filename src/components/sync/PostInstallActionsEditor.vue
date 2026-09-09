<script setup lang="ts">
import { computed } from 'vue';
import { KeyRound, Network, RotateCw, ShieldCheck } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import type { PostInstallActions } from '@/lib/tauri';

const model = defineModel<PostInstallActions>({ required: true });
const { t } = useI18n();

const replicaText = computed({
  get: () => model.value.topology.replica_ips.join('\n'),
  set: (value: string) => {
    model.value.topology.replica_ips = Array.from(new Set(
      value.split(/[\s,;]+/).map(item => item.trim()).filter(Boolean),
    ));
  },
});
</script>

<template>
  <section class="rounded-xl border border-indigo-200 bg-indigo-50/40 p-4">
    <label class="flex min-h-11 cursor-pointer items-start gap-3">
      <input v-model="model.enabled" type="checkbox" class="mt-1 h-4 w-4 rounded border-slate-300 text-indigo-600 focus:ring-indigo-500" />
      <span>
        <span class="flex items-center gap-2 text-sm font-semibold text-indigo-950">
          <RotateCw class="h-4 w-4" />{{ t('settings.postInstall.title') }}
        </span>
        <span class="mt-1 block text-xs leading-5 text-indigo-800">{{ t('settings.postInstall.description') }}</span>
      </span>
    </label>

    <div v-if="model.enabled" class="mt-4 space-y-4 border-t border-indigo-200 pt-4">
      <div class="grid gap-3 md:grid-cols-2">
        <label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-lg border border-sky-200 bg-white px-3 py-2 text-sm font-medium text-slate-700">
          <input v-model="model.enable_ssh" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-500" />
          <ShieldCheck class="h-4 w-4 text-sky-600" />{{ t('settings.postInstall.enableSsh') }}
        </label>
        <label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-lg border border-teal-200 bg-white px-3 py-2 text-sm font-medium text-slate-700">
          <input v-model="model.passwords.enabled" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-teal-600 focus:ring-teal-500" />
          <KeyRound class="h-4 w-4 text-teal-600" />{{ t('settings.postInstall.changePasswords') }}
        </label>
      </div>
      <p v-if="model.enable_ssh" class="text-xs text-sky-800">{{ t('settings.postInstall.sshHint') }}</p>

      <div v-if="model.passwords.enabled" class="rounded-lg border border-teal-200 bg-white p-3">
        <div class="grid gap-3 xl:grid-cols-3">
          <fieldset v-for="kind in (['framework', 'ums', 'cdm'] as const)" :key="kind" class="rounded-lg border border-slate-200 p-3">
            <label class="flex min-h-11 cursor-pointer items-center gap-2 text-sm font-semibold text-slate-700">
              <input v-model="model.passwords[kind]" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-teal-600 focus:ring-teal-500" />
              {{ t(`settings.postInstall.${kind}`) }}
            </label>
            <div v-if="model.passwords[kind]" class="mt-2 space-y-2">
              <label v-if="kind === 'ums'" class="block text-xs text-slate-600">
                {{ t('settings.postInstall.username') }}
                <input v-model="model.passwords.ums_username" autocomplete="username" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-teal-500/40" />
              </label>
              <label class="block text-xs text-slate-600">
                {{ t('settings.postInstall.oldPassword') }}
                <input v-model="model.passwords[`${kind}_old_password`]" type="text" autocomplete="current-password" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-teal-500/40" />
              </label>
              <label class="block text-xs text-slate-600">
                {{ t('settings.postInstall.newPassword') }}
                <input v-model="model.passwords[`${kind}_new_password`]" type="text" autocomplete="new-password" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-teal-500/40" />
              </label>
            </div>
          </fieldset>
        </div>
      </div>

      <div class="rounded-lg border border-fuchsia-200 bg-white p-3">
        <label class="flex min-h-11 cursor-pointer items-center gap-3 text-sm font-semibold text-slate-700">
          <input v-model="model.topology.enabled" type="checkbox" class="h-4 w-4 rounded border-slate-300 text-fuchsia-600 focus:ring-fuchsia-500" />
          <Network class="h-4 w-4 text-fuchsia-600" />{{ t('settings.postInstall.configureTopology') }}
        </label>
        <div v-if="model.topology.enabled" class="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          <label class="text-xs text-slate-600">{{ t('settings.topologyMode') }}
            <select v-model="model.topology.mode" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm">
              <option value="ha">{{ t('settings.topologyModeHa') }}</option>
              <option value="replica">{{ t('settings.topologyModeReplica') }}</option>
              <option value="ha_and_replica">{{ t('settings.topologyModeCombined') }}</option>
            </select>
          </label>
          <label class="text-xs text-slate-600">{{ t('settings.topologyPrimaryIp') }}
            <input v-model="model.topology.primary_ip" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm" placeholder="192.115.1.17" />
          </label>
          <template v-if="model.topology.mode !== 'replica'">
            <label class="text-xs text-slate-600">{{ t('settings.topologyHaReplicaIp') }}
              <input v-model="model.topology.ha_replica_ip" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm" placeholder="192.115.1.55" />
            </label>
            <label class="text-xs text-slate-600">{{ t('settings.topologyVirtualIp') }}
              <input v-model="model.topology.virtual_ip" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm" placeholder="192.115.1.128" />
            </label>
          </template>
          <label v-if="model.topology.mode !== 'ha'" class="text-xs text-slate-600">{{ t('settings.topologyReplicaIps') }}
            <textarea v-model="replicaText" rows="3" class="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm" placeholder="192.115.1.18" />
          </label>
          <label class="text-xs text-slate-600">{{ t('settings.topologyFrameworkPassword') }}
            <input v-model="model.topology.framework_password" type="text" autocomplete="current-password" class="mt-1 min-h-11 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm" />
          </label>
        </div>
      </div>

      <p class="text-xs text-indigo-800">{{ t('settings.postInstall.pollHint', { seconds: model.poll_interval_secs, attempts: model.poll_attempts }) }}</p>
    </div>
  </section>
</template>
