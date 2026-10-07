<script setup lang="ts">
import { computed } from 'vue'
import type { CommandUiState } from '../stores/projectStore'
import { commandMessage, statusLabel } from '../presentation/productLanguage'
const props = defineProps<{ state: CommandUiState; onConfirm?: () => unknown }>()
const label = computed(() => props.state.confirmation === 'pending' ? '结果待确认' : statusLabel(props.state.status, ''))
const message = computed(() => commandMessage(props.state))
const sourceKindLabels: Record<string, string> = {
  CONTRACT: '目标合同',
  ENTITY: '人物或实体',
  CANON: '已确认事实',
  KNOWLEDGE: '人物认知',
  CONTINUITY: '剧情责任',
  TASK: '卷内任务',
  RESERVATION: '后续预留',
  EVIDENCE: '依据证据',
}
const sourceName = (source: NonNullable<CommandUiState['contextBudget']>['sourceSizes'][number]) =>
  [
    sourceKindLabels[source.kind] ?? '必需来源',
    [...new Set([source.fieldPath, ...(source.additionalFieldPaths ?? [])].filter(Boolean))].join(' / ')
      || source.selectionRef
      || [source.sourceRef, ...(source.additionalSourceRefs ?? []).map(ref => ref.split('@', 1)[0])].join(', '),
  ].join(' · ')
</script>
<template>
  <div v-if="state.status !== 'idle'" class="command-feedback" :class="state.confirmation === 'pending' ? 'blocked' : state.status" role="status" aria-live="polite">
    <strong>{{ label }}</strong>
    <span v-if="message"> · {{ message }}</span>
    <ul v-if="state.contextBudget?.sourceSizes.length" class="context-budget-sources" data-context-budget-sources>
      <li v-for="source in state.contextBudget.sourceSizes" :key="[source.kind, source.sourceRef, source.fieldPath ?? ''].join(':')">
        {{ sourceName(source) }}：{{ source.estimatedTokens }} token / {{ source.serializedBytes }} 字节
      </li>
    </ul>
    <button v-if="state.confirmation === 'pending' && onConfirm" class="btn small" @click="onConfirm?.()">重新读取结果</button>
  </div>
</template>
