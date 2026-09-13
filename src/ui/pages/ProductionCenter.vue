<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import RuntimeBanner from '../components/RuntimeBanner.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import type { RunState } from '../../application/contracts'
import { isProductUxEvidenceDemoEnabled, productErrorMessage, receiptSummary, sanitizeProductText, scopeLabel as productScopeLabel, statusLabel as productStatusLabel } from '../presentation/productLanguage'
import { createProductionRuntimePoller, isProductionJobTerminal } from '../runtime/productionRuntimeSync'

const store = useProjectStore()
const route = useRoute()
const project = computed(() => store.project)
const production = computed(() => store.production)
const lastCommand = ref('none')
const commandState = computed(() => store.commandState(lastCommand.value))
const productionStartState = computed(() => store.commandState('production-start'))
const evaluationCommandState = computed(() => store.commandState('opening-evaluate'))
const bookPlanCommandState = computed(() => store.commandState('book-plan-revise'))
const qualityDimensions = computed(() => Object.entries(production.value?.quality?.dimensions ?? {}))
const qualityFindings = computed(() => production.value?.quality?.findings ?? [])
const openingProgress = computed(() => production.value?.openingProgress)
const signing = computed(() => production.value?.signing)
const bookPlan = computed(() => production.value?.bookPlan)
const stageLabel = (value: unknown) => ({ CONTEXT: '准备创作上下文', PLAN: '编译章节计划', RENDER: '生成正文', CANDIDATE: '整理正文候选', AUDIT: '审计与事实提取', GATE: '一致性与准入校验', COMMIT: '正式提交', RECONCILE: '状态回写与后续判断', COMPLETED: '本章完成', OPENING: '开篇准备', OPENING_PRODUCTION: '开篇创作', VOLUME_PRODUCTION: '长篇创作', IDLE: '等待开始' } as Record<string, string>)[String(value)] ?? '进行中'
const gateLabel = (value: unknown) => ({ PASS: '已通过', BLOCKED: '暂时阻断', PENDING: '等待检查', FAIL: '未通过' } as Record<string, string>)[String(value)] ?? '等待检查'
const reconciliationLabel = (value: unknown) => ({ NO_ACTION: '无需处理', CONTINUE: '继续', PAUSE: '暂时暂停', ESCALATE: '需要确认' } as Record<string, string>)[String(value)] ?? '等待判断'
const capabilityLabel = (value: unknown) => ({ RENDER_CHAPTER: '撰写章节', AUDIT_EXTRACT: '检查章节', QUALITY_EVALUATION: '评估叙事质量' } as Record<string, string>)[String(value)] ?? '创作步骤'
const timelineLabel = (value: unknown) => ({ CONTEXT: '准备创作上下文', PLAN: '编译章节计划', RENDER: '生成正文', CANDIDATE: '整理正文候选', AUDIT: '审计与事实提取', GATE: '一致性与准入校验', COMMIT: '正式提交', RECONCILE: '状态回写与后续判断', COMPLETED: '本章完成' } as Record<string, string>)[String(value)] ?? '创作步骤'
const timelineDescription = (item: { title?: unknown; description?: unknown; status?: unknown }) => {
  const title = String(item.title ?? '')
  if (title === 'CONTEXT') return '已整理当前作品记录和本章所需上下文。'
  if (title === 'PLAN') return '已建立本章目标、冲突和结构安排。'
  if (title === 'RENDER') return '已形成正文候选，等待检查和确认。'
  if (title === 'CANDIDATE') return '正在检查正文候选与当前作品约束。'
  if (title === 'AUDIT') return '正在核对连续性、质量和正式准入条件。'
  if (title === 'GATE') return '正式确认前的一致性与准入校验正在进行。'
  if (title === 'COMMIT') return '正在把正文、事实和回执正式写入作品记录。'
  if (title === 'RECONCILE') return '正在回写正式结果并准备下一章。'
  if (item.status === 'blocked' && production.value?.jobStatus === 'CANCELLED') return '当前任务已取消，未伪装为完成。'
  return sanitizeProductText(item.description, '当前步骤已完成。')
}
const statusLabel = (value: unknown) => productStatusLabel(value, '待检查')
const detailLabel = (value: unknown, fallback = '当前状态已查询。') => sanitizeProductText(value, fallback)
const scopeLabel = (value: unknown) => /^[A-Z_]+$/.test(String(value)) ? productScopeLabel(value) : sanitizeProductText(value, '当前作品')
const dimensionLabel = (value: unknown) => ({ agency: '主角主动性', desire: '主角欲望', hook: '开篇吸引力', pacing: '节奏', voice: '叙事声音', dialogue: '对话', subtext: '潜台词', repetition: '重复风险', exposition: '信息呈现', emotionalPayoff: '情绪回报' } as Record<string, string>)[String(value)] ?? '叙事维度'
const historicalB10Terms = ['ReaderDesire', 'SOFT 质量评估', 'HARD Gate']
const productUxEvidenceMode = computed(() => isProductUxEvidenceDemoEnabled() ? String(route.query.productUxEvidence ?? '') : '')
const productUxReceipt = computed(() => receiptSummary({ finalStatus: 'committed', result: { status: 'VERIFIED' } }))
const productUxStateTitle = computed(() => productUxEvidenceMode.value === 'm14-stale' ? '需要重新确认'
  : productUxEvidenceMode.value === 'm14-blocked' ? '暂时无法继续'
    : '操作未完成')
