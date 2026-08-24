<script setup lang="ts">
import { computed } from 'vue'
import type { CommandUiState } from '../stores/projectStore'
const props = defineProps<{ state: CommandUiState }>()
const label = computed(() => ({ submitting:'正在提交', accepted:'已接收', processing:'处理中', committed:'已完成', blocked:'已阻断', failed:'失败', idle:'' }[props.state.status]))
</script>
<template>
  <div v-if="state.status !== 'idle'" class="command-feedback" :class="state.status">
    <strong>{{ label }}</strong>
    <span> · {{ state.message }}</span>
  </div>
</template>
