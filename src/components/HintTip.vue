<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  text: string;
  title?: string;
}>();

const { t } = useI18n();
const open = ref(false);
const positioned = ref(false);
const triggerRef = ref<HTMLButtonElement | null>(null);
const tooltipRef = ref<HTMLElement | null>(null);
const tooltipStyle = ref({ left: '0px', top: '0px' });
const tipId = `hint-${useId().replace(/:/g, '')}`;
const accessibleLabel = computed(() => props.title
  ? `${t('common.hint')}: ${props.title}`
  : t('common.hint'));

const VIEWPORT_GAP = 8;
const TOOLTIP_GAP = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

async function updateTooltipPosition(): Promise<void> {
  if (!open.value) return;

  await nextTick();
  if (!open.value || !triggerRef.value || !tooltipRef.value) return;

  const triggerRect = triggerRef.value.getBoundingClientRect();
  const tooltipRect = tooltipRef.value.getBoundingClientRect();
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;

  const left = clamp(
    triggerRect.left + triggerRect.width / 2 - tooltipRect.width / 2,
    VIEWPORT_GAP,
    viewportWidth - tooltipRect.width - VIEWPORT_GAP,
  );
  const spaceBelow = viewportHeight - triggerRect.bottom - TOOLTIP_GAP - VIEWPORT_GAP;
  const spaceAbove = triggerRect.top - TOOLTIP_GAP - VIEWPORT_GAP;
  const placeAbove = tooltipRect.height > spaceBelow && spaceAbove > spaceBelow;
  const preferredTop = placeAbove
    ? triggerRect.top - tooltipRect.height - TOOLTIP_GAP
    : triggerRect.bottom + TOOLTIP_GAP;
  const top = clamp(
    preferredTop,
    VIEWPORT_GAP,
    viewportHeight - tooltipRect.height - VIEWPORT_GAP,
  );

  tooltipStyle.value = {
    left: `${Math.round(left)}px`,
    top: `${Math.round(top)}px`,
  };
  positioned.value = true;
}

function addPositionListeners(): void {
  window.addEventListener('resize', updateTooltipPosition);
  window.addEventListener('scroll', updateTooltipPosition, true);
}

function removePositionListeners(): void {
  window.removeEventListener('resize', updateTooltipPosition);
  window.removeEventListener('scroll', updateTooltipPosition, true);
}

watch(open, (isOpen) => {
  positioned.value = false;
  removePositionListeners();
  if (!isOpen) return;

  addPositionListeners();
  void updateTooltipPosition();
});

watch(() => [props.text, props.title], () => {
  if (open.value) void updateTooltipPosition();
});

onBeforeUnmount(removePositionListeners);
</script>

<template>
  <span class="inline-flex shrink-0 align-middle">
    <button
      ref="triggerRef"
      type="button"
      class="flex h-4 w-4 cursor-pointer items-center justify-center rounded-full border border-slate-300 bg-white text-[10px] font-bold leading-none text-slate-400 transition-colors hover:border-slate-400 hover:text-slate-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/45 focus-visible:ring-offset-2"
      :aria-label="accessibleLabel"
      :aria-describedby="tipId"
      :aria-expanded="open"
      @mouseenter="open = true"
      @mouseleave="open = false"
      @focus="open = true"
      @blur="open = false"
      @keydown.esc="open = false"
    >?</button>
    <Teleport to="body">
      <span
        v-show="open"
        :id="tipId"
        ref="tooltipRef"
        role="tooltip"
        :style="tooltipStyle"
        class="pointer-events-none fixed z-[9999] w-max max-w-80 overflow-y-auto rounded-lg bg-slate-900 px-3 py-2 text-left text-xs font-normal leading-5 text-white shadow-lg [max-height:calc(100vh-1rem)] [max-width:calc(100vw-1rem)]"
        :class="positioned ? 'visible opacity-100' : 'invisible opacity-0'"
      >
        <strong v-if="title" class="mb-1 block font-semibold">{{ title }}</strong>
        {{ text }}
      </span>
    </Teleport>
  </span>
</template>
