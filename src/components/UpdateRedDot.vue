<script setup lang="ts">
import { ArrowUp } from 'lucide-vue-next';
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'UpdateRedDot' });

/**
 * halo    呼吸光环：琥珀胶囊 + NEW，整体缓慢明暗呼吸
 * bounce  跳动箭头：上行箭头轻跳，语义直指“可升级”
 * radar   雷达脉冲：极简圆点 + 双层扩散环
 * shimmer 流光胶囊：显示目标版本号，光带扫过 + 描边呼吸
 */
type UpdateBadgeVariant = 'halo' | 'bounce' | 'radar' | 'shimmer';

const props = withDefaults(
  defineProps<{
    variant?: UpdateBadgeVariant;
    version?: string | null;
  }>(),
  {
    variant: 'bounce',
    version: null,
  },
);

const { t } = useI18n();

const shimmerLabel = computed(() => props.version ?? 'NEW');
</script>

<template>
  <span class="update-badge" role="status" :aria-label="t('sidebar.updateAvailable')">
    <template v-if="variant === 'halo'">
      <span class="halo" aria-hidden="true">
        <span class="halo-dot"></span>
        NEW
      </span>
    </template>

    <template v-else-if="variant === 'bounce'">
      <span class="bounce" aria-hidden="true">
        <ArrowUp class="h-3 w-3" stroke-width="2.75" />
      </span>
    </template>

    <template v-else-if="variant === 'radar'">
      <span class="radar" aria-hidden="true">
        <span class="radar-ring"></span>
        <span class="radar-ring radar-ring-delayed"></span>
        <span class="radar-core"></span>
      </span>
    </template>

    <template v-else>
      <span class="shimmer" aria-hidden="true">
        <span class="shimmer-label">{{ shimmerLabel }}</span>
        <span class="shimmer-sweep"></span>
      </span>
    </template>

    <span class="sr-only">{{ t('sidebar.updateAvailable') }}</span>
  </span>
</template>

<style scoped>
.update-badge {
  display: inline-flex;
  flex: none;
  align-items: center;
}

/* ---------- halo：呼吸光环 ---------- */
.halo {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border-radius: 9999px;
  background: linear-gradient(135deg, rgba(251, 191, 36, 0.22), rgba(245, 158, 11, 0.14));
  border: 1px solid rgba(251, 191, 36, 0.45);
  padding: 2px 8px 2px 6px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.12em;
  line-height: 1.2;
  color: rgb(253, 230, 138);
  animation: update-halo-breathe 2.6s ease-in-out infinite;
}

.halo-dot {
  height: 5px;
  width: 5px;
  border-radius: 9999px;
  background: rgb(252, 211, 77);
}

@keyframes update-halo-breathe {
  0%,
  100% {
    transform: scale(1);
    box-shadow: 0 0 0 0 rgba(251, 191, 36, 0.3);
    filter: brightness(0.92);
  }
  50% {
    transform: scale(1.05);
    box-shadow: 0 0 14px 1px rgba(251, 191, 36, 0.45);
    filter: brightness(1.12);
  }
}

/* ---------- bounce：跳动箭头 ---------- */
.bounce {
  display: inline-flex;
  height: 20px;
  width: 20px;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  background: linear-gradient(160deg, rgb(125, 211, 252), rgb(34, 211, 238));
  color: rgb(8, 47, 73);
  animation: update-bounce-hop 2s cubic-bezier(0.28, 0.84, 0.42, 1) infinite;
}

@keyframes update-bounce-hop {
  0%,
  55%,
  100% {
    transform: translateY(0) scale(1);
    box-shadow: 0 0 0 0 rgba(34, 211, 238, 0);
  }
  12% {
    transform: translateY(-4px) scale(1.06, 1.08);
    box-shadow: 0 6px 14px -4px rgba(34, 211, 238, 0.7);
  }
  28% {
    transform: translateY(0) scale(1.08, 0.9);
    box-shadow: 0 0 0 4px rgba(34, 211, 238, 0.18);
  }
  40% {
    transform: translateY(-2px) scale(1, 1.03);
    box-shadow: 0 3px 10px -4px rgba(34, 211, 238, 0.5);
  }
}

/* ---------- radar：雷达脉冲 ---------- */
.radar {
  position: relative;
  display: inline-flex;
  height: 8px;
  width: 8px;
  align-items: center;
  justify-content: center;
}

.radar-core {
  position: relative;
  height: 8px;
  width: 8px;
  border-radius: 9999px;
  background: rgb(251, 191, 36);
  box-shadow: 0 0 8px rgba(251, 191, 36, 0.65);
  animation: update-radar-core 2.4s ease-in-out infinite;
}

.radar-ring {
  position: absolute;
  inset: 0;
  border-radius: 9999px;
  border: 1px solid rgba(251, 191, 36, 0.8);
  animation: update-radar-ping 2.4s cubic-bezier(0, 0, 0.2, 1) infinite;
}

.radar-ring-delayed {
  animation-delay: 1.2s;
}

@keyframes update-radar-ping {
  0% {
    transform: scale(0.7);
    opacity: 0.85;
  }
  70%,
  100% {
    transform: scale(3);
    opacity: 0;
  }
}

@keyframes update-radar-core {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(1.2);
  }
}

/* ---------- shimmer：流光胶囊 ---------- */
.shimmer {
  position: relative;
  display: inline-flex;
  align-items: center;
  overflow: hidden;
  border-radius: 9999px;
  border: 1px solid rgba(56, 189, 248, 0.4);
  background: rgba(8, 47, 73, 0.55);
  padding: 2px 8px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.06em;
  line-height: 1.2;
  color: rgb(186, 230, 253);
  animation: update-shimmer-breathe 3s ease-in-out infinite;
}

.shimmer-label {
  position: relative;
  z-index: 1;
  font-variant-numeric: tabular-nums;
}

.shimmer-sweep {
  position: absolute;
  inset: 0;
  background: linear-gradient(100deg, transparent 20%, rgba(224, 242, 254, 0.55) 50%, transparent 80%);
  animation: update-shimmer-sweep 3s ease-in-out infinite;
}

@keyframes update-shimmer-sweep {
  0% {
    transform: translateX(-130%);
  }
  45%,
  100% {
    transform: translateX(130%);
  }
}

@keyframes update-shimmer-breathe {
  0%,
  100% {
    border-color: rgba(56, 189, 248, 0.35);
    box-shadow: 0 0 8px rgba(56, 189, 248, 0.1);
  }
  50% {
    border-color: rgba(103, 232, 249, 0.75);
    box-shadow: 0 0 16px rgba(56, 189, 248, 0.32);
  }
}

@media (prefers-reduced-motion: reduce) {
  .halo,
  .bounce,
  .radar-core,
  .radar-ring,
  .shimmer,
  .shimmer-sweep {
    animation: none;
  }

  .shimmer-sweep {
    opacity: 0;
  }

  .radar-ring {
    opacity: 0.5;
  }
}
</style>
