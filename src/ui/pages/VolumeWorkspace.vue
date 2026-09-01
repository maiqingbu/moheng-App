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
const lifecycle = computed(() => store.volumeLifecycle)
const planningState = computed(() => lifecycle.value?.planningState ?? {})
const tasks = computed(() => planRoute.value?.tasks ?? [])
const state = computed(() => store.commandState('volume-build'))
const closeState = computed(() => store.commandState('volume-close'))
const locked = computed(() => detail.value?.volume.status === 'LOCKED')
const taskReality = computed(() => planRoute.value?.taskReality ?? [])
const realityStatus = (taskId: unknown) => { const value = taskReality.value.find(item => String(item.stateId) === String(taskId))?.value as Record<string, unknown> | undefined; return String(value?.status ?? 'NOT_STARTED') }
const tasksByClass = (taskClass: string) => tasks.value.filter(task => String(task.class) === taskClass)
const closeReady = computed(() => planningState.value.closeReadiness === 'READY')
const remainingTasks = computed(() => Array.isArray(planningState.value.remainingTaskIds) ? planningState.value.remainingTaskIds as string[] : [])
const responsibilities = computed(() => Array.isArray(planningState.value.unresolvedResponsibilities) ? planningState.value.unresolvedResponsibilities as Array<Record<string, unknown>> : [])
const routeReplans = computed(() => lifecycle.value?.routeReplans ?? [])
const lifecycleOwner = 'B09::VolumeLifecycleOwner'
const exitSnapshotKind = 'VolumeExitState'
const unresolvedLabel = 'OPEN / SUSPENDED'
const handoffFields = ['fromVolumeId', 'toVolumeId', 'handoffStatus']
const statusLabel = (value: unknown) => ({ READY: '已满足', BLOCKED: '待完成', OPEN: '进行中', SUSPENDED: '已挂起', CLOSED: '已关闭', UNKNOWN: '待确认', PASS: '已通过', FAIL: '未通过', NOT_STARTED: '未开始' } as Record<string, string>)[String(value)] ?? '待确认'
const taskClassLabel = (value: string) => ({ REQUIRED: '必须完成', OPTIONAL: '可选安排', FLEXIBLE: '弹性安排', BLOCKED: '当前阻塞' } as Record<string, string>)[value] ?? '任务'

async function closeVolume() {
  await store.runCommand('volume-close', {
    type: 'volume_close',
    projectId: store.currentProjectId ?? undefined,
    payload: { volumeId: volumeId.value, reason: '用户在卷工作区确认关闭；退出条件已由 B09 projection 复核。' }
  })
  await store.loadVolumeLifecycle(volumeId.value, true)
}

onMounted(() => { void store.loadVolumeDetail(volumeId.value); void store.loadPlanningRoute(volumeId.value); void store.loadVolumeLifecycle(volumeId.value) })
</script>

