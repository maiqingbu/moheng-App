<script setup lang="ts">
defineProps<{ open: boolean; title: string; subtitle?: string }>()
defineEmits<{ close: [] }>()
</script>
<template>
  <Teleport to="body">
    <div v-if="open" class="drawer-backdrop" @click="$emit('close')" />
    <aside v-if="open" class="drawer" role="dialog" aria-modal="true">
      <div class="drawer-head"><div><h2 style="margin:0;font-size:18px">{{ title }}</h2><div v-if="subtitle" class="muted" style="font-size:12px">{{ subtitle }}</div></div><button class="icon-btn" @click="$emit('close')">×</button></div>
      <div class="drawer-body"><slot /></div>
    </aside>
  </Teleport>
</template>