const productUxStateMessage = computed(() => {
  if (productUxEvidenceMode.value === 'm14-stale') return productErrorMessage('PLAN_STALE')
  if (productUxEvidenceMode.value === 'm14-blocked') return productStatusLabel('BLOCKED', '暂时无法继续')
  if (productUxEvidenceMode.value === 'm14-error') return productErrorMessage('当前 Rust Runtime 处理异常，请刷新。', '操作未完成，请刷新当前作品后重试。')
  return ''
})
const productionStatus = computed(() => production.value?.jobStatus ?? null)
const productionIsActive = computed(() => production.value?.isActive === true)
const canRetryProduction = computed(() => production.value?.canRetry === true)
const productionStartInFlight = computed(() => ['submitting', 'accepted', 'processing'].includes(productionStartState.value.status))
const evaluationInFlight = computed(() => ['submitting', 'accepted', 'processing'].includes(evaluationCommandState.value.status))
const bookPlanInFlight = computed(() => ['submitting', 'accepted', 'processing'].includes(bookPlanCommandState.value.status))
const canEvaluateOpening = computed(() => openingProgress.value?.completed === openingProgress.value?.total
  && openingProgress.value?.total === 3
  && signing.value?.status !== 'PASS')
const canFinalizeBookPlan = computed(() => signing.value?.status === 'PASS'
  && bookPlan.value?.status !== 'APPROVED')
const canStartProduction = computed(() => Boolean(production.value?.nextChapter) && (productionIsActive.value || !production.value?.jobStatus || ['COMPLETED', 'CANCELLED'].includes(production.value.jobStatus) || canRetryProduction.value))
const authoritativeRunState = computed<RunState>(() => {
  switch (production.value?.jobStatus) {
    case 'FAILED': return 'FAILED'
    case 'WAITING_DECISION': return 'WAITING_DECISION'
    case 'PAUSED': return 'PAUSED'
    case 'RUNNING':
    case 'WAITING_MODEL': return 'ACTIVE'
    case 'BLOCKED': return 'BLOCKED'
    case 'COMPLETED': return 'COMPLETED'
    case 'CANCELLED': return 'IDLE'
    default: return project.value?.runState ?? 'IDLE'
  }
})

const runtimePoller = createProductionRuntimePoller({
  query: async () => {
    const value = await store.loadProduction(true)
    if (!value) throw new Error('生产状态暂不可用。')
    return value
  },
  onProjection: projection => { store.production = projection },
  isActiveProjection: projection => projection.runtimeOperation?.active === true || projection.isActive === true,
  isTerminalProjection: projection => {
    if (projection.runtimeOperation) return ['COMPLETED', 'FAILED', 'BLOCKED', 'CANCELLED'].includes(projection.runtimeOperation.status)
    return isProductionJobTerminal(projection.jobStatus)
  },
})

onMounted(() => {
  void store.loadProduction(true)
    .then(value => {
      if (value?.runtimeOperation?.active || (value?.jobStatus && !isProductionJobTerminal(value.jobStatus))) runtimePoller.start()
    })
    .catch(() => undefined)
})

onUnmounted(() => runtimePoller.stop())

