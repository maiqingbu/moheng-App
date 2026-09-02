import type { CommandReceipt, CommandRequest } from '../../application/contracts'

export interface RetconFactOption {
  factId: string
  label: string
  current: boolean
}

export interface RetconEvidenceOption {
  artifactId: string
  version: number
  payloadHash: string
  label: string
}

export interface RetconContext {
  facts: RetconFactOption[]
  evidence: RetconEvidenceOption[]
}

export interface RetconDraft {
  projectId: string
  changeRequestId: string
  summary: string
  targetFactId: string
  replacement: string
  evidenceRef: RetconEvidenceOption
  impactDigest?: string
}

export interface RetconPlanView {
  retconPlanId: string
  planDigest: string
  impactDigest: string
  approvedScopeClass?: string
  riskClass?: string
  requiresBackup?: boolean
  replacementDigest?: string
  targetAuthorityDigest?: string
  oldCanon?: Record<string, unknown>
  replacementFact?: Record<string, unknown>
  evidenceRefs?: Array<Record<string, unknown>>
  signingImpact?: boolean
}

type ExecuteCommand = (key: string, request: CommandRequest) => Promise<CommandReceipt>

function resultOf(receipt: CommandReceipt): Record<string, unknown> {
  return receipt.result && typeof receipt.result === 'object' ? receipt.result : {}
}

export async function createRetconProposal(
  execute: ExecuteCommand,
  draft: RetconDraft,
): Promise<{ draft: RetconDraft; impactDigest: string; preflight: Record<string, unknown> }> {
  await execute('retcon-proposal', {
    type: 'change_request_create',
    projectId: draft.projectId,
    payload: {
      changeRequestId: draft.changeRequestId,
      changeType: 'RETCON',
      scope: 'PROJECT',
      summary: draft.summary,
      reason: draft.summary,
      targetFactId: draft.targetFactId,
      targetRefs: [draft.targetFactId],
      sourceRefs: [draft.targetFactId],
      requestedChange: {
        description: draft.summary,
        replacement: { objectJson: { description: draft.replacement } },
        evidenceRef: draft.evidenceRef,
      },
    },
  })
  const preflight = await execute('retcon-impact', {
    type: 'impact_preflight',
    projectId: draft.projectId,
    payload: { changeRequestId: draft.changeRequestId },
  })
  const value = resultOf(preflight)
  const impactDigest = typeof value.impactDigest === 'string' ? value.impactDigest : draft.impactDigest ?? ''
  return { draft: { ...draft, impactDigest }, impactDigest, preflight: value }
}

export async function createRetconPlan(
  execute: ExecuteCommand,
  draft: RetconDraft,
): Promise<RetconPlanView> {
  if (!draft.impactDigest) throw new Error('事实修订方案需要先完成影响分析。')
  const receipt = await execute('retcon-plan', {
    type: 'retcon_plan_create',
    projectId: draft.projectId,
    payload: {
      changeRequestId: draft.changeRequestId,
      retconPlanId: `retcon-plan:${draft.changeRequestId}`,
      targetFactId: draft.targetFactId,
      reason: draft.summary,
      fixType: 'REWRITE',
      replacement: { objectJson: { description: draft.replacement } },
      evidenceRefs: [draft.evidenceRef],
    },
  })
  const value = resultOf(receipt)
  const planDigest = typeof value.planDigest === 'string' ? value.planDigest : ''
  if (!planDigest) throw new Error('事实修订方案未能完成核对。')
  return {
    retconPlanId: String(value.retconPlanId ?? ''),
    planDigest,
    impactDigest: draft.impactDigest,
    approvedScopeClass: typeof value.approvedScopeClass === 'string' ? value.approvedScopeClass : undefined,
    riskClass: typeof value.riskClass === 'string' ? value.riskClass : undefined,
    requiresBackup: typeof value.requiresBackup === 'boolean' ? value.requiresBackup : undefined,
    replacementDigest: typeof value.replacementDigest === 'string' ? value.replacementDigest : undefined,
    targetAuthorityDigest: typeof value.targetAuthorityDigest === 'string' ? value.targetAuthorityDigest : undefined,
    oldCanon: value.oldCanon && typeof value.oldCanon === 'object' ? value.oldCanon as Record<string, unknown> : undefined,
    replacementFact: value.replacement && typeof value.replacement === 'object' ? value.replacement as Record<string, unknown> : undefined,
    evidenceRefs: Array.isArray(value.evidenceRefs) ? value.evidenceRefs as Array<Record<string, unknown>> : undefined,
    signingImpact: typeof value.signingImpact === 'boolean' ? value.signingImpact : undefined,
  }
}

export async function confirmRetconPlan(
  execute: ExecuteCommand,
  plan: RetconDraft & RetconPlanView,
): Promise<CommandReceipt> {
  await execute('retcon-decision', {
    type: 'change_request_decide',
    projectId: plan.projectId,
    payload: {
      changeRequestId: plan.changeRequestId,
      retconPlanId: plan.retconPlanId,
      planDigest: plan.planDigest,
      decision: 'APPROVE',
      impactDigest: plan.impactDigest,
      approvedScopeClass: plan.approvedScopeClass,
      approvedRiskClass: plan.riskClass,
      requiresBackup: plan.requiresBackup,
      replacementDigest: plan.replacementDigest,
      targetAuthorityDigest: plan.targetAuthorityDigest,
      scopeAcknowledged: true,
      riskAcknowledged: true,
    },
  })
  return execute('retcon-apply', {
    type: 'retcon_apply',
    projectId: plan.projectId,
    payload: {
      changeRequestId: plan.changeRequestId,
      retconPlanId: plan.retconPlanId,
      planDigest: plan.planDigest,
      impactDigest: plan.impactDigest,
    },
  })
}

export function selectGroundedRetconContext(context: RetconContext, _question: string): {
  fact: RetconFactOption
  evidence: RetconEvidenceOption
} {
  const fact = context.facts.find(item => item.current)
  const evidence = context.evidence[0]
  if (!fact) throw new Error('当前作品没有可供修订的正式事实。')
  if (!evidence) throw new Error('当前作品没有可供核验的已有依据。')
  return { fact, evidence }
}

function replacementFromQuestion(question: string): string {
  return question.match(/改成\s*([^。！？!?]+)/)?.[1]?.trim() || question.trim()
}

export async function createGroundedAssistantRetcon(
  dependencies: {
    execute: ExecuteCommand
    queryRetconContext: (request: Record<string, unknown>) => Promise<RetconContext>
  },
  projectId: string,
  question: string,
): Promise<RetconDraft & { fact: RetconFactOption; evidence: RetconEvidenceOption; preflight: Record<string, unknown> }> {
  const context = await dependencies.queryRetconContext({
    projectId,
    scope: 'assistant-retcon',
    cursor: null,
    pageSize: 20,
    query: question,
  })
  const selected = selectGroundedRetconContext(context, question)
  const draft: RetconDraft = {
    projectId,
    changeRequestId: `change:${projectId}:${Date.now()}`,
    summary: question.trim(),
    targetFactId: selected.fact.factId,
    replacement: replacementFromQuestion(question),
    evidenceRef: selected.evidence,
  }
  const proposal = await createRetconProposal(dependencies.execute, draft)
  return { ...proposal.draft, fact: selected.fact, evidence: selected.evidence, preflight: proposal.preflight }
}
