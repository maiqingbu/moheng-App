<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import RuntimeBanner from '../components/RuntimeBanner.vue'
import NarrativeValidation from '../components/NarrativeValidation.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import ResumeConditionFields from '../components/ResumeConditionFields.vue'
import { useProjectStore } from '../stores/projectStore'
import type { ProductionCenterProjection, RunState } from '../../application/contracts'
import { isProductUxEvidenceDemoEnabled, productErrorMessage, receiptSummary, repairExecutionLabel, sanitizeProductText, scopeLabel as productScopeLabel, statusLabel as productStatusLabel } from '../presentation/productLanguage'
import { createProductionRuntimePoller, isProductionJobTerminal } from '../runtime/productionRuntimeSync'
import { buildResponsibilityEvidence, createResumeConditionDraft, serializeResumeCondition, type ResumeConditionDraft } from '../flows/resumeCondition'
import { resumeConditionLabel } from '../presentation/continuity'

const store = useProjectStore()
const route = useRoute()
const project = computed(() => store.project)
const production = computed(() => store.production)
const productionLoadError = ref('')
const lastCommand = ref('none')
const selectedSigningPlatform = ref('')
const repairReverifyInFlight = ref(false)
const commandState = computed(() => store.commandState(lastCommand.value))
const repairExecutionPending = computed(() => {
  const state = store.commandState('repair-execute')
  return state.confirmation === 'pending' || state.repairOutcome === 'unknown' || state.repairOutcome === 'receipt'
})
const productionStartState = computed(() => store.commandState('production-start'))
const evaluationCommandState = computed(() => store.commandState('opening-evaluate'))
const signingConfirmationCommandState = computed(() => store.commandState('signing-confirm'))
const bookPlanCommandState = computed(() => store.commandState('book-plan-revise'))
const qualityDimensions = computed(() => Object.entries(production.value?.quality?.dimensions ?? {}))
const qualityFindings = computed(() => production.value?.quality?.findings ?? [])
const qualityRepairScope = computed(() => production.value?.quality?.repair?.scope ?? [])
const repairAffectedScopes = computed(() => production.value?.repair?.affectedScopes ?? [])
const openingProgress = computed(() => production.value?.openingProgress)
const signing = computed(() => production.value?.signing)
const bookPlan = computed(() => production.value?.bookPlan)
const passingSigningPlatforms = computed(() => signing.value?.platformMatrix?.filter(item => item.mainstream && item.status === 'PASS') ?? [])
const newResponsibilities = computed(() => production.value?.newResponsibilities ?? [])
const newHardConstraintCount = computed(() => newResponsibilities.value.filter(item =>
  ['A', 'B'].includes(item.authorityLevel) && !['ABANDONED', 'RESOLVED', 'SUPERSEDED'].includes(item.lifecycle),
).length)
const responsibilityAdjustmentInFlight = ref<string | null>(null)
const gatePolicyUpdateInFlight = ref(false)
const gatePolicyRuleDraft = ref('{"predicate":"alive","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}')
const gatePolicyDraftError = ref('')
const fulfillmentObjectDraft = reactive<Record<string, string>>({})
const resumeConditionDraft = reactive<Record<string, ResumeConditionDraft>>({})
const advancedResumeConditionDraft = reactive<Record<string, string>>({})
const responsibilityFormError = ref('')
const evidenceStartDraft = reactive<Record<string, number | undefined>>({})
const evidenceEndDraft = reactive<Record<string, number | undefined>>({})
const evidenceQuoteDraft = reactive<Record<string, string>>({})
const selectedSigningPlatformId = computed(() => passingSigningPlatforms.value.some(item => item.platformId === selectedSigningPlatform.value)
  ? selectedSigningPlatform.value
  : passingSigningPlatforms.value[0]?.platformId ?? '')
