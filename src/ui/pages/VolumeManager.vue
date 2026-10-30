<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { createVolumePlanningRuntimePoller } from '../runtime/volumePlanningRuntimeSync'
import { executeVolumePlanningSequence } from '../runtime/volumePlanningOrchestration'
import VolumePlanningProgress from '../components/VolumePlanningProgress.vue'
import type { VolumeDetailProjection } from '../../application/contracts'

const store = useProjectStore()
const router = useRouter()
const items = computed(() => store.volumes?.items ?? [])
const state = computed(() => store.commandState('volume-build'))
const signingPassed = computed(() => store.project?.signingStatus === 'PASS')
const currentVolume = computed(() => items.value.find(volume => volume.status !== 'CLOSED') ?? items.value[0] ?? null)
const runtimeOperation = computed(() => store.volumeDetail?.runtimeOperation ?? null)
const planningInFlight = computed(() => ['submitting', 'accepted', 'processing'].includes(state.value.status))
const buildInFlight = ref(false)
const planningActive = computed(() => buildInFlight.value || planningInFlight.value || runtimeOperation.value?.active === true)
const actionLabel = computed(() => planningActive.value ? '查看当前卷规划进度' : '建立当前卷目标')
const actionDisabled = computed(() => !signingPassed.value && !planningActive.value)
const signingLabel = (value: boolean) => value ? '已通过' : '尚未通过'
const volumeStatusLabel = (value: string) => ({ LOCKED: '待解锁', CLOSED: '已关闭', PRODUCING: '生产中', PAUSED: '已暂停', PLANNING: '规划中' } as Record<string, string>)[value] ?? '待确认'
const runtimePollerOptions = {
  query: async () => {
    if (!currentVolume.value) throw new Error('当前没有可规划的卷。')
    const value = await store.loadVolumeDetail(currentVolume.value.id, true)
    if (!value) throw new Error('当前卷状态暂不可用。')
    return value
  },
  onProjection: (projection: VolumeDetailProjection) => { store.volumeDetail = projection },
}
const intentRuntimePoller = createVolumePlanningRuntimePoller(runtimePollerOptions)
const planRuntimePoller = createVolumePlanningRuntimePoller(runtimePollerOptions)

onMounted(() => {
  void (async () => {
    const loaded = await store.loadVolumes()
    const current = loaded?.items.find(volume => volume.status !== 'CLOSED') ?? loaded?.items[0]
    if (current) await store.loadVolumeDetail(current.id)
    if (store.volumeDetail?.runtimeOperation?.active) {
      const poller = store.volumeDetail.runtimeOperation.kind === 'B03::VOLUME_PLAN' ? planRuntimePoller : intentRuntimePoller
      poller.start()
    }
  })()
})
onUnmounted(() => { intentRuntimePoller.stop(); planRuntimePoller.stop() })

function focusPlanningProgress() {
  document.querySelector<HTMLElement>('[data-volume-planning-progress]')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

async function build() {
  const current = currentVolume.value
  if (!current) return
  if (!signingPassed.value && !planningActive.value) return
  if (planningActive.value) {
    focusPlanningProgress()
    return
  }
  buildInFlight.value = true
  try {
    await store.refresh()
    await store.loadVolumes(true)
    await store.loadVolumeDetail(current.id, true)
    await executeVolumePlanningSequence({
      startIntent: options => intentRuntimePoller.start(options),
      stopIntent: () => intentRuntimePoller.stop(),
      startPlan: options => planRuntimePoller.start(options),
      stopPlan: () => planRuntimePoller.stop(),
      runIntent: () => store.runCommand('volume-build', { type: 'planning_create_volume_intent', payload: { volumeId: current.id } }),
      runPlan: () => store.runCommand('volume-build', { type: 'planning_create_volume_plan', payload: { volumeId: current.id } }),
    })
    await store.loadVolumes(true)
    await store.loadVolumeDetail(current.id, true)
  } catch {
    // Store-owned feedback keeps the typed failure visible.
  } finally {
    buildInFlight.value = false
    if (!store.volumeDetail?.runtimeOperation?.active) {
      intentRuntimePoller.stop()
      planRuntimePoller.stop()
    }
  }
}
</script>

<template>
  <div class="page">
    <PageHeader title="当前卷" subtitle="区分规划、实际进展、系统推算与暂定安排，不把计划当成已经发生的事实。">
      <button class="btn primary" :aria-disabled="actionDisabled" @click="build">{{ actionLabel }}</button>
    </PageHeader>
    <CommandFeedback :state="state" />

    <VolumePlanningProgress :operation="runtimeOperation" :in-flight="buildInFlight" />

    <div v-if="!signingPassed" class="banner warning section">
      <div><div class="banner-title">卷级生产尚未解锁</div><div class="banner-copy">开篇评估通过并生成正式回执后，才能建立详细卷计划和启动后续长篇生产。</div></div>
      <button class="btn" @click="router.push(`/projects/${store.currentProjectId}/production`)">去开篇评估</button>
    </div>

    <div class="section" style="display:grid;gap:12px">
      <article v-for="volume in items" :key="volume.id" class="card pad">
        <div class="card-head">
          <div><span class="badge" :class="volume.health === 'risk' ? 'danger' : volume.health === 'attention' ? 'warning' : 'success'">第 {{ volume.number }} 卷 · {{ volumeStatusLabel(volume.status) }}</span><h2 class="card-title" style="margin-top:8px">{{ volume.title }}</h2><p class="card-subtitle">{{ volume.goal }}</p></div>
          <button class="btn" @click="router.push(`/projects/${store.currentProjectId}/volumes/${volume.id}`)">进入卷工作区</button>
        </div>
          <div class="muted" style="font-size:11px;margin-top:8px">已提交正文 {{ volume.wordCount.toLocaleString() }} 字；进入卷工作区查看任务安排、实际进展与动态规划压力。</div>
      </article>
    </div>
  </div>
</template>
