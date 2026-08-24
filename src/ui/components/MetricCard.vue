<script setup lang="ts">
defineProps<{ label: string; value: string | number; note?: string }>()
</script>
<template><div class="metric"><div class="metric-label">{{ label }}</div><div class="metric-value">{{ value }}</div><div v-if="note" class="metric-note">{{ note }}</div></div></template>