const stageLabel = (value: unknown) => ({ CONTEXT: '准备创作上下文', PLAN: '编译章节计划', RENDER: '生成正文', CANDIDATE: '整理正文候选', AUDIT: '审计与事实提取', GATE: '一致性与准入校验', COMMIT: '正式提交', RECONCILE: '状态回写与后续判断', COMPLETED: '本章完成', OPENING: '开篇准备', OPENING_PRODUCTION: '开篇创作', VOLUME_PRODUCTION: '长篇创作', IDLE: '等待开始' } as Record<string, string>)[String(value)] ?? '进行中'
const gateLabel = (value: unknown) => ({ PASS: '已通过', BLOCKED: '暂时阻断', PENDING: '等待检查', FAIL: '未通过' } as Record<string, string>)[String(value)] ?? '等待检查'
const reconciliationLabel = (value: unknown) => ({ NO_ACTION: '无需处理', RECOMPILE_NEXT_CHAPTER: '需要重新规划下一章', RECOMPILE_ROLLING_WINDOW: '需要更新后续预览', REPLAN_VOLUME_ROUTE: '需要调整本卷路线', CHANGE_REQUEST: '需要正式变更确认' } as Record<string, string>)[String(value)] ?? '等待判断'
const capabilityLabel = (value: unknown) => ({ RENDER_CHAPTER: '撰写章节', AUDIT_EXTRACT: '检查章节', QUALITY_EVALUATION: '评估叙事质量' } as Record<string, string>)[String(value)] ?? '创作步骤'
const timelineLabel = (value: unknown) => ({ CONTEXT: '准备创作上下文', PLAN: '编译章节计划', RENDER: '生成正文', CANDIDATE: '整理正文候选', AUDIT: '审计与事实提取', GATE: '一致性与准入校验', COMMIT: '正式提交', RECONCILE: '状态回写与后续判断', COMPLETED: '本章完成' } as Record<string, string>)[String(value)] ?? '创作步骤'
const timelineDescription = (item: { title?: unknown; description?: unknown; status?: unknown }) => {
  if (item.status === 'pending' || item.status === 'waiting') return '等待执行。'
  if (item.status === 'blocked' || item.status === 'failed') return sanitizeProductText(item.description, '本步骤未完成，请核对阻断原因。')
  if (item.status === 'done') return sanitizeProductText(item.description, '本步骤已完成。')
  const title = String(item.title ?? '')
  if (title === 'CONTEXT') return '正在整理当前作品记录和本章所需上下文。'
  if (title === 'PLAN') return '正在建立本章目标、冲突和结构安排。'
  if (title === 'RENDER') return '正在生成正文候选，完成后还需检查和确认。'
  if (title === 'CANDIDATE') return '正在检查正文候选与当前作品约束。'
  if (title === 'AUDIT') return '正在核对连续性、质量和正式准入条件。'
  if (title === 'GATE') return '正式确认前的一致性与准入校验正在进行。'
  if (title === 'COMMIT') return '正在把正文、事实和回执正式写入作品记录。'
  if (title === 'RECONCILE') return '正在回写正式结果并准备下一章。'
  if (item.status === 'blocked' && production.value?.jobStatus === 'CANCELLED') return '当前任务已取消，未伪装为完成。'
  return sanitizeProductText(item.description, '当前步骤状态待确认。')
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
  && signing.value?.confirmation?.confirmed === true
  && (bookPlan.value?.status !== 'APPROVED' || bookPlan.value?.reapprovalRequired === true))
const canConfirmSigning = computed(() => signing.value?.status === 'PASS'
  && signing.value?.confirmation?.confirmed !== true
  && passingSigningPlatforms.value.length > 0)
const canStartProduction = computed(() => !productionLoadError.value && production.value?.validation?.canExecute !== false && production.value?.stale?.status !== 'PLAN_STALE' && Boolean(production.value?.nextChapter) && (productionIsActive.value || !production.value?.jobStatus || ['COMPLETED', 'CANCELLED'].includes(production.value.jobStatus) || canRetryProduction.value))
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
  onProjection: () => { productionLoadError.value = '' },
  onError: () => { productionLoadError.value = '创作状态暂时无法更新；当前展示为上次查询结果，请重新读取后再操作。' },
  isActiveProjection: projection => projection.runtimeOperation?.active === true || projection.isActive === true,
  isTerminalProjection: projection => {
    if (projection.runtimeOperation) return ['COMPLETED', 'FAILED', 'BLOCKED', 'CANCELLED'].includes(projection.runtimeOperation.status)
    return isProductionJobTerminal(projection.jobStatus)
  },
})

onMounted(() => {
  void loadProductionProjection()
})

onUnmounted(() => runtimePoller.stop())

async function loadProductionProjection() {
  productionLoadError.value = ''
  try {
    const value = await store.loadProduction(true)
    if (!value) throw new Error('生产状态暂不可用。')
    if (value.runtimeOperation?.active || (value.jobStatus && !isProductionJobTerminal(value.jobStatus))) runtimePoller.start()
  } catch (error) {
    productionLoadError.value = error instanceof Error ? error.message : '暂时无法读取创作状态。'
  }
}