<template>
  <div v-if="detail" class="page">
    <PageHeader :title="`第 ${detail.volume.number} 卷 · ${detail.volume.title}`" :subtitle="detail.volume.goal">
    </PageHeader>
    <section class="card pad section"><span class="badge">规划</span><h2 class="card-title" style="margin-top:8px">本卷目标</h2><p class="card-subtitle">{{ detail.volume.goal }}</p></section>
    <CommandFeedback :state="state" />

    <section v-if="lifecycle" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">卷生命周期</h2><p class="card-subtitle">状态来自当前作品记录，关闭与下一卷安排会在同一流程中核对。</p></div><span class="badge" :class="closeReady ? 'success' : 'warning'">{{ closeReady ? '可关闭' : '待完成' }}</span></div>
      <div class="metric-grid compact-grid">
        <MetricCard label="关闭准入" :value="closeReady ? '可关闭' : '条件未满足'" :note="`待补条件 ${String(planningState.exitConditionGap ?? 0)} 项`" />
        <MetricCard label="待完成重点" :value="String(planningState.requiredTasksRemaining ?? '—')" note="来自当前实际进展" />
        <MetricCard label="下一卷" :value="statusLabel(lifecycle.latestClose?.nextVolumeEligibility ?? '待关闭')" note="关闭记录中的下一步判断" />
        <MetricCard label="记录版本" :value="String(planningState.generatedRevision ?? lifecycle.projectRevision)" note="可重新生成的当前记录" />
      </div>
      <div v-if="remainingTasks.length" class="muted" style="margin-top:10px">阻塞项：{{ remainingTasks.join('、') }}</div>
      <div v-if="responsibilities.length" class="section-subgroup" style="margin-top:14px"><strong>未完成的跨卷事项</strong><div class="timeline"><div v-for="item in responsibilities" :key="String(item.threadRef)" class="timeline-item"><div class="timeline-marker">T</div><div><strong>相关剧情线</strong><div class="muted" style="font-size:12px">{{ statusLabel(item.status ?? 'OPEN') }} · 需要在后续卷继续核对</div></div></div></div></div>
      <div v-if="routeReplans.length" class="section-subgroup" style="margin-top:14px"><strong>规划调整记录</strong><div class="timeline"><div v-for="item in routeReplans" :key="String(item.replanId)" class="timeline-item"><div class="timeline-marker">R</div><div><strong>第 {{ String(item.newVersion) }} 次调整 · {{ statusLabel(item.validationStatus ?? 'UNKNOWN') }}</strong><div class="muted" style="font-size:12px">{{ String(item.reason ?? '未提供原因') }}</div></div></div></div></div>
      <div class="section-subgroup" style="margin-top:14px"><strong>下一卷交接</strong><div v-if="lifecycle.latestHandoff" class="muted">{{ statusLabel(lifecycle.latestHandoff.handoffStatus ?? 'UNKNOWN') }} · 下一卷安排已记录</div><div v-else class="muted">当前卷尚无交接记录；关闭并满足条件后才可创建。</div></div>
      <CommandFeedback :state="closeState" />
      <div class="actions" style="margin-top:14px"><button class="btn primary" :disabled="!closeReady || detail.volume.status === 'CLOSED'" @click="closeVolume">生成本卷退出记录并关闭本卷</button><span class="muted" style="font-size:12px">关闭与退出记录会在同一流程中提交；失败会完整回滚。</span></div>
    </section>

    <div v-if="locked" class="banner warning section"><div><div class="banner-title">当前卷仍处于准入锁定</div><div class="banner-copy">开篇评估通过前可以查看全书战略中的卷意图，但不能创建详细卷计划或启动后续正文生产。</div></div></div>

    <div v-if="planRoute" class="metric-grid section">
      <MetricCard label="剩余容量" :value="String(planRoute.planningPressure.remainingChapterCapacity ?? '—')" note="根据当前卷安排推算" />
      <MetricCard label="规划压力" :value="String(planRoute.planningPressure.priorityScore ?? '—')" note="由待完成事项和退出条件共同判断" />
      <MetricCard label="规划版本" :value="String(planRoute.routeVersion)" note="当前卷的安排版本" />
      <MetricCard label="任务数" :value="String(tasks.length)" note="状态来自实际进展" />
    </div>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">任务实际进展</h2><p class="card-subtitle">这里显示已经发生的进展，不用计划内容代替实际状态。</p></div><span class="badge">实际进展</span></div>
      <div v-for="taskClass in ['REQUIRED', 'OPTIONAL', 'FLEXIBLE', 'BLOCKED']" :key="taskClass" class="section-subgroup"><strong>{{ taskClassLabel(taskClass) }}</strong><div class="timeline"><div v-for="task in tasksByClass(taskClass === 'BLOCKED' ? 'REQUIRED' : taskClass).filter(item => taskClass !== 'BLOCKED' || realityStatus(item.taskId) === 'BLOCKED')" :key="String(task.taskId)" class="timeline-item"><div class="timeline-marker">R</div><div><strong>当前任务</strong><div class="muted" style="font-size:12px">{{ statusLabel(realityStatus(task.taskId)) }}</div></div></div></div></div>
    </section>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">前后连贯与后续安排</h2><p class="card-subtitle">这些内容用于规划参考，不会改变正式作品事实或已经发生的进展。</p></div><span class="badge">系统推算</span></div>
      <dl class="pitch-kv"><div><dt>未完成的剧情线</dt><dd>{{ planRoute.continuityThreads?.length ?? 0 }}</dd></div><div><dt>预留安排</dt><dd>{{ planRoute.futureReservations?.length ?? 0 }}</dd></div><div><dt>当前章节规划</dt><dd>{{ planRoute.currentChapterPlan ? '已生成' : '未生成' }}</dd></div><div><dt>后续预览</dt><dd>{{ planRoute.horizonPreview?.length ?? 0 }} 项暂定安排</dd></div></dl>
    </section>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">本卷安排</h2><p class="card-subtitle">规划与实际进展分开记录；不会用静态章表或百分比替代作品事实。</p></div><span class="badge">规划</span></div>
      <div class="timeline">
        <div v-for="task in tasks" :key="String(task.taskId)" class="timeline-item">
          <div class="timeline-marker">{{ String(task.class).slice(0, 1) }}</div>
          <div><strong>{{ String(task.objective) }}</strong><div class="muted" style="font-size:12px">{{ taskClassLabel(String(task.class)) }} · 完成条件已记录</div></div>
        </div>
      </div>
    </section>

    <section v-if="store.advancedMode" class="card pad section">
      <h2 class="card-title">高级详情</h2>
      <dl v-if="planRoute" class="pitch-kv"><div><dt>本卷目标记录</dt><dd>已记录 · v{{ planRoute.volumeIntentRef.version }}</dd></div><div><dt>本卷规划版本</dt><dd>第 {{ planRoute.volumePlanRef.version }} 版</dd></div><div><dt>作品记录版本</dt><dd>{{ planRoute.projectRevision }}</dd></div></dl>
    </section>
  </div>
</template>
