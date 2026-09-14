<script setup lang="ts">
import { computed } from 'vue'

type StepStatus = 'active' | 'done' | 'pending' | 'failed'
interface RuntimeStep {
  id: string
  label: string
  status: StepStatus
}
interface RuntimeOperation {
  jobId: string
  kind: string
  phase: string
  status: string
  active: boolean
  steps?: RuntimeStep[]
}

const props = withDefaults(defineProps<{
  operation?: RuntimeOperation | null
  inFlight?: boolean
}>(), {
  operation: null,
  inFlight: false,
})

const visible = computed(() => props.inFlight || props.operation != null)
const badgeLabel = computed(() => {
  const operation = props.operation
  if (props.inFlight && operation?.kind === 'B03::VOLUME_PLAN' && !operation.active && operation.status === 'COMPLETED') return '正在同步规划结果…'
  if (props.inFlight && operation?.kind === 'B03::VOLUME_INTENT' && !operation.active) return '正在等待规划阶段…'
  if (operation?.active) return operation.kind === 'B03::VOLUME_PLAN' ? '正在规划本卷任务路线' : '正在建立当前卷目标'
  if (operation?.status === 'FAILED') return '规划未完成'
  if (operation?.status === 'COMPLETED') return '规划已完成'
  return props.inFlight ? '正在等待规划阶段…' : '等待规划'
})

const badgeClass = computed(() => props.operation?.active ? 'warning' : props.operation?.status === 'FAILED' ? 'danger' : 'success')
const steps = computed(() => props.operation?.steps ?? [])
const stepStatusLabel = (status: StepStatus) => status === 'active' ? '正在处理' : status === 'done' ? '已完成' : status === 'failed' ? '处理失败' : '等待处理'
const stepMarker = (status: StepStatus) => status === 'done' ? '✓' : status === 'failed' ? '!' : status === 'active' ? '…' : '·'
</script>

<template>
  <section v-if="visible" class="card pad section" data-volume-planning-progress>
    <div class="card-head"><div><h2 class="card-title">当前卷规划进度</h2><p class="card-subtitle">阶段来自后端持久化运行回执，页面只轮询投影。</p></div><span class="badge" :class="badgeClass">{{ badgeLabel }}</span></div>
    <div v-if="operation" class="timeline">
      <div v-for="step in steps" :key="step.id" class="timeline-item" :class="step.status">
        <div class="timeline-marker">{{ stepMarker(step.status) }}</div>
        <div><strong>{{ step.label }}</strong><div class="muted" style="font-size:12px">{{ stepStatusLabel(step.status) }}</div></div>
      </div>
    </div>
    <div v-else class="muted">正在等待后端规划阶段…</div>
  </section>
</template>
