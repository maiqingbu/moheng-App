<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import ContinuityPanel from '../components/ContinuityPanel.vue'
import type { ContinuityThreadViewProjection, VolumeDetailProjection } from '../../application/contracts'
import { sanitizeProductText, statusLabel as productStatusLabel } from '../presentation/productLanguage'
import VolumePlanningProgress from '../components/VolumePlanningProgress.vue'
import { createVolumePlanningRuntimePoller } from '../runtime/volumePlanningRuntimeSync'

const store = useProjectStore()
const route = useRoute()
const router = useRouter()
const volumeId = computed(() => String(route.params.volumeId ?? ''))
const detail = computed(() => store.volumeDetail?.volume.id === volumeId.value ? store.volumeDetail : null)
const workspaceError = ref('')
const planRoute = computed(() => store.planningRoute?.volumeId === volumeId.value ? store.planningRoute : null)
const continuity = computed(() => store.continuityVolumeId === volumeId.value ? store.continuityProjection : null)
const continuityError = ref('')
const lifecycle = computed(() => store.volumeLifecycle?.volumeId === volumeId.value ? store.volumeLifecycle : null)
const planningState = computed(() => lifecycle.value?.planningState ?? {})
const tasks = computed(() => planRoute.value?.tasks ?? [])
const state = computed(() => store.commandState('volume-build'))
const closeState = computed(() => store.commandState('volume-close'))
const locked = computed(() => detail.value?.volume.status === 'LOCKED')
const chapterActionsAvailable = computed(() => !!detail.value && !locked.value && detail.value.volume.status !== 'CLOSED' && !workspaceError.value && planRoute.value?.truncated !== true)
const taskReality = computed(() => planRoute.value?.taskReality ?? [])
const realityStatus = (taskId: unknown) => { const value = taskReality.value.find(item => String(item.stateId) === String(taskId))?.value as Record<string, unknown> | undefined; return typeof value?.status === 'string' ? value.status : 'UNKNOWN' }
const tasksByClass = (taskClass: string) => tasks.value.filter(task => String(task.class) === taskClass)
const closeReady = computed(() => planningState.value.closeReadiness === 'READY')
const remainingTasks = computed(() => Array.isArray(planningState.value.remainingTaskIds) ? planningState.value.remainingTaskIds as string[] : [])
const responsibilities = computed(() => Array.isArray(planningState.value.unresolvedResponsibilities) ? planningState.value.unresolvedResponsibilities as Array<Record<string, unknown>> : [])
const routeReplans = computed(() => lifecycle.value?.routeReplans ?? [])
const runtimeOperation = computed(() => detail.value?.runtimeOperation ?? null)
const routeInvariants = computed(() => planRoute.value?.routeInvariants ?? [])
const exitConditions = computed(() => planRoute.value?.requiredExitConditions ?? [])
const capacityBudget = computed(() => planRoute.value?.capacityBudget ?? {})
const goalContractRefs = computed(() => planRoute.value?.goalContractRefs ?? [])
const lifecycleOwner = 'B09::VolumeLifecycleOwner'
const exitSnapshotKind = 'VolumeExitState'
const unresolvedLabel = 'OPEN / SUSPENDED'
const handoffFields = ['fromVolumeId', 'toVolumeId', 'handoffStatus']
const frozenProjectionMarkers = ['PLAN', 'REALITY', 'PROJECTION']
const chapterPrepareState = computed(() => store.commandState('chapter-prepare'))
const chapterPlanState = computed(() => store.commandState('chapter-plan-compile'))
const nextChapter = computed(() => detail.value?.chapters.items
  .filter(chapter => chapter.status !== 'official')
  .sort((left, right) => left.number - right.number)[0] ?? null)
