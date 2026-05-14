<script setup lang="ts">
defineProps<{
  message: string;
}>();
</script>

<template>
  <transition name="flash">
    <div v-if="message" class="flash" role="status" aria-live="polite">
      {{ message }}
    </div>
  </transition>
</template>

<style scoped>
.flash-enter-active,
.flash-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.flash-enter-from,
.flash-leave-to {
  opacity: 0;
  transform: translate(-50%, 8px);
}

.flash-enter-to,
.flash-leave-from {
  opacity: 1;
  transform: translate(-50%, 0);
}
</style>