function focusProductionProgress() {
  document.querySelector<HTMLElement>('[data-production-progress]')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

function focusOpeningEvaluationProgress() {
  document.querySelector<HTMLElement>('[data-opening-evaluation-progress]')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

type ResponsibilityItem = NonNullable<ProductionCenterProjection['newResponsibilities']>[number]

function resumeConditionForm(item: ResponsibilityItem): ResumeConditionDraft {
  return resumeConditionDraft[item.threadRef] ?? createResumeConditionDraft(item.resumeCondition)
}

function updateResumeConditionForm(item: ResponsibilityItem, value: ResumeConditionDraft) {
  resumeConditionDraft[item.threadRef] = value
}

async function adjustResponsibility(
  item: NonNullable<ProductionCenterProjection['newResponsibilities']>[number],
  action: 'DOWNGRADE' | 'UPGRADE' | 'DELETE',
  authorityLevel?: 'A' | 'B',
) {
  if (responsibilityAdjustmentInFlight.value) return
  responsibilityFormError.value = ''
  const key = `thread-responsibility-${item.threadRef}`
  const payload: Record<string, unknown> = { threadRef: item.threadRef, action }
  if (action === 'UPGRADE') {
    let resumeCondition: Record<string, unknown> | null = null
    const rawAdvancedCondition = store.advancedMode ? advancedResumeConditionDraft[item.threadRef]?.trim() : ''
    if (rawAdvancedCondition) {
      try {
        const parsed: unknown = JSON.parse(rawAdvancedCondition)
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('invalid condition')
        resumeCondition = parsed as Record<string, unknown>
      } catch {
        responsibilityFormError.value = '高级兑现条件 JSON 格式不正确。'
        return
      }
    } else {
      resumeCondition = serializeResumeCondition(resumeConditionDraft[item.threadRef] ?? createResumeConditionDraft(item.resumeCondition))
    }
    if (!resumeCondition) {
      responsibilityFormError.value = '请补全兑现条件所需的信息。'
      return
    }
    payload.resumeCondition = resumeCondition
    const fulfillmentObject = fulfillmentObjectDraft[item.threadRef]?.trim() || item.fulfillmentObject
    if (fulfillmentObject) payload.fulfillmentObject = fulfillmentObject
    if (item.evidence) {
      payload.evidence = item.evidence
    } else {
      const evidence = buildResponsibilityEvidence(
        evidenceQuoteDraft[item.threadRef] ?? '',
        evidenceStartDraft[item.threadRef],
        evidenceEndDraft[item.threadRef],
        store.advancedMode,
      )
      if (evidence) payload.evidence = evidence
    }
  }
  responsibilityAdjustmentInFlight.value = item.threadRef
  lastCommand.value = key
  try {
    await store.runCommand(key, {
      type: 'thread_responsibility_adjust',
      projectId: project.value?.id,
      expectedRevision: project.value?.revision,
      payload: { ...payload, ...(authorityLevel ? { authorityLevel } : {}) },
    })
    await store.loadProduction(true)
  } catch {
    // CommandFeedback keeps the typed result for the user.
  } finally {
    responsibilityAdjustmentInFlight.value = null
  }
}

async function updateGatePolicy(action: 'ADOPT_DEFAULT' | 'UPSERT_RULE' | 'DISABLE_RULE', fields: Record<string, unknown>) {
  if (gatePolicyUpdateInFlight.value) return
  const commandKey = `gate-policy-${action.toLowerCase()}-${Date.now()}`
  gatePolicyUpdateInFlight.value = true
  lastCommand.value = commandKey
  try {
    await store.runCommand(commandKey, {
      type: 'gate_policy_update',
      payload: { action, ...fields },
    })
    await store.loadProduction(true)
  } catch {
    // CommandFeedback displays the typed backend result.
  } finally {
    gatePolicyUpdateInFlight.value = false
  }
}

async function adoptDefaultGatePolicy() {
  await updateGatePolicy('ADOPT_DEFAULT', {})
}

async function saveGatePolicyRule() {
  gatePolicyDraftError.value = ''
  let rule: Record<string, unknown>
  try {
    const parsed: unknown = JSON.parse(gatePolicyRuleDraft.value)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('规则必须是 JSON 对象')
    rule = parsed as Record<string, unknown>
  } catch (error) {
    gatePolicyDraftError.value = error instanceof Error ? error.message : '规则 JSON 格式无效'
    return
  }
  await updateGatePolicy('UPSERT_RULE', { rule })
}

async function disableGatePolicyRule(rule: Record<string, unknown>) {
  const predicate = typeof rule.predicate === 'string' ? rule.predicate : ''
  if (!predicate) return
  await updateGatePolicy('DISABLE_RULE', { predicate })
}

async function startProduction() {
  if (!canStartProduction.value) return
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
  if (signing.value?.status === 'STALE') return '签约规则已更新，请重新评估'
  if (signing.value?.evaluation?.status === 'FAILED') return '评估未完成'
  if (evaluationInFlight.value || signing.value?.evaluation?.status === 'IN_REVIEW') return '正在评估'
  if (['PASS', 'FAIL', 'NOT_VERIFIED', 'REVISE', 'REJECTED'].includes(String(signing.value?.status))) return '评估完成'
  return '等待三章完成'
}

function signingVerdictLabel() {
  if (signing.value?.status === 'STALE') return '需要重新评估'
  if (signing.value?.evaluation?.status === 'FAILED') return '评估失败，尚无有效结论'
  if (signing.value?.status === 'PASS') return 'PASS'
  if (signing.value?.status === 'FAIL') return 'FAIL'
  if (signing.value?.status === 'NOT_VERIFIED') return '无法验证，不能确认签约'
  if (signing.value?.status === 'REVISE') return '未通过（历史评估）'
  if (signing.value?.status === 'REJECTED') return '未通过'
  return '尚未评估'
}

function signingItemLabel(itemId: string) {
  const labels: Record<string, string> = {
    'C0-2': '内容禁区', 'C0-3': '原创性', 'F1-1': '章节字数与标题', 'F1-2': '标点与排版',
    'F1-3': '提纲式写文', 'F1-4': '剧情复述', 'F1-5': '句式重复',
    'R2-1': '注水', 'R2-2': '流水账', 'R2-3': '逻辑连贯', 'R2-4': '语言机械', 'R2-5': '表述清晰', 'R2-6': '风格统一',
    'E3-1': '主线清晰', 'E3-2': '情绪与代入', 'E3-3': '情节流畅', 'E3-4': '开篇切入', 'E3-5': '核心看点', 'E3-6': '情绪回报',
    'E3-7': '章末钩子', 'E3-8': '人物辨识度', 'E3-9': '长篇可扩展', 'E3-11': '危机感',
  }
  if (itemId.startsWith('E3-10:')) return '平台定位匹配'
  return labels[itemId] ?? itemId
}

function itemResultLabel(result: unknown) {
  return ({ PASS: '通过', FAIL: '未通过', NOT_VERIFIED: '无法验证' } as Record<string, string>)[String(result)] ?? '待检查'
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
  if (bookPlan.value?.reapprovalRequired === true) return '更新正式总纲'
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

async function confirmSigning() {
  if (!canConfirmSigning.value || !selectedSigningPlatformId.value) return
  lastCommand.value = 'signing-confirm'
  try {
    await store.runCommand(lastCommand.value, {
      type: 'signing_confirm',
      payload: { platformId: selectedSigningPlatformId.value },
    })
    await store.loadProduction(true)
  } catch {
    // Store-owned command feedback keeps the typed failure visible.
  }
}

const planValidityLabel = computed(() => {
  switch (production.value?.stale?.status) {
    case 'VALID': return '后端已确认有效'
    case 'PLAN_STALE': return '需要重新确认'
    case 'NO_PLAN': return '当前没有可执行章节计划'
    default: return '等待相关状态复核'
  }
})

async function finalizeBookPlan() {
  if (!canFinalizeBookPlan.value) return
  if (bookPlanInFlight.value || production.value?.runtimeOperation?.kind === 'B02::BOOK_PLAN' && production.value.runtimeOperation.active) {
    focusOpeningEvaluationProgress()
    return
  }
  lastCommand.value = 'book-plan-revise'
  runtimePoller.start({ waitForActive: true })
  try {
    await store.refresh()
    await store.loadProduction(true)
    if (!canFinalizeBookPlan.value) return
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
  lastCommand.value = 'repair-authorize'
  await store.runCommand(lastCommand.value, {
    type: 'repair_authorize',
    payload: { repairPlanId: repair.repairPlanId, operationDigest: repair.operationDigest },
  })
}

async function executeRepair() {
  const repair = production.value?.repair
  if (!repair?.authorizationId || repair.status !== 'AUTHORIZED') return
  lastCommand.value = 'repair-execute'
  if (repairExecutionPending.value) {
    await store.confirmCommandResult(lastCommand.value)
    return
  }
  try {
    await store.runCommand(lastCommand.value, {
      type: 'repair_execute',
      payload: { repairPlanId: repair.repairPlanId, authorizationId: repair.authorizationId },
    })
  } catch {
    // The command feedback shows the durable result or its pending state.
  } finally {
    try {
      await store.loadProduction(true)
      if (store.commandState(lastCommand.value).repairOutcome === 'unknown') {
        await store.confirmCommandResult(lastCommand.value)
      }
    } catch { /* preserve the command result and allow a read retry */ }
  }
}

async function reverifyRepair() {
  const repair = production.value?.repair
  if (repairReverifyInFlight.value
    || !repair?.authorizationId
    || repair.executionReceipt?.status !== 'REPAIR_VERIFICATION_INCOMPLETE'
    || !repair.consumedAt) return
  repairReverifyInFlight.value = true
  lastCommand.value = 'repair-reverify'
  try {
    await store.runCommand(lastCommand.value, {
      type: 'repair_execute',
      payload: { repairPlanId: repair.repairPlanId, authorizationId: repair.authorizationId },
    })
  } catch {
    // The command feedback shows the durable result or its pending state.
  } finally {
    try {
      await store.loadProduction(true)
      if (store.commandState(lastCommand.value).repairOutcome === 'unknown') {
        await store.confirmCommandResult(lastCommand.value)
      }
    } catch { /* preserve the command result and allow a read retry */ }
    repairReverifyInFlight.value = false
  }
}

function costText(value: number | null | undefined) {
  return value == null ? '未设上限' : `¥${value.toFixed(2)}`
}
</script>

<template>
  <div v-if="project && production" class="page">
    <PageHeader title="创作" subtitle="从章节准备到正文候选、检查与正式确认，都从当前作品记录重新查询。">
      <button v-if="canStartProduction && productionStartState.confirmation !== 'pending'" class="btn primary" @click="startProduction">{{ productionStartInFlight || productionIsActive ? '查看第' : (canRetryProduction ? '重新生成' : '开始') }}{{ productionStartInFlight || productionIsActive ? ` ${production.nextChapter?.number} 章进度` : `第 ${production.nextChapter?.number} 章` }}</button>
      <button v-if="productionIsActive" class="btn" @click="controlProduction('production_pause')">暂停</button>
      <button v-if="productionStatus === 'PAUSED'" class="btn primary" @click="controlProduction('production_resume')">恢复</button>
    </PageHeader>
    <RuntimeBanner :state="authoritativeRunState" :reason="project.blockReason" />
    <CommandFeedback :state="commandState" @confirm="store.confirmCommandResult(lastCommand)" />
    <div v-if="productionLoadError" class="banner warning" role="alert"><span>{{ productionLoadError }}</span><button class="btn" @click="loadProductionProjection">重新读取</button></div>
    <NarrativeValidation :validation="production.validation" :advanced="store.advancedMode" />

    <section v-if="production.costSummary" class="card pad section" data-cost-summary aria-label="模型成本与预算">
      <div class="card-head"><div><h2 class="card-title">模型成本与预算</h2><p class="card-subtitle">预算按作品设置。已设置的上限会在发送前校验并预留预测成本。</p></div></div>
      <div class="setting-grid">
        <div class="task-row"><span>本章已用 / 预留</span><strong>{{ costText(production.costSummary.chapterUsed ?? 0) }}</strong></div>
        <div class="task-row"><span>本章上限 / 剩余</span><strong>{{ costText(production.costSummary.chapterBudget) }}<template v-if="production.costSummary.chapterRemaining != null"> · 剩余 {{ costText(production.costSummary.chapterRemaining) }}</template></strong></div>
        <div class="task-row"><span>本月已用 / 预留</span><strong>{{ costText(production.costSummary.monthUsed ?? 0) }}</strong></div>
        <div class="task-row"><span>本月上限 / 剩余</span><strong>{{ costText(production.costSummary.monthlyBudget) }}<template v-if="production.costSummary.monthRemaining != null"> · 剩余 {{ costText(production.costSummary.monthRemaining) }}</template></strong></div>
      </div>
    </section>

    <section v-if="production.gatePolicy" class="card pad section" data-gate-policy>
      <div class="card-head">
        <div>
          <span class="eyebrow">一致性规则</span>
          <h2 class="card-title" style="margin-top:6px">GatePolicy · 第 {{ production.gatePolicy.version }} 版</h2>
          <p class="card-subtitle">规则版本与摘要会绑定到规划和正式检查记录。</p>
        </div>
        <span class="badge" :class="production.gatePolicy.needsUpgrade ? 'warning' : 'success'">{{ production.gatePolicy.needsUpgrade ? '需要升级' : '当前规则有效' }}</span>
      </div>
      <div v-if="production.gatePolicy.needsUpgrade" class="task-row" style="align-items:flex-start;flex-wrap:wrap">
        <span>当前长篇规则仍是旧开篇策略。采用默认规则集会创建新的 GatePolicy 版本，并保留旧版本供查询。</span>
        <button class="btn primary" :disabled="gatePolicyUpdateInFlight" @click="adoptDefaultGatePolicy">采用默认规则集</button>
      </div>
      <template v-else>
        <p v-if="production.gatePolicy.resourceRef" class="muted">默认资源 {{ production.gatePolicy.resourceRef.version }} · SHA-256 {{ production.gatePolicy.resourceRef.sha256 }}</p>
        <div v-for="rule in production.gatePolicy.rules" :key="String(rule.predicate)" class="task-row" style="align-items:flex-start">
          <code>{{ JSON.stringify(rule) }}</code>
          <button class="btn" :disabled="gatePolicyUpdateInFlight" @click="disableGatePolicyRule(rule)">停用</button>
        </div>
        <label class="section" style="display:grid;gap:8px">
          新增或修改谓词规则（JSON）
          <textarea v-model="gatePolicyRuleDraft" rows="4" autocomplete="off" spellcheck="false" />
        </label>
        <p v-if="gatePolicyDraftError" class="banner warning" role="alert">{{ gatePolicyDraftError }}</p>
        <button class="btn primary" :disabled="gatePolicyUpdateInFlight" @click="saveGatePolicyRule">保存规则新版本</button>
      </template>
    </section>

    <section class="card pad section opening-evaluation-panel" data-opening-evaluation>
      <div class="card-head">
        <div>
          <span class="eyebrow">开篇评估 · 模拟签约</span>
          <h2 class="card-title" style="margin-top:6px">开篇评估</h2>
          <p class="card-subtitle">只读取三章已正式提交的正文、事实与证据；评估结果和签约回执来自后端正式记录。</p>
        </div>
        <span class="badge" :class="signing?.status === 'PASS' ? 'success' : signing?.status === 'FAIL' || signing?.status === 'NOT_VERIFIED' || signing?.status === 'STALE' || signing?.status === 'REVISE' || signing?.status === 'REJECTED' ? 'warning' : 'info'">{{ evaluationStatusLabel() }}</span>
      </div>
      <CommandFeedback :state="evaluationCommandState" @confirm="store.confirmCommandResult('opening-evaluate')" />
      <CommandFeedback :state="signingConfirmationCommandState" @confirm="store.confirmCommandResult('signing-confirm')" />
      <CommandFeedback :state="bookPlanCommandState" @confirm="store.confirmCommandResult('book-plan-revise')" />
      <div class="evaluation-steps" v-if="signing?.evaluation?.steps?.length" data-opening-evaluation-progress>
        <div v-for="step in signing.evaluation.steps" :key="step.id" class="evaluation-step" :class="evaluationStepClass(step.status)">
          <span class="evaluation-step-marker">{{ step.status === 'done' ? '✓' : step.status === 'active' ? '…' : '·' }}</span>
          <span>{{ step.label }}</span>
        </div>
      </div>
      <div class="task-row" aria-live="polite"><strong>评估结果</strong><span>{{ signingVerdictLabel() }} · {{ signing?.verdict || '等待三章正式提交后开始评估。' }}</span></div>
      <div v-if="signing?.coreStatus" class="task-row"><strong>通用核心</strong><span>{{ itemResultLabel(signing.coreStatus) }}</span></div>
      <div v-if="signing?.platformMatrix?.length" class="signing-matrix" aria-label="平台适配矩阵">
        <h3 class="section-title">平台适配矩阵</h3>
        <article v-for="platform in signing.platformMatrix" :key="platform.platformId" class="signing-platform">
          <div class="task-row"><strong>{{ platform.displayName }}</strong><span>{{ itemResultLabel(platform.status) }}<span v-if="platform.submissionWords?.minimum !== null && platform.submissionWords?.minimum !== undefined"> · 距投稿门槛约 {{ platform.submissionWords.distance }} 字</span></span></div>
          <div v-for="item in platform.items" :key="`${platform.platformId}:${item.itemId}`" class="signing-item-detail">
            <strong>{{ signingItemLabel(item.itemId) }} · {{ itemResultLabel(item.result) }}</strong>
            <span>{{ item.reason }}</span>
          </div>
        </article>
      </div>
      <div v-if="signing?.itemResults?.length" class="signing-results" aria-label="逐项评估结果">
        <h3 class="section-title">逐项评估与原文依据</h3>
        <article v-for="item in signing.itemResults" :key="item.itemId" class="signing-item-detail">
          <strong>{{ signingItemLabel(item.itemId) }} · {{ itemResultLabel(item.result) }}</strong>
          <span>{{ item.reason }}</span>
          <blockquote v-for="(evidence, index) in item.evidence ?? []" :key="`${item.itemId}:${index}`">
            第 {{ evidence.chapterNo }} 章：{{ evidence.exactQuote }}
          </blockquote>
        </article>
      </div>
      <div class="task-row"><strong>评估依据摘要</strong><span>三章 {{ signing?.summary?.chapters ?? '未提供' }} · 事实 {{ signing?.summary?.canonRefs ?? '未提供' }} 条 · 证据 {{ signing?.summary?.evidenceRefs ?? '未提供' }} 条</span></div>
      <div class="task-row"><strong>正式签约回执</strong><span>{{ signing?.receipt?.id ? `${signing.receipt.id} · 已提交` : '尚未生成' }}</span></div>
      <div v-if="signing?.confirmation?.confirmed" class="task-row"><strong>签约确认</strong><span>已确认 {{ signing.confirmation.platformId }} · {{ signing.confirmation.confirmedAt }}</span></div>
      <div v-if="signing?.declarationPrompt" class="task-row"><strong>AI 生成声明提示</strong><span>{{ signing.declarationPrompt }}</span></div>
      <div class="task-row"><strong>正式总纲</strong><span>{{ signing?.status === 'STALE' ? '历史版本保留，等待依据复核' : bookPlan?.reapprovalRequired ? '新签约回执可生成新版本，旧版本仍保留' : bookPlan?.status === 'APPROVED' ? '已生成正式版本' : '等待评估通过后生成' }}<span v-if="bookPlan?.seedPreserved"> · 开篇草稿历史已保留</span></span></div>
      <div v-if="bookPlan?.steps?.length" class="evaluation-steps" aria-label="正式总纲运行步骤">
        <div v-for="step in bookPlan.steps" :key="step.id" class="evaluation-step" :class="evaluationStepClass(step.status)">
          <span class="evaluation-step-marker">{{ step.status === 'done' ? '✓' : step.status === 'active' ? '…' : '·' }}</span>
          <span>{{ step.label }}</span>
        </div>
      </div>
      <div class="actions" style="margin-top:14px">
        <button v-if="canEvaluateOpening" class="btn primary" @click="evaluateOpening">{{ evaluationActionLabel() }}</button>
        <label v-if="canConfirmSigning" class="field signing-platform-select">
          <span class="field-label">确认签约平台</span>
          <select :value="selectedSigningPlatformId" class="select" @change="selectedSigningPlatform = ($event.target as HTMLSelectElement).value">
            <option v-for="platform in passingSigningPlatforms" :key="platform.platformId" :value="platform.platformId">{{ platform.displayName }}</option>
          </select>
        </label>
        <button v-if="canConfirmSigning" class="btn primary" @click="confirmSigning">确认签约</button>
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
      <MetricCard label="黄金三章" :value="`第 ${openingProgress?.completed ?? '—'} / ${openingProgress?.total ?? 3} 章`" :note="openingProgress?.currentChapter ? `当前第 ${openingProgress.currentChapter} 章` : '当前章信息尚未提供'" />
      <MetricCard label="准入检查" :value="gateLabel(production.gate?.status)" :note="productErrorMessage(production.gate?.reason, '等待检查结果。')" />
      <MetricCard label="后续安排" :value="reconciliationLabel(production.reconciliation)" note="系统会根据当前记录给出下一步" />
    </div>

    <section v-if="newResponsibilities.length" class="card pad section" data-new-responsibilities>
      <div class="card-head">
        <div>
          <h2 class="card-title">新增约束 {{ newHardConstraintCount }} 条</h2>
          <p class="card-subtitle">本章审计发现的连续性事项。B 级进入后续章节上下文；C 级保留为候选，不作为硬约束。</p>
        </div>
      </div>
      <div v-for="item in newResponsibilities" :key="item.threadRef" class="task-row" style="align-items:flex-start;flex-wrap:wrap">
        <div style="flex:1;min-width:240px">
          <strong>{{ item.authorityLevel }} 级 · {{ item.threadRef }}</strong>
          <p class="muted" style="margin:6px 0">{{ detailLabel(item.description, '审计记录中没有说明文本。') }}</p>
          <p v-if="item.evidence?.quote" class="muted" style="margin:6px 0">原文证据：“{{ item.evidence.quote }}”</p>
          <p v-if="item.fulfillmentObject" class="muted" style="margin:6px 0">兑现对象：{{ item.fulfillmentObject }}</p>
          <p v-if="item.authorityLevel === 'B'" class="muted" style="margin:6px 0">兑现条件：{{ resumeConditionLabel(item.resumeCondition) }}</p>
          <small class="muted">规则版本 {{ item.ruleVersion ?? '未知' }} · {{ statusLabel(item.lifecycle) }}</small>
        </div>
        <div v-if="!['ABANDONED', 'RESOLVED', 'SUPERSEDED'].includes(item.lifecycle)" class="actions">
          <template v-if="item.authorityLevel === 'B'">
            <button class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'DOWNGRADE')">降为 C 级</button>
            <button class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'DELETE')">撤销责任</button>
          </template>
          <template v-else-if="item.authorityLevel === 'C'">
            <div class="responsibility-upgrade-fields" style="display:grid;gap:8px;flex-basis:100%;min-width:260px">
              <label style="display:grid;gap:4px">升级时的兑现对象
                <input v-model="fulfillmentObjectDraft[item.threadRef]" :placeholder="item.fulfillmentObject ?? '人物、事件或物件引用'" />
              </label>
              <label style="display:grid;gap:4px">升级时的兑现条件
                <ResumeConditionFields :model-value="resumeConditionForm(item)" @update:model-value="updateResumeConditionForm(item, $event)" />
              </label>
              <label v-if="store.advancedMode" style="display:grid;gap:4px">原始兑现条件 JSON
                <textarea v-model="advancedResumeConditionDraft[item.threadRef]" :placeholder="JSON.stringify(item.resumeCondition ?? {})" rows="2" />
              </label>
              <div v-if="!item.evidence" class="responsibility-evidence-fields">
                <small class="muted">普通模式只需粘贴原文引文，系统会在该责任的来源正文中定位唯一位置。</small>
                <input v-model="evidenceQuoteDraft[item.threadRef]" placeholder="逐字证据引文" />
                <template v-if="store.advancedMode">
                  <small class="muted">高级模式可手动指定零起点字符范围。</small>
                  <input v-model.number="evidenceStartDraft[item.threadRef]" type="number" min="0" placeholder="证据起点（字符）" />
                  <input v-model.number="evidenceEndDraft[item.threadRef]" type="number" min="0" placeholder="证据终点（字符）" />
                </template>
              </div>
              <p v-if="responsibilityFormError" class="banner warning" role="alert">{{ responsibilityFormError }}</p>
            </div>
            <button class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'UPGRADE', 'B')">升级为 B 级</button>
            <button class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'UPGRADE', 'A')">升级为 A 级</button>
            <button class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'DELETE')">撤销责任</button>
          </template>
          <button v-else class="btn" :disabled="responsibilityAdjustmentInFlight !== null" @click="adjustResponsibility(item, 'DELETE')">撤销责任</button>
        </div>
        <small v-else class="muted">责任已结束；历史审计记录与证据仍可查询。</small>
      </div>
    </section>

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
            <p class="muted">规划状态：{{ planValidityLabel }}<span v-if="production.stale?.reason"> · {{ productErrorMessage(production.stale.reason, '当前规划需要重新确认。') }}</span></p>
            <div v-for="item in production.triggeredContinuity" :key="item.threadRef" class="task-row"><strong>相关剧情线</strong><small>{{ detailLabel(item.bridge, '这条剧情线需要继续核对。') }}</small></div>
          </template>
          <p v-else class="muted">当前没有可执行章节计划。未规划、被阻断或已完结时可以为空；开篇无需先创建全卷计划。</p>
        </section>
        <section class="card pad">
          <h2 class="card-title">创作步骤与结果</h2>
          <div v-for="run in production.modelRuns" :key="run.modelRunId" class="task-row"><strong>{{ capabilityLabel(run.capability) }}</strong><small class="muted">上次尝试：{{ run.model }} · {{ statusLabel(run.status) }}</small></div>
          <p class="muted">制作稿：{{ production.candidateRef ? '已生成，等待检查' : '无' }}</p>
          <p class="muted">正式正文：{{ production.finalChapterRef ? '已提交' : '无' }}</p>
        </section>
      </aside>
    </div>

    <section v-if="store.advancedMode" class="card pad section">
      <h2 class="card-title">规划与签约来源</h2>
      <dl class="pitch-kv">
        <div><dt>执行模式</dt><dd>{{ production.nextChapter?.executionMode ?? '无当前执行章' }}</dd></div>
        <div><dt>编译 revision / digest</dt><dd>{{ production.chapterPlan?.compiledFromProjectRevision ?? '未提供' }} / {{ production.chapterPlan?.compiledFromStateDigest ?? '未提供' }}</dd></div>
        <div><dt>ContextManifest</dt><dd>{{ production.chapterPlan?.contextManifestRef?.artifactId ?? '未提供' }} · v{{ production.chapterPlan?.contextManifestRef?.version ?? '—' }}</dd></div>
        <div><dt>签约受评章节摘要</dt><dd>{{ signing?.receipt?.chapterHashes?.join('、') || '未提供' }}</dd></div>
        <div><dt>开篇合同 / 评估规则版本</dt><dd>{{ signing?.receipt?.openingContractVersion ?? '未提供' }} / {{ signing?.receipt?.platformPolicyVersion ?? '未提供' }}</dd></div>
        <div><dt>签约证据引用</dt><dd>{{ signing?.signingEvidenceRefs?.join('、') || '未提供' }}</dd></div>
      </dl>
    </section>

    <section v-if="production.creativeSkills || production.quality" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">创作建议与叙事质量</h2><p class="card-subtitle">建议用于改善正文表现；正式准入仍以作品记录和结构化检查为准。</p></div>
        <span class="muted">{{ production.quality?.softOnly ? '辅助质量评估' : '等待质量评估' }}</span>
      </div>
      <div v-if="production.creativeSkills" class="metric-grid">
        <MetricCard label="创作建议状态" value="已加载" note="使用当前作品的受限建议" />
        <MetricCard label="主要建议" :value="production.creativeSkills.selection?.primary?.nameZh || '无'" note="辅助参考" />
        <MetricCard label="补充建议数" :value="String(production.creativeSkills.selection?.support?.length || 0)" note="只展示当前任务需要的内容" />
        <MetricCard label="方法注入" :value="String(production.creativeSkills.methodInjectionProvenance?.length || 0)" note="已关联正文生成记录" />
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
        <small class="muted">当前偏好版本已记录 · 评估方式 {{ production.quality.evaluatorKind === 'HEURISTIC_KEYWORD' ? '关键词启发式' : '待确认' }}</small>
      </div>
      <div v-if="production.quality?.repair" class="task-row">
        <strong>{{ production.quality.repair.status === 'ADVISORY_ESCALATION' ? '质量提示待复核' : `局部修订 ${production.quality.repair.run}/${production.quality.repair.limit}` }}</strong>
        <small class="muted">影响范围：{{ qualityRepairScope.map(scopeLabel).join('、') || '无' }} · {{ production.quality.repair.status === 'ADVISORY_ESCALATION' ? '正文未修改，建议进一步复核' : detailLabel(production.quality.repair.escalation, '未升级') }}</small>
      </div>
      <div v-for="finding in qualityFindings" :key="finding.findingId" class="task-row">
        <strong>{{ dimensionLabel(finding.dimension) }}</strong><small class="muted">{{ finding.repairable ? '建议复核' : '仅供查看' }}</small>
      </div>
    </section>

    <section v-if="production.repair" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">需要确认的修复</h2><p class="card-subtitle">系统会先说明影响范围；只有你明确确认后才会执行，并在完成后重新核对。</p></div>
        <button v-if="production.repair.status === 'PROPOSED'" class="btn primary" @click="authorizeRepair">明确授权</button>
        <button v-if="production.repair.status === 'AUTHORIZED' && !production.repair.consumedAt && !repairExecutionPending" class="btn primary" @click="executeRepair">执行一次性修复</button>
        <button v-if="production.repair.executionReceipt?.status === 'REPAIR_VERIFICATION_INCOMPLETE' && production.repair.authorizationId && production.repair.consumedAt" class="btn primary" :disabled="repairReverifyInFlight" @click="reverifyRepair">{{ repairReverifyInFlight ? '正在重新核对…' : '重新核对结果' }}</button>
      </div>
      <div class="metric-grid">
        <MetricCard label="状态" :value="statusLabel(production.repair.status)" :note="production.repair.authorizationId ? '已获得本次确认' : '尚未确认'" />
        <MetricCard label="影响等级" :value="production.repair.riskLevel === 'LOW' ? '较低' : '需要关注'" :note="production.repair.requiresBackup ? '执行前需要备份' : '无需备份'" />
        <MetricCard label="核对范围" :value="repairAffectedScopes.map(scopeLabel).join('、') || '当前作品'" note="执行后会再次核对" />
      </div>
      <p><strong>影响范围：</strong>{{ repairAffectedScopes.map(scopeLabel).join('、') || '无' }}</p>
      <p><strong>处理方式：</strong>重新整理并核对当前作品展示。</p>
      <div v-if="production.repair.executionReceipt" class="task-row">
        <strong>{{ repairExecutionLabel(production.repair.executionReceipt) }}</strong><small class="muted">{{ production.repair.executionReceipt.status === 'REPAIR_VERIFICATION_INCOMPLETE' ? '重新核对只检查现有回执，不会再次执行正式操作。' : production.repair.executionReceipt.status === 'REPAIR_VERIFICATION_FAILED' ? '请查看回执中的验证项；系统不会自动重试正式操作。' : production.repair.executionReceipt.status === 'REPAIR_VERIFIED' ? '正式操作及其预期结果已核对。' : '请根据回执状态核对结果。' }}</small>
      </div>
    </section>
  </div>
  <div v-else-if="project" class="page">
    <PageHeader title="创作" subtitle="正在重新读取当前作品的正式创作状态。" />
    <div class="banner warning"><div><div class="banner-title">暂时无法读取创作状态</div><div class="banner-copy">{{ productionLoadError || '正在读取当前作品记录…' }}</div></div></div>
    <div class="actions" style="margin-top:14px"><button class="btn primary" @click="loadProductionProjection">重试读取</button></div>
  </div>
</template>
