<script setup lang="ts">
import { computed } from 'vue'
import type { CommandUiState } from '../stores/projectStore'
import { commandMessage, statusLabel } from '../presentation/productLanguage'
const props = defineProps<{ state: CommandUiState; onConfirm?: () => unknown }>()
const label = computed(() => props.state.confirmation === 'pending' ? '结果待确认' : statusLabel(props.state.status, ''))
const message = computed(() => commandMessage(props.state))
</script>
<template>
  <div v-if="state.status !== 'idle'" class="command-feedback" :class="state.confirmation === 'pending' ? 'blocked' : state.status" role="status" aria-live="polite">
    <strong>{{ label }}</strong>
    <span v-if="message"> · {{ message }}</span>
    <button v-if="state.confirmation === 'pending' && onConfirm" class="btn small" @click="onConfirm?.()">重新读取结果</button>
  </div>
</template>
