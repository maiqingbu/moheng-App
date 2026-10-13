<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import BaseModal from '../components/BaseModal.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { confirmRetconPlan, createRetconPlan, createRetconProposal, monitorDependencySummary, startDependencySummaryBootstrap, type DependencySummaryStatus, type RetconDraft, type RetconPlanView } from '../flows/b12RetconFlow'
import { isReaderProposalOnlySource, readerProposalStatusLabel } from '../flows/readerRevision'
import type { HistoryEntry, RetconContextProjection } from '../../application/contracts'
import { changeStatusLabel, evidenceDisplayLabel, evidenceSelector, isProductUxEvidenceDemoEnabled, productErrorMessage, sanitizeProductText, selectEvidenceBySelector, scopeLabel } from '../presentation/productLanguage'
import { readerHistoryStatusLabel } from '../flows/readerRevision'

const store = useProjectStore()
const route = useRoute()
const productUxEvidenceMode = computed(() => isProductUxEvidenceDemoEnabled() ? String(route.query.productUxEvidence ?? '') : '')
type HistoryKind = 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON'
const filters: { value: HistoryKind; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: 'VERSION', label: '版本' },
  { value: 'CHANGE_REQUEST', label: '修改' },
  { value: 'RETCON', label: '事实修订' }
]
const kind = ref<HistoryKind>('all')
const loadedHistoryKind = ref<HistoryKind | null>(null)
const historyFilterLoading = ref(false)
const historyFilterError = ref('')
const failedHistoryKind = ref<HistoryKind | null>(null)
let historyFilterRequestSequence = 0
const requestText = ref('')
const retconTargetRef = ref('')
const retconReplacement = ref('')
const retconEvidenceRef = ref('')
const retconContext = ref<RetconContextProjection | null>(null)
const retconContextLoading = ref(false)
const modal = ref<'change' | 'retcon' | null>(null)
const history = computed(() => store.history)
const state = computed(() => store.commandState('history-create'))
const impactState = computed(() => store.commandState('history-impact'))
const decisionState = computed(() => store.commandState('history-decision'))
const applyState = computed(() => store.commandState('history-apply'))
const retconPlanState = computed(() => store.commandState('retcon-plan'))
const retconDecisionState = computed(() => store.commandState('retcon-decision'))
const retconApplyState = computed(() => store.commandState('retcon-apply'))
const formalWriteOutcomePending = computed(() => [
  state.value,
  impactState.value,
  decisionState.value,
  applyState.value,
  retconPlanState.value,
  retconDecisionState.value,
  retconApplyState.value,
].some(command => command.confirmation === 'pending'))
const activeChange = ref<{ id: string; type: 'CHANGE' | 'RETCON'; digest: string; scope: string; status: string; expandedEdges: number; totalEdges: number; source?: string; preflightPending?: boolean; targetFactId?: string; replacement?: string; evidenceRef?: string; evidenceVersion?: number; evidencePayloadHash?: string; planId?: string; planDigest?: string; plan?: RetconPlanView } | null>(null)
const impactItems = ref<string[]>([])
const actionMessage = ref('')
const historyStatusLabel = (value: string) => changeStatusLabel(value)
function historyEntryStatusLabel(entry: HistoryEntry) {
  return readerHistoryStatusLabel(entry) ?? historyStatusLabel(entry.status)
}
function historyEntryStatusClass(entry: HistoryEntry) {
  if (entry.source === 'MANUSCRIPT_READER') return entry.status === 'rejected' ? '' : 'warning'
  return entry.status === 'applied' ? 'success' : entry.status === 'impact_ready' ? 'warning' : ''
}
function historyFilterLabel(value: HistoryKind) {
  return filters.find(item => item.value === value)?.label ?? '全部'
}
async function loadHistoryKind(next: HistoryKind, force: boolean) {
  const requestSequence = ++historyFilterRequestSequence
  loadedHistoryKind.value = loadedHistoryKind.value ?? store.historyKind ?? null
  kind.value = next
  historyFilterLoading.value = true
  historyFilterError.value = ''
  failedHistoryKind.value = null
  try {
    await store.loadHistory(next, force)
    if (requestSequence !== historyFilterRequestSequence) return
    loadedHistoryKind.value = next
  } catch {
    if (requestSequence !== historyFilterRequestSequence) return
    const hasHistoryProjection = store.history !== null
    const restoredKind = store.historyKind ?? loadedHistoryKind.value ?? 'all'
    kind.value = restoredKind
    loadedHistoryKind.value = restoredKind
    failedHistoryKind.value = next
    historyFilterError.value = hasHistoryProjection
      ? `未能加载“${historyFilterLabel(next)}”记录，仍显示“${historyFilterLabel(restoredKind)}”记录。`
      : '暂时无法加载历史记录，请重试。'
  } finally {
    if (requestSequence === historyFilterRequestSequence) historyFilterLoading.value = false
  }
}
const changeScopeLabel = (value: unknown) => scopeLabel(value)
const readerProposalOnly = computed(() => isReaderProposalOnlySource(activeChange.value?.source))
const activeChangeStatusLabel = computed(() => {
  if (!readerProposalOnly.value || !activeChange.value) return changeStatusLabel(activeChange.value?.status ?? '')
  return readerProposalStatusLabel(activeChange.value.status)
})
const changeImpactLabel = (value: unknown) => productErrorMessage(value, '影响范围已完成分析。')
const impactSummary = computed(() => impactItems.value.length ? `已识别 ${impactItems.value.length} 项可能受影响的内容${impactItems.value.length > 12 ? '，其余内容将在继续分析后显示。' : '。'}` : '')

