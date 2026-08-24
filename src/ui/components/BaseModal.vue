<script setup lang="ts">
defineProps<{ open: boolean; title: string; subtitle?: string }>()
defineEmits<{ close: [] }>()
</script>
<template>
  <Teleport to="body">
    <div v-if="open" class="modal-backdrop" @click="$emit('close')" />
    <div v-if="open" class="modal-wrap" role="dialog" aria-modal="true">
      <div class="modal">
        <div class="modal-head"><div><h2 style="margin:0;font-size:18px">{{ title }}</h2><div v-if="subtitle" class="muted" style="font-size:12px">{{ subtitle }}</div></div><button class="icon-btn" @click="$emit('close')">×</button></div>
        <div class="modal-body"><slot /></div>
        <div class="modal-foot"><slot name="footer" /></div>
      </div>
    </div>
  </Teleport>
</template>
