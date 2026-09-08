<script setup lang="ts">
import { computed } from 'vue'
import type { CommandUiState } from '../stores/projectStore'
import { commandMessage, statusLabel } from '../presentation/productLanguage'
const props = defineProps<{ state: CommandUiState }>()
const label = computed(() => statusLabel(props.state.status, ''))
const message = computed(() => commandMessage(props.state))
</script>
<template>
  <div v-if="state.status !== 'idle'" class="command-feedback" :class="state.status" role="status" aria-live="polite">
    <strong>{{ label }}</strong>
    <span v-if="message"> · {{ message }}</span>
  </div>
</template>