async function openRetconModal() {
  modal.value = 'retcon'
  if (retconContext.value || retconContextLoading.value) return
  retconContextLoading.value = true
  try {
    retconContext.value = await store.queryRetconContext(requestText.value.trim())
    retconTargetRef.value = retconContext.value.facts.find(item => item.current)?.factId ?? ''
    const firstEvidence = retconContext.value.evidence.find(item => item.supportsFactId === retconTargetRef.value && item.relationship === 'FACT_SOURCE')
    retconEvidenceRef.value = firstEvidence ? evidenceSelector(firstEvidence) : ''
  } catch (error) {
    actionMessage.value = productErrorMessage(error, '暂时无法读取当前事实和依据。')
  } finally {
    retconContextLoading.value = false
  }
}

function selectedEvidence() {
  const options = retconContext.value?.evidence.filter(item => item.supportsFactId === retconTargetRef.value && item.relationship === 'FACT_SOURCE') ?? []
  return selectEvidenceBySelector(options, retconEvidenceRef.value)
}

const selectedFactLabel = computed(() => retconContext.value?.facts.find(item => item.factId === retconTargetRef.value)?.label ?? '')
const retconEvidenceOptions = computed(() => retconContext.value?.evidence.filter(item => item.supportsFactId === retconTargetRef.value && item.relationship === 'FACT_SOURCE') ?? [])

function chooseFact(event: Event) {
  const label = (event.target as HTMLInputElement).value
  retconTargetRef.value = retconContext.value?.facts.find(item => item.label === label && item.current)?.factId ?? ''
  const firstEvidence = retconContext.value?.evidence.find(item => item.supportsFactId === retconTargetRef.value && item.relationship === 'FACT_SOURCE')
  retconEvidenceRef.value = firstEvidence ? evidenceSelector(firstEvidence) : ''
}

function retconDraftFromActive(): RetconDraft | null {
  const active = activeChange.value
  if (!active?.targetFactId || !active.replacement || !active.evidenceRef || active.evidenceVersion === undefined) return null
  const evidence = retconContext.value?.evidence.find(item => item.artifactId === active.evidenceRef
    && item.version === active.evidenceVersion
    && (!active.evidencePayloadHash || item.payloadHash === active.evidencePayloadHash))
    ?? {
      artifactId: active.evidenceRef,
      version: active.evidenceVersion,
      payloadHash: active.evidencePayloadHash ?? '',
      label: '已核验依据',
    }
  if (!evidence.payloadHash) return null
  return {
    projectId: store.currentProjectId ?? '',
    changeRequestId: active.id,
    summary: '用户确认的事实修订',
    targetFactId: active.targetFactId,
    replacement: active.replacement,
    evidenceRef: evidence,
    impactDigest: active.digest,
  }
}

function planFactLabel(fact: Record<string, unknown> | undefined) {
  if (!fact) return '未读取到当前正式事实。'
  const predicate = typeof fact.predicate === 'string' ? fact.predicate : '正式事实'
  const object = fact.objectJson && typeof fact.objectJson === 'object' ? fact.objectJson as Record<string, unknown> : {}
  const description = sanitizeProductText(object.description, '当前记录')
  return `${predicate}：${description}`
}