function focusProductionProgress() {
  document.querySelector<HTMLElement>('[data-production-progress]')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

function focusOpeningEvaluationProgress() {
  document.querySelector<HTMLElement>('[data-opening-evaluation-progress]')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

async function startProduction() {
  const chapter = production.value?.nextChapter
  if (!chapter) throw new Error('B08_CHAPTER_REQUIRED')
  if (productionStartInFlight.value || productionIsActive.value) {
    focusProductionProgress()
    return
  }
  lastCommand.value = 'production-start'
  runtimePoller.start({ waitForActive: true })
  try {
    await store.runCommand(lastCommand.value, {
      type: 'production_start',
      payload: {
        chapterId: chapter.id,
        volumeId: chapter.volumeId,
        executionMode: chapter.executionMode,
        entityIds: [],
        references: [],
        skillsEnabled: true,
        ...(canRetryProduction.value && production.value?.tasks[0]?.id
          ? { retryOfJobId: production.value.tasks[0].id }
          : {}),
      },
    })
  } catch {
    // The store has already placed the typed failure in CommandFeedback.
  } finally {
    if (!productionIsActive.value || isProductionJobTerminal(production.value?.jobStatus)) runtimePoller.stop()
  }
}

function evaluationStatusLabel() {
  if (evaluationInFlight.value || signing.value?.evaluation?.status === 'IN_REVIEW') return '正在评估'
  if (signing.value?.status === 'PASS' || signing.value?.status === 'REVISE' || signing.value?.status === 'REJECTED') return '评估完成'
  return '等待三章完成'
}

function signingVerdictLabel() {
  if (signing.value?.status === 'PASS') return 'PASS'
  if (signing.value?.status === 'REVISE' || signing.value?.status === 'REJECTED') return '需要修订'
  return '尚未评估'
}

function evaluationStepClass(status: unknown) {
  return status === 'active' ? 'active' : status === 'done' ? 'done' : status === 'failed' ? 'failed' : 'pending'
}

function evaluationActionLabel() {
  return evaluationInFlight.value || signing.value?.evaluation?.status === 'IN_REVIEW'
    ? '查看开篇评估进度'
    : '开始开篇评估'
}

function bookPlanActionLabel() {
  return bookPlanInFlight.value || production.value?.runtimeOperation?.kind === 'B02::BOOK_PLAN'
    ? '查看正式总纲进度'
    : '生成正式总纲'
}

async function evaluateOpening() {
  if (!canEvaluateOpening.value) return
  if (evaluationInFlight.value || signing.value?.evaluation?.status === 'IN_REVIEW') {
    focusOpeningEvaluationProgress()
    return
  }
  lastCommand.value = 'opening-evaluate'
  runtimePoller.start({ waitForActive: true })
  try {
    await store.runCommand(lastCommand.value, { type: 'opening_evaluate', payload: {} })
  } catch {
    // Store-owned command feedback keeps the typed failure visible.
  } finally {
    if (!store.production?.runtimeOperation?.active) runtimePoller.stop()
  }
}

async function finalizeBookPlan() {
  if (!canFinalizeBookPlan.value) return
  if (bookPlanInFlight.value || production.value?.runtimeOperation?.kind === 'B02::BOOK_PLAN' && production.value.runtimeOperation.active) {
    focusOpeningEvaluationProgress()
    return
  }
  lastCommand.value = 'book-plan-revise'
  runtimePoller.start({ waitForActive: true })
  try {
    await store.runCommand(lastCommand.value, { type: 'book_plan_revise', payload: {} })
  } catch {
    // Store-owned command feedback keeps the typed failure visible.
  } finally {
    if (!store.production?.runtimeOperation?.active) runtimePoller.stop()
  }
}

async function controlProduction(type: 'production_pause' | 'production_resume' | 'production_cancel') {
  lastCommand.value = type
  try {
    await store.runCommand(lastCommand.value, {
      type,
      payload: production.value?.tasks[0]?.id ? { jobId: production.value.tasks[0].id } : {},
    })
  } catch {
    // The store has already placed the typed failure in CommandFeedback.
  }
}

async function authorizeRepair() {
  const repair = production.value?.repair
  if (!repair || repair.status !== 'PROPOSED') return
  const authorizationId = `repair-auth:${repair.repairPlanId}:${Date.now()}`
  lastCommand.value = 'repair-authorize'
  await store.runCommand(lastCommand.value, {
    type: 'repair_authorize',
    payload: {
      authorization: {
        authorizationId,
        projectId: project.value?.id,
        repairPlanId: repair.repairPlanId,
        basedOnProjectRevision: repair.basedOnProjectRevision,
        operationDigest: repair.operationDigest,
        approvedByUser: true,
        issuedAt: new Date().toISOString(),
      },
    },
  })
}

async function executeRepair() {
  const repair = production.value?.repair
  if (!repair?.authorizationId || repair.status !== 'AUTHORIZED') return
  lastCommand.value = 'repair-execute'
  await store.runCommand(lastCommand.value, {
    type: 'repair_execute',
    payload: { repairPlanId: repair.repairPlanId, authorizationId: repair.authorizationId },
  })
}
</script>

<template>
  <div v-if="project && production" class="page">
    <PageHeader title="创作" subtitle="从章节准备到正文候选、检查与正式确认，都从当前作品记录重新查询。">
      <button v-if="canStartProduction" class="btn primary" @click="startProduction">{{ productionStartInFlight || productionIsActive ? '查看第' : (canRetryProduction ? '重新生成' : '开始') }}{{ productionStartInFlight || productionIsActive ? ` ${production.nextChapter?.number} 章进度` : `第 ${production.nextChapter?.number} 章` }}</button>
      <button v-if="productionIsActive" class="btn" @click="controlProduction('production_pause')">暂停</button>
      <button v-if="productionStatus === 'PAUSED'" class="btn primary" @click="controlProduction('production_resume')">恢复</button>
    </PageHeader>
    <RuntimeBanner :state="authoritativeRunState" :reason="project.blockReason" />
    <CommandFeedback :state="commandState" />

    <section class="card pad section opening-evaluation-panel" data-opening-evaluation>
      <div class="card-head">
        <div>
          <span class="eyebrow">开篇评估 · 模拟签约</span>
          <h2 class="card-title" style="margin-top:6px">开篇评估</h2>
          <p class="card-subtitle">只读取三章已正式提交的正文、事实与证据；评估结果和签约回执来自后端正式记录。</p>
        </div>
        <span class="badge" :class="signing?.status === 'PASS' ? 'success' : signing?.status === 'REVISE' || signing?.status === 'REJECTED' ? 'warning' : 'info'">{{ evaluationStatusLabel() }}</span>
      </div>
      <CommandFeedback :state="evaluationCommandState" />
      <CommandFeedback :state="bookPlanCommandState" />
      <div class="evaluation-steps" v-if="signing?.evaluation?.steps?.length" data-opening-evaluation-progress>
        <div v-for="step in signing.evaluation.steps" :key="step.id" class="evaluation-step" :class="evaluationStepClass(step.status)">
          <span class="evaluation-step-marker">{{ step.status === 'done' ? '✓' : step.status === 'active' ? '…' : '·' }}</span>
          <span>{{ step.label }}</span>
        </div>
      </div>
      <div class="task-row" aria-live="polite"><strong>评估结果</strong><span>{{ signingVerdictLabel() }} · {{ signing?.verdict || '等待三章正式提交后开始评估。' }}</span></div>
      <div class="task-row"><strong>评估依据摘要</strong><span>三章 {{ signing?.summary?.chapters ?? 0 }} · 事实 {{ signing?.summary?.canonRefs ?? 0 }} 条 · 证据 {{ signing?.summary?.evidenceRefs ?? 0 }} 条</span></div>
      <div class="task-row"><strong>正式签约回执</strong><span>{{ signing?.receipt?.id ? `${signing.receipt.id} · 已提交` : '尚未生成' }}</span></div>
      <div class="task-row"><strong>正式总纲</strong><span>{{ bookPlan?.status === 'APPROVED' ? '已完成' : '等待评估通过后生成' }}<span v-if="bookPlan?.seedPreserved"> · 开篇草稿历史已保留</span></span></div>
      <div v-if="bookPlan?.steps?.length" class="evaluation-steps" aria-label="正式总纲运行步骤">
        <div v-for="step in bookPlan.steps" :key="step.id" class="evaluation-step" :class="evaluationStepClass(step.status)">
          <span class="evaluation-step-marker">{{ step.status === 'done' ? '✓' : step.status === 'active' ? '…' : '·' }}</span>
          <span>{{ step.label }}</span>
        </div>
      </div>
      <div class="actions" style="margin-top:14px">
        <button v-if="canEvaluateOpening" class="btn primary" @click="evaluateOpening">{{ evaluationActionLabel() }}</button>
        <button v-if="canFinalizeBookPlan" class="btn primary" @click="finalizeBookPlan">{{ bookPlanActionLabel() }}</button>
      </div>
    </section>

    <section v-if="productUxEvidenceMode === 'm13'" class="card pad section" data-product-ux-state="receipt">
      <div class="card-head"><div><h2 class="card-title">修复执行回执</h2><p class="card-subtitle">{{ productUxReceipt.title }}</p></div><span class="badge success">{{ productUxReceipt.title }}</span></div>
      <div class="task-row"><strong>执行结果</strong><span>已按确认范围完成修复。</span></div>
      <div class="task-row"><strong>重新检查</strong><span>{{ productUxReceipt.detail }}</span></div>
      <div class="task-row"><strong>下一步</strong><span>可以继续创作。</span></div>
    </section>

    <section v-if="productUxEvidenceMode === 'm14-stale' || productUxEvidenceMode === 'm14-blocked' || productUxEvidenceMode === 'm14-error'" class="card pad section" data-product-ux-state="error-state">
      <div class="card-head"><div><h2 class="card-title">当前状态</h2><p class="card-subtitle">{{ productUxStateTitle }}</p></div><span class="badge warning">需要处理</span></div>
      <p>{{ productUxStateMessage }}</p>
      <div class="task-row"><strong>建议</strong><span>请刷新当前作品后重新核对，再决定下一步。</span></div>
    </section>

    <div class="metric-grid section">
      <MetricCard label="当前阶段" :value="production.jobStatus === 'FAILED' ? '生成失败' : stageLabel(production.productionStage)" :note="detailLabel(production.activeWork.detail)" />
      <MetricCard label="正式章节" :value="project.committedChapters" note="只有正式确认的正文会计入这里" />
      <MetricCard label="黄金三章" :value="`第 ${openingProgress?.completed ?? 0} / ${openingProgress?.total ?? 3} 章`" :note="openingProgress?.currentChapter ? `当前第 ${openingProgress.currentChapter} 章` : '黄金三章已完成'" />
      <MetricCard label="准入检查" :value="gateLabel(production.gate?.status)" :note="productErrorMessage(production.gate?.reason, '等待检查结果。')" />
      <MetricCard label="后续安排" :value="reconciliationLabel(production.reconciliation)" note="系统会根据当前记录给出下一步" />
    </div>

    <div class="production-layout section">
      <section class="card pad" data-production-progress>
        <div class="card-head"><div><h2 class="card-title">生产时间线</h2><p class="card-subtitle">上下文 → 规划 → 撰写 → 检查 → 准入 → 提交 → 后续安排</p></div></div>
        <div v-if="openingProgress" class="task-row"><strong>黄金三章：第 {{ openingProgress.completed }} / {{ openingProgress.total }} 章</strong><span>当前第 {{ openingProgress.currentChapter ?? '—' }} 章 · 上一章{{ openingProgress.previousCommitted ? '已正式提交' : '尚无' }} · 下一章{{ openingProgress.nextPrepared ? '已准备' : '未准备' }}</span></div>
        <div class="timeline">
          <div v-for="item in production.timeline" :key="item.id" class="timeline-item" :class="item.status">
            <div class="timeline-marker">{{ item.status === 'done' ? '✓' : item.order }}</div>
            <div><strong>{{ timelineLabel(item.title) }}</strong><div class="muted" style="font-size:12px">{{ timelineDescription(item) }}</div></div>
          </div>
        </div>
      </section>
      <aside style="display:grid;gap:16px">
        <section class="card pad">
          <h2 class="card-title">当前章节规划</h2>
          <template v-if="production.chapterPlan">
            <p><strong>{{ production.chapterPlan.chapterGoal }}</strong></p>
            <small class="muted">当前规划已与作品版本绑定</small>
            <p class="muted">依据范围：当前任务所需的作品记录</p>
            <p class="muted">规划状态：{{ production.stale?.status === 'PLAN_STALE' ? '需要重新确认' : '可以使用' }}<span v-if="production.stale?.reason"> · {{ productErrorMessage(production.stale.reason, '当前规划需要重新确认。') }}</span></p>
            <div v-for="item in production.triggeredContinuity" :key="item.threadRef" class="task-row"><strong>相关剧情线</strong><small>{{ detailLabel(item.bridge, '这条剧情线需要继续核对。') }}</small></div>
          </template>
          <p v-else class="muted">开篇阶段会先整理当前所需信息，再建立第一章规划。</p>
        </section>
        <section class="card pad">
          <h2 class="card-title">创作步骤与结果</h2>
          <div v-for="run in production.modelRuns" :key="run.modelRunId" class="task-row"><strong>{{ capabilityLabel(run.capability) }}</strong><small class="muted">上次尝试：{{ run.model }} · {{ statusLabel(run.status) }}</small></div>
          <p class="muted">制作稿：{{ production.candidateRef ? '已生成，等待检查' : '无' }}</p>
          <p class="muted">正式正文：{{ production.finalChapterRef ? '已提交' : '无' }}</p>
        </section>
      </aside>
    </div>

    <section v-if="production.creativeSkills || production.quality" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">创作建议与叙事质量</h2><p class="card-subtitle">建议用于改善正文表现；正式准入仍以作品记录和结构化检查为准。</p></div>
        <span class="muted">{{ production.quality?.softOnly ? '辅助质量评估' : '等待质量评估' }}</span>
      </div>
      <div v-if="production.creativeSkills" class="metric-grid">
        <MetricCard label="创作建议状态" value="已加载" note="使用当前作品的受限建议" />
        <MetricCard label="主要建议" :value="production.creativeSkills.selection?.primary?.nameZh || '无'" note="辅助参考" />
        <MetricCard label="补充建议数" :value="String(production.creativeSkills.selection?.support?.length || 0)" note="只展示当前任务需要的内容" />
        <MetricCard label="本次使用" :value="String(production.creativeSkills.invocations?.length || 0)" note="已记录到当前作品" />
      </div>
      <div v-if="production.creativeSkills" class="task-row">
        <strong>当前作品版本 {{ production.creativeSkills.context?.projectRevision ?? '-' }}</strong>
        <small class="muted">已选择 {{ production.creativeSkills.context?.boundedRefs?.length || 0 }} 项必要记录 · 本次使用 {{ production.creativeSkills.context?.budget?.modelCalls || 0 }} 次创作调用</small>
      </div>
      <div v-if="production.quality" class="quality-grid">
        <div v-for="([dimension, item]) in qualityDimensions" :key="dimension" class="task-row">
          <strong>{{ dimensionLabel(dimension) }}</strong><span>{{ item.score }} 分 · {{ item.severity === 'risk' ? '需重点关注' : item.severity === 'attention' ? '建议关注' : '稳定' }}</span>
        </div>
      </div>
      <div v-if="production.quality" class="task-row">
        <strong>读者期待：{{ production.quality.softAssessment?.readerDesire || '等待评估' }}</strong>
        <small class="muted">当前偏好版本已记录 · 本次评估 {{ production.quality.modelProvenance?.evaluatorCalls || 0 }} 次</small>
      </div>
      <div v-if="production.quality?.repair" class="task-row">
        <strong>局部修订 {{ production.quality.repair.run }}/{{ production.quality.repair.limit }}</strong>
        <small class="muted">影响范围：{{ production.quality.repair.scope.map(scopeLabel).join('、') || '无' }} · {{ detailLabel(production.quality.repair.escalation, '未升级') }}</small>
      </div>
      <div v-for="finding in qualityFindings" :key="finding.findingId" class="task-row">
        <strong>{{ dimensionLabel(finding.dimension) }}</strong><small class="muted">{{ finding.repairable ? '可以做局部修订' : '仅供查看' }}</small>
      </div>
    </section>

    <section v-if="production.repair" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">需要确认的修复</h2><p class="card-subtitle">系统会先说明影响范围；只有你明确确认后才会执行，并在完成后重新核对。</p></div>
        <button v-if="production.repair.status === 'PROPOSED'" class="btn primary" @click="authorizeRepair">明确授权</button>
        <button v-if="production.repair.status === 'AUTHORIZED' && !production.repair.consumedAt" class="btn primary" @click="executeRepair">执行一次性修复</button>
      </div>
      <div class="metric-grid">
        <MetricCard label="状态" :value="statusLabel(production.repair.status)" :note="production.repair.authorizationId ? '已获得本次确认' : '尚未确认'" />
        <MetricCard label="影响等级" :value="production.repair.riskLevel === 'LOW' ? '较低' : '需要关注'" :note="production.repair.requiresBackup ? '执行前需要备份' : '无需备份'" />
        <MetricCard label="核对范围" :value="production.repair.affectedScopes.map(scopeLabel).join('、') || '当前作品'" note="执行后会再次核对" />
      </div>
      <p><strong>影响范围：</strong>{{ production.repair.affectedScopes.map(scopeLabel).join('、') || '无' }}</p>
      <p><strong>处理方式：</strong>重新整理并核对当前作品展示。</p>
      <div v-if="production.repair.executionReceipt" class="task-row">
        <strong>{{ statusLabel(production.repair.executionReceipt.status) }}</strong><small class="muted">修复结果已经重新核对</small>
      </div>
    </section>
  </div>
</template>
