<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  Info,
  X,
  type LucideIcon,
} from 'lucide-vue-next';

import type { Toast } from '@/composables/useToast';

defineOptions({ name: 'Toast' });

const props = defineProps<{ toast: Toast }>();
const emit = defineEmits<{ (e: 'dismiss', id: string): void }>();

const { t } = useI18n();

interface ToneStyle {
  borderClass: string;
  iconClass: string;
  icon: LucideIcon;
  role: 'status' | 'alert';
  ariaLive: 'polite' | 'assertive';
}

const TONE_STYLES: Record<Toast['tone'], ToneStyle> = {
  success: {
    borderClass: 'border-l-emerald-500',
    iconClass: 'text-emerald-500',
    icon: CheckCircle2,
    role: 'status',
    ariaLive: 'polite',
  },
  error: {
    borderClass: 'border-l-rose-500',
    iconClass: 'text-rose-500',
    icon: AlertCircle,
    role: 'alert',
    ariaLive: 'assertive',
  },
  warning: {
    borderClass: 'border-l-amber-500',
    iconClass: 'text-amber-500',
    icon: AlertTriangle,
    role: 'alert',
    ariaLive: 'assertive',
  },
  info: {
    borderClass: 'border-l-indigo-500',
    iconClass: 'text-indigo-500',
    icon: Info,
    role: 'status',
    ariaLive: 'polite',
  },
};

const tone = computed(() => TONE_STYLES[props.toast.tone]);
const dismissLabel = computed(() => t('common.toast.dismiss'));

function onDismiss() {
  emit('dismiss', props.toast.id);
}

function onAction() {
  props.toast.action?.onClick();
  emit('dismiss', props.toast.id);
}
</script>

<template>
  <div
    :role="tone.role"
    :aria-live="tone.ariaLive"
    class="flex w-[320px] max-w-[420px] gap-3 rounded-xl border border-l-4 border-slate-200 bg-white px-4 py-3 shadow-[0_14px_40px_rgba(15,23,42,0.12)]"
    :class="tone.borderClass"
  >
    <component :is="tone.icon" class="mt-0.5 h-5 w-5 flex-shrink-0" :class="tone.iconClass" aria-hidden="true" />
    <div class="min-w-0 flex-1 text-sm leading-relaxed text-slate-700">
      <p class="break-words">{{ toast.message }}</p>
      <button
        v-if="toast.action"
        type="button"
        class="mt-1.5 inline-flex items-center text-xs font-semibold text-indigo-600 transition-colors duration-150 hover:text-indigo-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
        @click="onAction"
      >
        {{ toast.action.label }}
      </button>
    </div>
    <button
      type="button"
      class="-mr-1 -mt-1 inline-flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-lg text-slate-400 transition-colors duration-150 hover:bg-slate-100 hover:text-slate-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
      :aria-label="dismissLabel"
      :title="dismissLabel"
      @click="onDismiss"
    >
      <X class="h-4 w-4" aria-hidden="true" />
    </button>
  </div>
</template>
