<script setup lang="ts">
defineProps<{ title: string; subtitle?: string }>()
</script>
<template>
  <div class="page-head">
    <div><h1 class="page-title">{{ title }}</h1><p v-if="subtitle" class="page-lead">{{ subtitle }}</p></div>
    <div class="actions"><slot /></div>
  </div>
</template>
