<script setup lang="ts">
defineProps<{ caption: string }>()
</script>

<template>
  <div class="loading-block" role="status" aria-live="polite">
    <div class="loading-caption"><span class="spinner" aria-hidden="true" />{{ caption }}</div>
    <div class="skeleton short" />
    <div class="skeleton tall" />
    <div class="skeleton tall" />
  </div>
</template>