function startSummaryBootstrap(changeId: string) {
  const projectId = store.currentProjectId
  if (!projectId) return
  actionMessage.value = '正在准备全局影响索引，可继续浏览当前页面。'
  void startDependencySummaryBootstrap(
    (key, request) => store.runCommand(key, request),
    projectId,
  ).then(() => monitorDependencySummary(
    () => store.queryDependencySummaryBootstrap() as Promise<DependencySummaryStatus>,
    async () => {
      const preflight = await store.runCommand('history-impact', {
        type: 'impact_preflight',
        projectId,
        payload: { changeRequestId: changeId },
      })
      if (activeChange.value?.id !== changeId) return
      const result = preflight.result ?? {}
      activeChange.value = {
        ...activeChange.value,
        digest: typeof result.impactDigest === 'string' ? result.impactDigest : activeChange.value.digest,
        scope: typeof result.scopeClass === 'string' ? result.scopeClass : activeChange.value.scope,
        status: typeof result.status === 'string' ? result.status : activeChange.value.status,
        expandedEdges: typeof result.expandedEdges === 'number' ? result.expandedEdges : activeChange.value.expandedEdges,
        totalEdges: typeof result.totalEdges === 'number' ? result.totalEdges : activeChange.value.totalEdges,
      }
      actionMessage.value = '影响索引已准备完成，可以重新分析影响范围。'
    },
    error => { actionMessage.value = productErrorMessage(error, '影响范围分析失败，请刷新后重试。') },
  )).catch(error => { actionMessage.value = productErrorMessage(error, '影响范围分析失败，请刷新后重试。') })
}

async function restoreChange(changeId: string) {
  try {
    const detail = await store.queryChangeRequest(changeId)
    let impact = detail.impact && typeof detail.impact === 'object' ? detail.impact as Record<string, unknown> : {}
    const changeType = detail.changeType === 'RETCON' ? 'RETCON' : 'CHANGE'
    const requested = detail.requestedChange && typeof detail.requestedChange === 'object' ? detail.requestedChange as Record<string, unknown> : {}
    let impactDigest = typeof detail.impactDigest === 'string' ? detail.impactDigest : ''
    let preflightPending = false
    let bootstrapPending = route.query.bootstrap === '1'
    if (requested.source === 'MANUSCRIPT_READER' && impact.status === 'NOT_STARTED') {
      try {
        const preflight = await store.runCommand('history-impact', {
          type: 'impact_preflight',
          projectId: store.currentProjectId ?? undefined,
          payload: { changeRequestId: changeId },
        })
        const result = preflight.result ?? {}
        impact = { ...impact, ...result }
        impactDigest = typeof result.impactDigest === 'string' ? result.impactDigest : ''
        bootstrapPending ||= result.status === 'SUMMARY_BOOTSTRAP_REQUIRED'
      } catch (error) {
        preflightPending = true
        actionMessage.value = productErrorMessage(error, '请求已保存；影响分析暂未完成，可以重试。')
      }
    }
    const replacement = requested.replacement && typeof requested.replacement === 'object' ? requested.replacement as Record<string, unknown> : {}
    const replacementObject = replacement.objectJson && typeof replacement.objectJson === 'object' ? replacement.objectJson as Record<string, unknown> : {}
    const evidence = requested.evidenceRef && typeof requested.evidenceRef === 'object' ? requested.evidenceRef as Record<string, unknown> : {}
    const storedPlan = detail.retconPlan && typeof detail.retconPlan === 'object' ? detail.retconPlan as Record<string, unknown> : undefined
    activeChange.value = {
      id: changeId,
      type: changeType,
      source: typeof requested.source === 'string' ? requested.source : undefined,
      digest: impactDigest,
      scope: typeof impact.scopeClass === 'string' ? impact.scopeClass : 'LOCAL',
      status: typeof impact.status === 'string' && impact.status !== 'NOT_STARTED'
        ? impact.status
        : typeof detail.status === 'string' ? detail.status : 'PREFLIGHTED',
      expandedEdges: Number(impact.expandedEdges ?? 0),
      totalEdges: Number(impact.totalEdges ?? 0),
      preflightPending,
      targetFactId: typeof detail.targetFactId === 'string' ? detail.targetFactId : undefined,
      replacement: typeof replacementObject.description === 'string' ? replacementObject.description : undefined,
      evidenceRef: typeof evidence.artifactId === 'string' ? evidence.artifactId : typeof requested.evidenceRef === 'string' ? requested.evidenceRef : undefined,
      evidenceVersion: typeof evidence.version === 'number' ? evidence.version : undefined,
      evidencePayloadHash: typeof evidence.payloadHash === 'string' ? evidence.payloadHash : undefined,
      planId: typeof detail.retconPlanId === 'string' ? detail.retconPlanId : undefined,
      planDigest: typeof storedPlan?.planDigest === 'string' ? storedPlan.planDigest : undefined,
      plan: storedPlan as RetconPlanView | undefined,
    }
    if (bootstrapPending) startSummaryBootstrap(changeId)
  } catch (error) {
    actionMessage.value = productErrorMessage(error, '无法恢复这项变更提案。')
  }
}