const currentChapterPlan = computed(() => planRoute.value?.currentChapterPlan ?? null)
const chapterPlanningBusy = (status: string) => ['submitting', 'accepted', 'processing'].includes(status)
type ScopeBudgetGroup = { key: string; kind: string; ref: string; estimatedTokens: number; serializedBytes: number }
const chapterBudgetFailure = computed(() => {
  const chapterScope = nextChapter.value?.id ? 'CHAPTER:' + nextChapter.value.id : ''
  return [
    store.commandState('chapter-plan-compile').contextBudget,
    store.commandState('production-start').contextBudget,
  ].find(report => report && (!chapterScope || report.scope === chapterScope))
})
function groupBudgetSources(report: typeof chapterBudgetFailure.value): ScopeBudgetGroup[] {
  const groups = new Map<string, ScopeBudgetGroup>()
  for (const source of report?.sourceSizes ?? []) {
    if (!source.adjustable || !source.selectionKind || !source.selectionRef) continue
    const key = source.selectionKind + ':' + source.selectionRef
    const group = groups.get(key) ?? {
      key,
      kind: source.selectionKind,
      ref: source.selectionRef,
      estimatedTokens: 0,
      serializedBytes: 0,
    }
    group.estimatedTokens += source.estimatedTokens
    group.serializedBytes += source.serializedBytes
    groups.set(key, group)
  }
  return [...groups.values()].sort((left, right) => left.kind.localeCompare(right.kind) || left.ref.localeCompare(right.ref))
}
const scopeBudgetGroups = computed(() => groupBudgetSources(chapterBudgetFailure.value))
const selectedScopeKeys = ref<string[]>([])
watch(chapterBudgetFailure, () => {
  selectedScopeKeys.value = scopeBudgetGroups.value.map(group => group.key)
}, { immediate: true })
const workspaceRuntimePoller = createVolumePlanningRuntimePoller({
  query: async () => {
    const value = await store.loadVolumeDetail(volumeId.value, true)
    if (!value) throw new Error('当前卷状态暂不可用。')
    return value
  },
  onProjection: (_projection: VolumeDetailProjection) => {},
  onError: () => { workspaceError.value = '卷状态暂时无法更新，请重新读取后再操作。' },
})
let workspaceMounted = false
const statusLabel = (value: unknown) => productStatusLabel(value, '待确认')
const taskClassLabel = (value: string) => ({ REQUIRED: '必须完成', OPTIONAL: '可选安排', FLEXIBLE: '弹性安排', CLIMAX: '高潮任务', RESERVED: '预留任务', BLOCKED: '当前阻塞' } as Record<string, string>)[value] ?? '任务'
const taskDependencies = (task: Record<string, unknown>) => Array.isArray(task.dependsOnTaskIds) && task.dependsOnTaskIds.length ? task.dependsOnTaskIds.map(String).join('、') : '无'
const taskPredicate = (task: Record<string, unknown>) => {
  const predicate = task.completionPredicate
  if (!predicate || typeof predicate !== 'object') return '未记录'
  const value = predicate as Record<string, unknown>
  return `${String(value.kind ?? '未记录')} · ${String(value.value ?? '')}`
}
const taskCost = (task: Record<string, unknown>) => {
  const cost = task.estimatedChapterCost
  if (!cost || typeof cost !== 'object') return '—'
  return String((cost as Record<string, unknown>).preferred ?? '—')
}
const realityEvidence = (task: Record<string, unknown>) => {
  const value = task.value as Record<string, unknown> | undefined
  const refs = value?.evidenceRefs ?? task.evidenceRefs
  return Array.isArray(refs) && refs.length ? refs.map(ref => String(ref)).join('、') : '尚无证据'
}

async function closeVolume() {
  if (!closeReady.value || !chapterActionsAvailable.value || chapterPlanningBusy(closeState.value.status)) return
  await store.runCommand('volume-close', {
    type: 'volume_close',
    projectId: store.currentProjectId ?? undefined,
    payload: { volumeId: volumeId.value, reason: '用户在卷工作区确认关闭；退出条件已由 B09 projection 复核。' }
  })
  await store.loadVolumeLifecycle(volumeId.value, true)
}

async function prepareNextChapter() {
  if (!chapterActionsAvailable.value || chapterPlanningBusy(chapterPrepareState.value.status)) return
  await store.runCommand('chapter-prepare', {
    type: 'planning_prepare_next_chapter',
    projectId: store.currentProjectId ?? undefined,
    payload: { volumeId: volumeId.value },
  })
  const refreshed = await store.loadVolumeDetail(volumeId.value, true)
  const chapter = refreshed?.chapters.items
    .filter(item => item.status !== 'official')
    .sort((left, right) => left.number - right.number)[0]
  if (chapter) await store.loadPlanningRoute(volumeId.value, true)
}

