<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const route = useRoute()
const volumeId = computed(() => String(route.params.volumeId ?? ''))
const detail = computed(() => store.volumeDetail)
const planRoute = computed(() => store.planningRoute)
const tasks = computed(() => planRoute.value?.tasks ?? [])
const state = computed(() => store.commandState('volume-command'))
const locked = computed(() => detail.value?.volume.status === 'LOCKED')
const canPause = computed(() => detail.value?.volume.status === 'PRODUCING')
const canClose = computed(() => !!detail.value && !locked.value && detail.value.volume.status !== 'CLOSED')

onMounted(() => { void store.loadVolumeDetail(volumeId.value); void store.loadPlanningRoute(volumeId.value) })
async function run(type: 'volume_production_start' | 'volume_production_pause' | 'volume_close') {
  await store.runCommand('volume-command', { type, payload: { volumeId: volumeId.value } })
  await Promise.all([store.loadVolumeDetail(volumeId.value, true), store.loadPlanningRoute(volumeId.value, true)])
}
</script>

<template>
  <div v-if="detail" class="page">
    <PageHeader :title="`第 ${detail.volume.number} 卷 · ${detail.volume.title}`" :subtitle="detail.volume.goal">
      <button class="btn primary" :disabled="locked" @click="run('volume_production_start')">开始/继续本卷</button>
      <button class="btn" :disabled="!canPause" @click="run('volume_production_pause')">暂停本卷</button>
      <button class="btn" :disabled="!canClose" @click="run('volume_close')">运行卷关闭检查</button>
    </PageHeader>
    <CommandFeedback :state="state" />

    <div v-if="locked" class="banner warning section"><div><div class="banner-title">当前卷仍处于准入锁定</div><div class="banner-copy">开篇评估 PASS 前可以查看全书战略中的卷意图，但不能创建详细卷计划或启动后续正文生产。</div></div></div>

    <div v-if="planRoute" class="metric-grid section">
      <MetricCard label="剩余容量" :value="String(planRoute.planningPressure.remainingChapterCapacity ?? '—')" note="PROJECTION · 当前 Route 的容量推导" />
      <MetricCard label="规划压力" :value="String(planRoute.planningPressure.priorityScore ?? '—')" note="PROJECTION · Required/阻塞/退出条件共同决定" />
      <MetricCard label="Route 版本" :value="String(planRoute.routeVersion)" note="PLAN · VolumePlan R1" />
      <MetricCard label="任务数" :value="String(tasks.length)" note="PLAN · Task Route；状态来自 Reality" />
    </div>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">本卷 Task Route</h2><p class="card-subtitle">PLAN 与 REALITY 分离；不以静态章表、Arc 时间线或百分比作为叙事真值。</p></div><span class="badge">PLAN</span></div>
      <div class="timeline">
        <div v-for="task in tasks" :key="String(task.taskId)" class="timeline-item">
          <div class="timeline-marker">{{ String(task.class).slice(0, 1) }}</div>
          <div><strong>{{ String(task.objective) }}</strong><div class="muted" style="font-size:12px">PLAN · {{ String(task.class) }} · completion predicate: {{ String((task.completionPredicate as Record<string, unknown>)?.kind ?? '—') }}</div></div>
        </div>
      </div>
    </section>

    <section v-if="store.advancedMode" class="card pad section">
      <h2 class="card-title">高级详情</h2>
      <dl v-if="planRoute" class="pitch-kv"><div><dt>VolumeIntent</dt><dd>{{ planRoute.volumeIntentRef.artifactId }} · v{{ planRoute.volumeIntentRef.version }}</dd></div><div><dt>VolumePlan</dt><dd>{{ planRoute.volumePlanRef.artifactId }} · v{{ planRoute.volumePlanRef.version }}</dd></div><div><dt>Project revision</dt><dd>{{ planRoute.projectRevision }}</dd></div></dl>
    </section>
  </div>
</template>