// FX-12: withdraw-responsibility requests decided here, outside the B12 impact flow.
const withdrawItems = computed(() => store.withdrawRequests?.items ?? [])
const withdrawOriginLabel = (origin: string) => origin === 'USER_DELETE' ? '你删除的责任' : '章节审核发现'
async function decideWithdraw(changeRequestId: string, decision: 'APPROVE' | 'REJECT') {
  try {
    await store.decideWithdrawRequest(changeRequestId, decision)
  } catch (error) {
    actionMessage.value = productErrorMessage(error, '撤销请求未能处理，请刷新后重试。')
  }
}

onMounted(() => {
  void loadHistoryKind('all', false)
  void store.loadWithdrawRequests().catch(() => undefined)
  if (productUxEvidenceMode.value === 'm12') {
    activeChange.value = {
      id: 'r1r2-demo-change',
      type: 'RETCON',
      digest: 'd'.repeat(64),
      scope: 'LOCAL',
      status: 'PLAN_READY',
      expandedEdges: 1,
      totalEdges: 1,
      targetFactId: 'fact:demo',
      replacement: '已确认的新事实',
      evidenceRef: 'artifact:demo-evidence',
      evidenceVersion: 4,
      evidencePayloadHash: 'h'.repeat(64),
    }
    actionMessage.value = '本次只确认当前修复方案；确认后系统才会执行，并在完成后重新核对。'
  }
  const changeId = route.query.changeRequestId
  if (typeof changeId === 'string' && changeId.trim()) void restoreChange(changeId)
  if (route.query.createChangeRequest === '1') {
    modal.value = 'change'
    const description = typeof route.query.continuityDescription === 'string' ? route.query.continuityDescription.trim() : ''
    requestText.value = description ? `请评估是否需要调整这条剧情线：${description}` : '请评估是否需要调整这条剧情线。'
  }
})
async function setKind(next: HistoryKind) {
  if (historyFilterLoading.value) return
  await loadHistoryKind(next, true)
}
async function retryHistoryFilter() {
  if (failedHistoryKind.value) await loadHistoryKind(failedHistoryKind.value, true)
}
async function loadMoreHistory() {
  const cursor = history.value?.entries.nextCursor
  if (cursor) await store.loadHistory(kind.value, false, cursor)
}
async function create() {
  const projectId = store.currentProjectId
  if (!projectId || !requestText.value.trim()) return
  const type = modal.value === 'retcon' ? 'RETCON' : 'CHANGE'
  const continuityThreadRef = typeof route.query.continuityThreadRef === 'string' ? route.query.continuityThreadRef : ''
  const changeId = `change:${projectId}:${Date.now()}`
  const evidence = type === 'RETCON' ? selectedEvidence() : undefined
  if (type === 'RETCON' && (!retconTargetRef.value || !retconReplacement.value.trim() || !evidence)) {
    actionMessage.value = '请先选择当前事实、填写新的事实内容，并选择已有依据。'
    return
  }
  const created = await store.runCommand('history-create', {
    type: 'change_request_create',
    projectId,
    payload: {
      changeRequestId: changeId,
      changeType: type,
      scope: 'PROJECT',
      summary: requestText.value.trim(),
      reason: requestText.value.trim(),
      targetFactId: type === 'RETCON' ? retconTargetRef.value.trim() : undefined,
      targetRefs: type === 'RETCON' ? [retconTargetRef.value.trim()] : continuityThreadRef ? [continuityThreadRef] : undefined,
      sourceRefs: type === 'RETCON' ? [retconTargetRef.value.trim()] : continuityThreadRef ? [continuityThreadRef] : undefined,
      requestedChange: {
        description: requestText.value.trim(),
        ...(type === 'RETCON' ? {
          replacement: { objectJson: { description: retconReplacement.value.trim() } },
          evidenceRef: evidence,
        } : {}),
      },
    },
  })
  let preflight = await store.runCommand('history-impact', {
    type: 'impact_preflight',
    projectId,
    payload: { changeRequestId: changeId },
  })
  const summaryBootstrapPending = preflight.result?.status === 'SUMMARY_BOOTSTRAP_REQUIRED'
  const result = preflight.result ?? created.result ?? {}
  activeChange.value = {
    id: changeId,
    type,
    digest: typeof result.impactDigest === 'string' ? result.impactDigest : '',
    scope: typeof result.scopeClass === 'string' ? result.scopeClass : 'LOCAL',
    status: summaryBootstrapPending ? 'SUMMARY_BOOTSTRAP_REQUIRED' : typeof result.status === 'string' ? result.status : 'PREFLIGHTED',
    expandedEdges: typeof result.expandedEdges === 'number' ? result.expandedEdges : 0,
    totalEdges: typeof result.totalEdges === 'number' ? result.totalEdges : 0,
    targetFactId: type === 'RETCON' ? retconTargetRef.value.trim() : undefined,
    replacement: type === 'RETCON' ? retconReplacement.value.trim() : undefined,
    evidenceRef: type === 'RETCON' ? evidence?.artifactId : undefined,
    evidenceVersion: type === 'RETCON' ? evidence?.version : undefined,
    evidencePayloadHash: type === 'RETCON' ? evidence?.payloadHash : undefined,
  }
  impactItems.value = []
  modal.value = null
  requestText.value = ''
  retconTargetRef.value = ''
  retconReplacement.value = ''
  retconEvidenceRef.value = ''
  await loadHistoryKind('all', true)
  if (summaryBootstrapPending) startSummaryBootstrap(changeId)
}
async function continueImpact() {
  if (!activeChange.value) return
  if (activeChange.value.preflightPending || !activeChange.value.digest) {
    const preflight = await store.runCommand('history-impact', {
      type: 'impact_preflight',
      projectId: store.currentProjectId ?? undefined,
      payload: { changeRequestId: activeChange.value.id },
    })
    const next = preflight.result ?? {}
    const bootstrapPending = next.status === 'SUMMARY_BOOTSTRAP_REQUIRED'
    activeChange.value = {
      ...activeChange.value,
      digest: typeof next.impactDigest === 'string' ? next.impactDigest : '',
      scope: typeof next.scopeClass === 'string' ? next.scopeClass : activeChange.value.scope,
      status: typeof next.status === 'string' ? next.status : activeChange.value.status,
      expandedEdges: Number(next.expandedEdges ?? activeChange.value.expandedEdges),
      totalEdges: Number(next.totalEdges ?? activeChange.value.totalEdges),
      preflightPending: false,
    }
    actionMessage.value = '影响分析已更新。'
    if (bootstrapPending) startSummaryBootstrap(activeChange.value.id)
    return
  }
  const result = await store.runCommand('history-impact', {
    type: 'impact_expand',
    projectId: store.currentProjectId ?? undefined,
    payload: { changeRequestId: activeChange.value.id, pageSize: 40, continueGlobal: true },
  })
  const next = result.result ?? {}
  activeChange.value = { ...activeChange.value, status: String(next.status ?? activeChange.value.status), expandedEdges: Number(next.expandedEdges ?? activeChange.value.expandedEdges), totalEdges: Number(next.totalEdges ?? activeChange.value.totalEdges), digest: String(next.impactDigest ?? activeChange.value.digest) }
  const items = Array.isArray(next.items) ? next.items : []
  impactItems.value = [...impactItems.value, ...items.map(item => {
    const target = item && typeof item === 'object' && typeof (item as Record<string, unknown>).targetRef === 'string' ? (item as Record<string, unknown>).targetRef as string : ''
    const chapter = target.match(/(?:chapter:|第)(?:[^:：]*[:：])?(\d+)$/i)?.[1]
    return chapter ? `第 ${chapter} 章` : '相关规划内容'
  })]
}
async function cancelImpact() {
  if (!activeChange.value) return
  await store.runCommand('history-impact', { type: 'impact_cancel', projectId: store.currentProjectId ?? undefined, payload: { changeRequestId: activeChange.value.id } })
  activeChange.value = { ...activeChange.value, status: 'CANCELLED' }
}
async function rejectChange() {
  if (!activeChange.value || !activeChange.value.digest) return
  await store.runCommand('history-decision', { type: 'change_request_decide', projectId: store.currentProjectId ?? undefined, payload: { changeRequestId: activeChange.value.id, decision: 'REJECT', impactDigest: activeChange.value.digest, scopeAcknowledged: true, riskAcknowledged: true } })
  activeChange.value = { ...activeChange.value, status: 'REJECTED' }
}
async function approveAndApply() {
  if (!activeChange.value || !activeChange.value.digest) return
  if (isReaderProposalOnlySource(activeChange.value.source)) {
    actionMessage.value = '这条章节修改建议仅作为意见留存；目前没有正式修订方案，正文未修改。'
    return
  }
  if (activeChange.value.type === 'RETCON') {
    const draft = retconDraftFromActive()
    if (!draft) {
      actionMessage.value = '请先选择当前事实、填写新的事实内容，并选择已有依据。'
      return
    }
    if (!activeChange.value.plan) {
      const plan = await createRetconPlan((key, request) => store.runCommand(key, request), draft)
      if (!plan.retconPlanId || !plan.planDigest) {
        actionMessage.value = '事实修订方案未能完成核对。'
        return
      }
      activeChange.value = { ...activeChange.value, planId: plan.retconPlanId, planDigest: plan.planDigest, plan, status: 'PLAN_READY' }
      actionMessage.value = '事实修订方案已生成，请核对当前事实、依据、影响和风险后再确认执行。'
      return
    }
    let result: Awaited<ReturnType<typeof confirmRetconPlan>>
    try {
      result = await confirmRetconPlan((key, request) => store.runCommand(key, request), { ...draft, ...activeChange.value.plan })
    } catch {
      actionMessage.value = '事实修订提交结果待确认。请只重新读取现有正式状态；系统不会重新提交。'
      await loadHistoryKind('all', true)
      return
    }
    activeChange.value = { ...activeChange.value, status: result.postverify?.status === 'VERIFIED' ? 'APPLIED' : 'VERIFY_REQUIRED' }
    actionMessage.value = result.postverify?.status === 'VERIFIED'
      ? '事实修订已完成，并已重新核对版本历史。'
      : '事实修订已提交，结果待确认。请重新读取状态，不要重复提交。'
    await loadHistoryKind('all', true)
    return
  }
  const projectId = store.currentProjectId
  if (!projectId) return
  const logicalChangeId = `${projectId}:${activeChange.value.id}`
  const decisionIdentity = `b12-change-decision:${logicalChangeId}`
  await store.runCommand('history-decision', {
    type: 'change_request_decide',
    projectId,
    commandId: decisionIdentity,
    idempotencyKey: decisionIdentity,
    payload: { changeRequestId: activeChange.value.id, decision: 'APPROVE', impactDigest: activeChange.value.digest, scopeAcknowledged: true, riskAcknowledged: true },
  })
  const applyIdentity = `b12-change-apply:${logicalChangeId}`
  let result: Awaited<ReturnType<typeof store.runCommand>>
  try {
    result = await store.runCommand('history-apply', {
      type: 'change_request_apply',
      projectId,
      commandId: applyIdentity,
      idempotencyKey: applyIdentity,
      payload: { changeRequestId: activeChange.value.id, impactDigest: activeChange.value.digest },
    })
  } catch {
    actionMessage.value = '普通修改结果待确认，提交状态未知。请只重新读取现有正式状态；系统不会重新提交。'
    await loadHistoryKind('all', true)
    return
  }
  activeChange.value = { ...activeChange.value, status: result.postverify?.status === 'VERIFIED' ? 'APPLIED' : 'VERIFY_REQUIRED' }
  actionMessage.value = result.postverify?.status === 'VERIFIED'
    ? '普通修改已完成，并已重新核对当前状态。'
    : '普通修改已提交，结果待确认。请重新读取状态，不要重复提交。'
  await loadHistoryKind('all', true)
}
</script>

