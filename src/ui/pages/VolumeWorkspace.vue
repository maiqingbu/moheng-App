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
const state = computed(() => store.commandState('volume-build'))
const locked = computed(() => detail.value?.volume.status === 'LOCKED')
const taskReality = computed(() => planRoute.value?.taskReality ?? [])
const realityStatus = (taskId: unknown) => { const value = taskReality.value.find(item => String(item.stateId) === String(taskId))?.value as Record<string, unknown> | undefined; return String(value?.status ?? 'NOT_STARTED') }
const tasksByClass = (taskClass: string) => tasks.value.filter(task => String(task.class) === taskClass)

onMounted(() => { void store.loadVolumeDetail(volumeId.value); void store.loadPlanningRoute(volumeId.value) })
</script>

<template>
  <div v-if="detail" class="page">
    <PageHeader :title="`第 ${detail.volume.number} 卷 · ${detail.volume.title}`" :subtitle="detail.volume.goal">
    </PageHeader>
    <section class="card pad section"><span class="badge">PLAN</span><h2 class="card-title" style="margin-top:8px">Volume Goal</h2><p class="card-subtitle">{{ detail.volume.goal }}</p></section>
    <CommandFeedback :state="state" />

    <div v-if="locked" class="banner warning section"><div><div class="banner-title">当前卷仍处于准入锁定</div><div class="banner-copy">开篇评估 PASS 前可以查看全书战略中的卷意图，但不能创建详细卷计划或启动后续正文生产。</div></div></div>

    <div v-if="planRoute" class="metric-grid section">
      <MetricCard label="剩余容量" :value="String(planRoute.planningPressure.remainingChapterCapacity ?? '—')" note="PROJECTION · 当前 Route 的容量推导" />
      <MetricCard label="规划压力" :value="String(planRoute.planningPressure.priorityScore ?? '—')" note="PROJECTION · Required/阻塞/退出条件共同决定" />
      <MetricCard label="Route 版本" :value="String(planRoute.routeVersion)" note="PLAN · VolumePlan R1" />
      <MetricCard label="任务数" :value="String(tasks.length)" note="PLAN · Task Route；状态来自 Reality" />
    </div>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">Task Reality</h2><p class="card-subtitle">REALITY · 来源为 state_deltas，不由 VolumePlan 的计划字段推断。</p></div><span class="badge">REALITY</span></div>
      <div v-for="taskClass in ['REQUIRED', 'OPTIONAL', 'FLEXIBLE', 'BLOCKED']" :key="taskClass" class="section-subgroup"><strong>{{ taskClass }} · PLAN</strong><div class="timeline"><div v-for="task in tasksByClass(taskClass === 'BLOCKED' ? 'REQUIRED' : taskClass).filter(item => taskClass !== 'BLOCKED' || realityStatus(item.taskId) === 'BLOCKED')" :key="String(task.taskId)" class="timeline-item"><div class="timeline-marker">R</div><div><strong>{{ String(task.taskId) }}</strong><div class="muted" style="font-size:12px">REALITY · {{ realityStatus(task.taskId) }}</div></div></div></div></div>
    </section>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">Continuity / Reservations / Horizon</h2><p class="card-subtitle">PROJECTION 与 TENTATIVE 仅供规划参考，不改变 Canon 或 Task Reality。</p></div><span class="badge">PROJECTION</span></div>
      <dl class="pitch-kv"><div><dt>Open / Suspended Threads</dt><dd>{{ planRoute.continuityThreads?.length ?? 0 }}</dd></div><div><dt>Future Reservations</dt><dd>{{ planRoute.futureReservations?.length ?? 0 }}</dd></div><div><dt>Current ChapterPlan</dt><dd>{{ planRoute.currentChapterPlan?.artifactId ?? '未生成' }}</dd></div><div><dt>Soft Rolling Horizon</dt><dd>{{ planRoute.horizonPreview?.length ?? 0 }} slots · TENTATIVE</dd></div></dl>
    </section>

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