async function compileNextChapterPlan(adjustScope = false) {
  const chapter = nextChapter.value
  if (!chapterActionsAvailable.value || !chapter || chapterPlanningBusy(chapterPlanState.value.status)) return
  const scopeSelection = adjustScope ? {
    taskRefs: scopeBudgetGroups.value.filter(group => group.kind === 'TASK' && selectedScopeKeys.value.includes(group.key)).map(group => group.ref),
    responsibilityRefs: scopeBudgetGroups.value.filter(group => group.kind === 'RESPONSIBILITY' && selectedScopeKeys.value.includes(group.key)).map(group => group.ref),
    reservationRefs: scopeBudgetGroups.value.filter(group => group.kind === 'RESERVATION' && selectedScopeKeys.value.includes(group.key)).map(group => group.ref),
    entityRefs: scopeBudgetGroups.value.filter(group => group.kind === 'ENTITY' && selectedScopeKeys.value.includes(group.key)).map(group => group.ref),
  } : undefined
  await store.runCommand('chapter-plan-compile', {
    type: 'planning_compile_chapter_plan',
    projectId: store.currentProjectId ?? undefined,
    payload: {
      chapterId: chapter.id,
      chapterGoalHint: '推进当前卷任务，并让真实选择产生可核对后果。',
      ...(scopeSelection ? { scopeSelection } : {}),
    },
  })
  await store.loadPlanningRoute(volumeId.value, true)
}

async function loadWorkspace(force = false) {
  workspaceError.value = ''
  continuityError.value = ''
  workspaceRuntimePoller.stop()
  const requestedVolumeId = volumeId.value
  void store.loadContinuity(requestedVolumeId, true).then(() => {
    if (workspaceMounted && requestedVolumeId === volumeId.value) continuityError.value = ''
  }).catch(() => {
    if (workspaceMounted && requestedVolumeId === volumeId.value) continuityError.value = '暂时无法读取已记录的剧情线，请重新读取。'
  })
  try {
    const [initial] = await Promise.all([
      force ? store.loadVolumeDetail(requestedVolumeId, true) : store.loadVolumeDetail(requestedVolumeId),
      store.loadVolumeLifecycle(requestedVolumeId, true),
    ])
    if (!workspaceMounted || requestedVolumeId !== volumeId.value) return
    await store.loadPlanningRoute(requestedVolumeId, true)
    if (!workspaceMounted || requestedVolumeId !== volumeId.value) return
    if (initial?.runtimeOperation?.active === true) workspaceRuntimePoller.start()
  } catch {
    if (workspaceMounted && requestedVolumeId === volumeId.value) workspaceError.value = '卷工作区查询未完成，当前结果可能不完整，请重新读取。'
  }
}

function requestContinuityChange(thread: ContinuityThreadViewProjection) {
  const projectId = store.currentProjectId
  if (!projectId) return
  void router.push({
    name: 'history',
    params: { id: projectId },
    query: {
      createChangeRequest: '1',
      continuityThreadRef: thread.threadId,
      continuityDescription: thread.description,
    },
  })
}

async function performAction(action: () => Promise<void>) {
  try { await action() } catch { workspaceError.value = '操作结果尚待核对，请查看回执并重新读取当前卷。' }
}

onMounted(() => { workspaceMounted = true; void loadWorkspace() })
watch(volumeId, () => { void loadWorkspace(true) })
onUnmounted(() => {
  workspaceMounted = false
  workspaceRuntimePoller.stop()
})
</script>