<template>
  <div v-if="history || historyFilterLoading || historyFilterError" class="page">
    <PageHeader title="版本与变更记录" subtitle="正式历史不可覆盖；普通修改先做影响分析，已生效的核心事实需要单独确认。">
      <button class="btn" @click="modal = 'change'">新建修改</button>
      <button class="btn warning" @click="openRetconModal">新建事实修订计划</button>
    </PageHeader>

    <div v-if="history" class="metric-grid section">
      <div class="metric-card"><div class="metric-label">版本记录</div><div class="metric-value">{{ history.stats.versions }}</div></div>
      <div class="metric-card"><div class="metric-label">修改请求</div><div class="metric-value">{{ history.stats.changes }}</div></div>
      <div class="metric-card"><div class="metric-label">事实修订</div><div class="metric-value">{{ history.stats.retcons }}</div></div>
      <div class="metric-card"><div class="metric-label">待确认</div><div class="metric-value">{{ history.stats.pending }}</div></div>
    </div>

    <div class="segmented section" style="width:max-content">
      <button v-for="item in filters" :key="item.value" :class="{ active: kind === item.value }" :disabled="historyFilterLoading" :aria-pressed="kind === item.value" @click="setKind(item.value)">{{ item.label }}</button>
    </div>
    <div v-if="historyFilterLoading" class="banner info section" role="status" aria-live="polite">正在加载{{ historyFilterLabel(kind) }}记录…</div>
    <div v-else-if="historyFilterError" class="banner warning section" role="alert">
      <span>{{ historyFilterError }}</span>
      <button v-if="failedHistoryKind" class="btn small" style="margin-left:10px" @click="retryHistoryFilter">重试</button>
    </div>

    <section v-if="withdrawItems.length" class="card pad section" data-withdraw-requests>
      <div class="card-head"><div><h2 class="card-title">待确认的撤销责任请求</h2><p class="card-subtitle">批准后这项责任正式撤销，原审计记录与证据仍然保留；保留则责任继续有效。</p></div><span class="badge warning">{{ withdrawItems.length }} 项</span></div>
      <div v-for="item in withdrawItems" :key="item.changeRequestId" class="list-row">
        <div><strong>{{ sanitizeProductText(item.summary, '撤销一项剧情责任') }}</strong><div class="muted">{{ withdrawOriginLabel(item.origin) }}</div></div>
        <div class="actions">
          <button class="btn small warning" @click="decideWithdraw(item.changeRequestId, 'APPROVE')">批准撤销</button>
          <button class="btn small" @click="decideWithdraw(item.changeRequestId, 'REJECT')">保留责任</button>
        </div>
        <CommandFeedback :state="store.commandState(`withdraw-decision:${item.changeRequestId}`)" />
      </div>
      <p v-if="store.withdrawRequests?.truncated" class="banner info">仅显示部分，已截断。</p>
    </section>

    <section v-if="activeChange" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">{{ readerProposalOnly ? '章节修改建议' : '当前变更方案' }}</h2><p class="card-subtitle">{{ readerProposalOnly ? '这条建议仅作意见留存并用于影响分析，尚无正式修订方案。' : '先看预计影响，再决定是否继续；正式执行后会再次核对结果。' }}</p></div><span class="badge" :class="!readerProposalOnly && activeChange.status === 'APPLIED' ? 'success' : 'warning'">{{ activeChangeStatusLabel }}</span></div>
      <p v-if="readerProposalOnly" class="banner info">章节修改建议仅作为意见留存，当前没有可执行的正式修订方案；正式章节正文未修改。</p>
      <div class="metric-grid"><div class="metric-card"><div class="metric-label">影响范围</div><div class="metric-value">{{ changeScopeLabel(activeChange.scope) }}</div></div><div class="metric-card"><div class="metric-label">已分析内容</div><div class="metric-value">{{ activeChange.expandedEdges }} / {{ activeChange.totalEdges }}</div></div></div>
      <div v-if="impactItems.length" class="banner info"><div><div class="banner-title">受影响内容</div><div class="banner-copy">{{ impactSummary }}</div></div></div>
      <p v-if="activeChange.type === 'RETCON'" class="banner warning">这项请求涉及已经确认的事实，需先查看原事实、依据和事实修订方案。</p>
      <div v-if="productUxEvidenceMode === 'm12'" class="banner warning" data-product-ux-state="authorization">
        <div><div class="banner-title">修复授权</div><div class="banner-copy">本次只授权当前方案一次；确认后才会执行，并在完成后重新核对。</div></div>
        <div class="actions"><button class="btn small" @click="actionMessage = '已保留当前方案，等待你的确认。'">暂不执行</button><button class="btn primary small" @click="actionMessage = '已确认本次执行，完成后会重新核对。'">确认本次执行</button></div>
      </div>
      <div v-if="activeChange.plan" class="card pad" style="display:grid;gap:8px;background:var(--surface-muted, #f7f5ef)">
        <div class="card-title">事实修订方案（请先核对）</div>
        <div class="task-row"><strong>当前正式事实</strong><span>{{ planFactLabel(activeChange.plan.oldCanon) }}</span></div>
        <div class="task-row"><strong>准备改为</strong><span>{{ planFactLabel(activeChange.plan.replacementFact) }}</span></div>
        <div class="task-row"><strong>影响范围</strong><span>{{ changeScopeLabel(activeChange.plan.approvedScopeClass) }}</span></div>
        <div class="task-row"><strong>风险</strong><span>{{ activeChange.plan.riskClass === 'HIGH' ? '高风险' : activeChange.plan.riskClass === 'MEDIUM' ? '需要关注' : '较低风险' }}</span></div>
        <div class="task-row"><strong>执行前备份</strong><span>{{ activeChange.plan.requiresBackup ? '需要' : '不需要' }}</span></div>
        <div class="task-row"><strong>开篇 / 签约影响</strong><span>{{ activeChange.plan.signingImpact ? '需要重新核对' : '未发现' }}</span></div>
      </div>
      <p v-if="actionMessage" class="banner warning">{{ actionMessage }}</p>
      <div v-if="!formalWriteOutcomePending" class="actions" style="justify-content:flex-end;margin-top:14px"><button class="btn small" :disabled="activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED'" @click="continueImpact">继续分析影响</button><button class="btn small" :disabled="activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED'" @click="cancelImpact">取消分析</button><button class="btn small" :disabled="!activeChange.digest || activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED' || activeChange.expandedEdges < activeChange.totalEdges" @click="rejectChange">{{ readerProposalOnly ? '拒绝建议' : '拒绝方案' }}</button><button v-if="!readerProposalOnly" class="btn primary small" :disabled="!activeChange.digest || activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED' || activeChange.expandedEdges < activeChange.totalEdges" @click="approveAndApply">{{ activeChange.type === 'RETCON' && !activeChange.plan ? '生成事实修订方案' : activeChange.type === 'RETCON' ? '确认执行事实修订' : '确认并执行' }}</button></div>
      <p v-else class="banner warning">正式写入结果待确认；请只重新读取现有回执，系统不会再次提交。</p>
      <CommandFeedback :state="impactState" @confirm="store.confirmCommandResult('history-impact')" /><CommandFeedback :state="decisionState" @confirm="store.confirmCommandResult('history-decision')" /><CommandFeedback :state="applyState" @confirm="store.confirmCommandResult('history-apply')" />
      <CommandFeedback :state="retconPlanState" @confirm="store.confirmCommandResult('retcon-plan')" /><CommandFeedback :state="retconDecisionState" @confirm="store.confirmCommandResult('retcon-decision')" /><CommandFeedback :state="retconApplyState" @confirm="store.confirmCommandResult('retcon-apply')" />
    </section>

    <div v-if="history && !historyFilterLoading" style="display:grid;gap:10px;margin-top:14px">
      <article v-for="entry in history.entries.items" :key="`${entry.kind}:${entry.id}`" class="card pad">
        <div class="card-head">
          <div>
            <span class="badge">{{ entry.kind === 'VERSION' ? '版本' : entry.kind === 'CHANGE_REQUEST' ? '修改请求' : '事实修订' }}</span>
            <h3 style="margin:8px 0 4px">{{ entry.title }}</h3>
            <p class="muted">{{ entry.summary }}</p>
          </div>
          <span class="badge" :class="historyEntryStatusClass(entry)">{{ historyEntryStatusLabel(entry) }}</span>
        </div>
        <div class="banner info"><div><div class="banner-title">影响范围 · {{ changeScopeLabel(entry.scope) }}</div><div class="banner-copy">{{ changeImpactLabel(entry.impact) }}</div></div></div>
      </article>
    </div>

    <div v-if="history" class="section actions" style="justify-content:space-between">
      <span class="muted">已显示 {{ history.entries.items.length }} / {{ history.entries.total }} 条</span>
      <button v-if="history.entries.nextCursor" class="btn" :disabled="store.isWorkspaceLoading('history')" @click="loadMoreHistory">
        {{ store.isWorkspaceLoading('history') ? '正在加载…' : '加载更多历史记录' }}
      </button>
    </div>

    <BaseModal :open="!!modal" :title="modal === 'retcon' ? '建立事实修订提案' : '建立修改请求'" :subtitle="modal === 'retcon' ? '重大正式事实不会被普通保存覆盖，需补充事实修订方案。' : '先计算影响，再决定是否应用。'" @close="modal = null">
      <textarea v-model="requestText" class="textarea" placeholder="描述你希望改变的内容" />
      <template v-if="modal === 'retcon'">
        <div v-if="retconContextLoading" class="empty">正在读取当前正式事实和已有依据…</div>
        <label v-else class="field"><span class="field-label">要修订的当前事实</span><input class="input" :value="selectedFactLabel" list="b12-retcon-facts" placeholder="请选择当前正式事实" @change="chooseFact" /><datalist id="b12-retcon-facts"><option v-for="fact in retconContext?.facts.filter(item => item.current)" :key="fact.factId" :value="fact.label" /></datalist></label>
        <label class="field"><span class="field-label">新的事实内容</span><textarea v-model="retconReplacement" class="textarea" placeholder="填写你确认后的新事实" /></label>
        <div class="field"><span class="field-label">这条事实的原始依据</span><div class="chip-row" role="listbox" aria-label="这条事实的原始依据"><button v-for="(item, index) in retconEvidenceOptions" :key="evidenceSelector(item)" type="button" class="chip" :class="{ active: retconEvidenceRef === evidenceSelector(item) }" role="option" :aria-selected="retconEvidenceRef === evidenceSelector(item)" @click="retconEvidenceRef = evidenceSelector(item)">{{ evidenceDisplayLabel(item, index) }}</button><span v-if="!retconEvidenceOptions.length" class="help">暂无可用依据</span></div></div>
      </template>
      <CommandFeedback :state="state" />
      <template #footer><button class="btn" @click="modal = null">取消</button><button v-if="state.confirmation !== 'pending'" class="btn primary" :disabled="!requestText.trim() || (modal === 'retcon' && (!retconTargetRef.trim() || !retconReplacement.trim() || !retconEvidenceRef.trim()))" @click="create">开始影响分析</button></template>
    </BaseModal>
  </div>
</template>