<template>
  <div v-if="detail" class="page">
    <PageHeader :title="`第 ${detail.volume.number} 卷 · ${detail.volume.title}`" :subtitle="detail.volume.goal"><button class="btn" @click="loadWorkspace(true)">重新读取</button>
    </PageHeader>
    <section class="card pad section"><span class="badge">PLAN</span><h2 class="card-title" style="margin-top:8px">本卷目标</h2><p class="card-subtitle">{{ detail.volume.goal }}</p></section>
    <CommandFeedback :state="state" @confirm="store.confirmCommandResult('volume-build')" />
    <div v-if="workspaceError || planRoute?.truncated" class="banner warning" role="alert">{{ workspaceError || '当前卷查询结果已截断，不能据此确认必需信息完整。' }}</div>

    <p v-if="continuityError" class="banner warning section" role="alert">{{ continuityError }}</p>
    <ContinuityPanel
      v-else-if="continuity"
      :threads="continuity.threads"
      :truncated="continuity.truncated"
      :advanced="store.advancedMode"
      @request-change="requestContinuityChange"
    />

    <VolumePlanningProgress :operation="runtimeOperation" />

    <section v-if="chapterActionsAvailable" class="card pad section" data-next-chapter-actions>
      <div class="card-head"><div><h2 class="card-title">下一章正式生产</h2><p class="card-subtitle">先准备当前章节并生成计划，再到“创作”页检查和开始生产。</p></div><span class="badge">当前章计划</span></div>
      <CommandFeedback :state="chapterPrepareState" @confirm="store.confirmCommandResult('chapter-prepare')" />
      <CommandFeedback :state="chapterPlanState" @confirm="store.confirmCommandResult('chapter-plan-compile')" />
      <div v-if="!nextChapter" class="actions" style="margin-top:14px"><button class="btn primary" data-action="prepare" :disabled="chapterPlanningBusy(chapterPrepareState.status)" @click="performAction(prepareNextChapter)">准备下一章</button><span class="muted" style="font-size:12px">以正式提交的章节为起点，准备当前章。</span></div>
      <div v-else-if="!currentChapterPlan" class="actions" style="margin-top:14px"><div><strong>第 {{ nextChapter.number }} 章 · {{ nextChapter.title }}</strong><div class="muted" style="font-size:12px;margin-top:4px">章节已准备，尚未生成当前章计划。</div></div><button class="btn primary" data-action="compile" :disabled="chapterPlanningBusy(chapterPlanState.status)" @click="performAction(compileNextChapterPlan)">生成当前章节计划</button></div>
      <div v-else class="actions" style="margin-top:14px"><div><strong>第 {{ nextChapter.number }} 章计划已绑定</strong><div class="muted" style="font-size:12px;margin-top:4px">第 {{ currentChapterPlan.version }} 版 · 执行有效性以生产前检查为准</div></div><span class="muted" style="font-size:12px">请在“创作”页从正式 UI 启动本章生产。</span></div>
      <section v-if="chapterBudgetFailure" class="section-subgroup" data-hard-budget-scope-adjustment aria-labelledby="hard-budget-scope-title">
        <h3 id="hard-budget-scope-title">缩小本章 Plan 范围后重新编译</h3>
        <p class="muted">硬性目标合同保持不变。取消本章不需要的任务、责任、预留或实体后，由你手动重新编译；系统不会自动删除来源、降低 HARD 或提高预算。</p>
        <fieldset v-if="scopeBudgetGroups.length" class="context-budget-scope-list">
          <legend>可调整的本章来源</legend>
          <label v-for="group in scopeBudgetGroups" :key="group.key" class="context-budget-scope-option">
            <input v-model="selectedScopeKeys" type="checkbox" :value="group.key">
            <span>{{ group.kind }} · {{ group.ref }} · {{ group.estimatedTokens }} token / {{ group.serializedBytes }} 字节</span>
          </label>
        </fieldset>
        <p v-else class="muted">超限来源都属于必须保留的合同或约束。请在正式规划记录中调整合同/计划来源后重新读取，再编译。</p>
        <button v-if="scopeBudgetGroups.length" class="btn primary" data-action="recompile-narrowed-scope" :disabled="chapterPlanningBusy(chapterPlanState.status)" @click="performAction(() => compileNextChapterPlan(true))">按所选范围重新编译</button>
      </section>
    </section>

    <section v-if="lifecycle" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">卷生命周期</h2><p class="card-subtitle">状态来自当前作品记录，关闭与下一卷安排会在同一流程中核对。</p></div><span class="badge" :class="closeReady ? 'success' : 'warning'">{{ closeReady ? '可关闭' : '待完成' }}</span></div>
      <div class="metric-grid compact-grid">
        <MetricCard label="关闭准入" :value="closeReady ? '可关闭' : '条件未满足'" :note="`待补条件 ${String(planningState.exitConditionGap ?? 0)} 项`" />
        <MetricCard label="待完成重点" :value="String(planningState.requiredTasksRemaining ?? '—')" note="来自当前实际进展" />
        <MetricCard label="下一卷" :value="statusLabel(lifecycle.latestClose?.nextVolumeEligibility ?? '待关闭')" note="关闭记录中的下一步判断" />
        <MetricCard label="记录版本" :value="String(planningState.generatedRevision ?? lifecycle.projectRevision)" note="可重新生成的当前记录" />
      </div>
      <div v-if="remainingTasks.length" class="muted" style="margin-top:10px">还有 {{ remainingTasks.length }} 项任务待完成。</div>
      <div v-if="responsibilities.length" class="section-subgroup" style="margin-top:14px"><strong>未完成的跨卷事项</strong><div class="timeline"><div v-for="item in responsibilities" :key="String(item.threadRef)" class="timeline-item"><div class="timeline-marker">T</div><div><strong>相关剧情线</strong><div class="muted" style="font-size:12px">{{ statusLabel(item.status ?? 'OPEN') }} · 需要在后续卷继续核对</div></div></div></div></div>
      <div v-if="routeReplans.length" class="section-subgroup" style="margin-top:14px"><strong>规划调整记录</strong><div class="timeline"><div v-for="item in routeReplans" :key="String(item.replanId)" class="timeline-item"><div class="timeline-marker">R</div><div><strong>第 {{ String(item.newVersion) }} 次调整 · {{ statusLabel(item.validationStatus ?? 'UNKNOWN') }}</strong><div class="muted" style="font-size:12px">{{ sanitizeProductText(item.reason, '调整原因已记录。') }}</div></div></div></div></div>
      <div class="section-subgroup" style="margin-top:14px"><strong>下一卷交接</strong><div v-if="lifecycle.latestHandoff" class="muted">{{ statusLabel(lifecycle.latestHandoff.handoffStatus ?? 'UNKNOWN') }} · 下一卷安排已记录</div><div v-else class="muted">当前卷尚无交接记录；关闭并满足条件后才可创建。</div></div>
      <CommandFeedback :state="closeState" @confirm="store.confirmCommandResult('volume-close')" />
      <div class="actions" style="margin-top:14px"><button class="btn primary" :disabled="!closeReady || !chapterActionsAvailable || chapterPlanningBusy(closeState.status)" @click="performAction(closeVolume)">生成本卷退出记录并关闭本卷</button><span class="muted" style="font-size:12px">关闭与退出记录会在同一流程中提交；失败会完整回滚。</span></div>
    </section>

    <div v-if="locked" class="banner warning section"><div><div class="banner-title">当前卷仍处于准入锁定</div><div class="banner-copy">开篇评估通过前可以查看全书战略中的卷意图，但不能创建详细卷计划或启动后续正文生产。</div></div></div>

    <div v-if="planRoute" class="metric-grid section">
      <MetricCard label="剩余容量" :value="String(planRoute.planningPressure.remainingChapterCapacity ?? '—')" note="根据当前卷安排推算" />
      <MetricCard label="规划压力" :value="String(planRoute.planningPressure.priorityScore ?? '—')" note="由待完成事项和退出条件共同判断" />
      <MetricCard label="规划版本" :value="String(planRoute.routeVersion)" note="当前卷的安排版本" />
      <MetricCard label="任务数" :value="String(tasks.length)" note="状态来自实际进展" />
    </div>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">任务实际进展</h2><p class="card-subtitle">这里显示已经发生的进展，不用计划内容代替实际状态。</p></div><span class="badge">REALITY · 实际进展</span></div>
      <div v-for="taskClass in ['REQUIRED', 'OPTIONAL', 'FLEXIBLE', 'CLIMAX', 'RESERVED', 'BLOCKED']" :key="taskClass" class="section-subgroup"><strong>{{ taskClassLabel(taskClass) }}</strong><div class="timeline"><div v-for="task in taskClass === 'BLOCKED' ? tasks.filter(item => realityStatus(item.taskId) === 'BLOCKED') : tasksByClass(taskClass)" :key="String(task.taskId)" class="timeline-item"><div class="timeline-marker">R</div><div><strong>{{ String(task.objective ?? '当前任务') }}</strong><div class="muted" style="font-size:12px">{{ realityStatus(task.taskId) === 'UNKNOWN' ? '实际状态尚未提供' : statusLabel(realityStatus(task.taskId)) }} · 证据：{{ realityEvidence(taskReality.find(item => String(item.stateId) === String(task.taskId)) ?? {}) }}</div></div></div></div></div>
    </section>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">前后连贯与后续安排</h2><p class="card-subtitle">这些内容用于规划参考，不会改变正式作品事实或已经发生的进展。</p></div><span class="badge">PROJECTION · 系统推算</span></div>
      <dl class="pitch-kv"><div><dt>预留安排</dt><dd>{{ planRoute.futureReservations?.length ?? '未提供' }}</dd></div><div><dt>当前章节规划</dt><dd>{{ planRoute.currentChapterPlan ? '已生成' : '未生成' }}</dd></div><div><dt>后续预览</dt><dd>{{ planRoute.horizonPreview == null ? '尚未提供预览' : planRoute.horizonPreview.length ? `${planRoute.horizonPreview.length} 项暂定安排` : '暂无后续预览' }}</dd></div></dl>
    </section>

    <section v-if="planRoute" class="card pad section" data-rolling-horizon>
      <div class="card-head"><div><h2 class="card-title">后续章节预览</h2><p class="card-subtitle">仅为暂定安排，不代表未来事实或可执行计划；卷末或书末允许为空。</p></div><span class="badge">TENTATIVE · 暂定</span></div>
      <p v-if="!planRoute.horizonPreview" class="muted">尚未提供预览。</p>
      <p v-else-if="!planRoute.horizonPreview.length" class="muted">暂无后续预览。不需要补齐占位章节。</p>
      <div v-for="(slot, index) in planRoute.horizonPreview ?? []" :key="index" class="task-row"><strong>暂定 {{ index + 1 }}</strong><span>{{ sanitizeProductText(slot.chapterGoal ?? slot.possibleConflict ?? slot.preparationDirection, '当前预览未提供可展示的安排说明。') }}</span></div>
    </section>

    <section v-if="planRoute" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">本卷任务路线</h2><p class="card-subtitle">规划与实际进展分开记录；不会用静态章表或百分比替代作品事实。</p></div><span class="badge">PLAN · 任务路线</span></div>
      <div class="timeline">
        <div v-for="task in tasks" :key="String(task.taskId)" class="timeline-item">
          <div class="timeline-marker">{{ String(task.class).slice(0, 1) }}</div>
          <div style="width:100%"><strong>{{ String(task.objective) }}</strong><div class="muted" style="font-size:12px">{{ taskClassLabel(String(task.class)) }} · 完成条件已记录</div><dl class="pitch-kv" style="margin-top:8px"><div><dt>依赖任务</dt><dd>{{ taskDependencies(task) }}</dd></div><div><dt>完成条件</dt><dd>{{ taskPredicate(task) }}</dd></div><div><dt>预计章节成本</dt><dd>{{ taskCost(task) }}</dd></div></dl></div>
        </div>
      </div>
      <div class="section-subgroup" style="margin-top:14px"><strong>路线约束</strong><ul class="muted"><li v-for="invariant in routeInvariants" :key="invariant">{{ invariant }}</li><li v-if="!routeInvariants.length">暂无路线约束</li></ul></div>
      <div class="section-subgroup" style="margin-top:14px"><strong>退出条件</strong><ul class="muted"><li v-for="condition in exitConditions" :key="String(condition.kind) + String(condition.value)">{{ String(condition.kind) }} · {{ String(condition.value ?? '') }}</li><li v-if="!exitConditions.length">暂无退出条件</li></ul></div>
      <div class="section-subgroup" style="margin-top:14px"><strong>容量预算</strong><div class="muted">最少 {{ String(capacityBudget.minChapters ?? '—') }} 章 · 预计 {{ String(capacityBudget.preferredChapters ?? '—') }} 章 · 最多 {{ String(capacityBudget.maxChapters ?? '—') }} 章</div></div>
      <div v-if="goalContractRefs.length" class="section-subgroup" style="margin-top:14px"><strong>Goal Contract 权威引用</strong><div class="muted">{{ goalContractRefs.map(reference => reference.artifactId).join('、') }}</div></div>
    </section>

    <section v-if="store.advancedMode" class="card pad section">
      <h2 class="card-title">高级详情</h2>
      <dl v-if="planRoute" class="pitch-kv"><div><dt>本卷目标记录</dt><dd>已记录 · v{{ planRoute.volumeIntentRef.version }}</dd></div><div><dt>本卷规划版本</dt><dd>第 {{ planRoute.volumePlanRef.version }} 版</dd></div><div><dt>作品记录版本</dt><dd>{{ planRoute.projectRevision }}</dd></div><div><dt>编译版本 / 相关摘要</dt><dd>{{ planRoute.compiledFromProjectRevision ?? '未提供' }} / {{ planRoute.semanticDigest ?? '未提供' }}</dd></div><div><dt>来源证据</dt><dd>{{ planRoute.sourceEvidenceRefs?.join('、') || '未提供' }}</dd></div></dl>
    </section>
  </div>
  <div v-else class="page"><PageHeader title="卷工作区" subtitle="读取当前卷的正式记录。" /><p role="status">{{ workspaceError || '正在读取当前卷…' }}</p><button class="btn" @click="loadWorkspace(true)">重新读取</button></div>
</template>
