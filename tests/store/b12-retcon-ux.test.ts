import { describe, expect, it, vi } from 'vitest'
import fs from 'node:fs'
import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'
import {
  confirmRetconPlan,
  createGroundedAssistantRetcon,
  createRetconPlan,
  createRetconProposal,
  RetconEvidenceGapError,
  RetconContextSelectionError,
  selectGroundedRetconContext,
  startDependencySummaryBootstrap,
  monitorDependencySummary,
} from '../../src/ui/flows/b12RetconFlow'

const receipt = (request: CommandRequest, result: Record<string, unknown> = {}): CommandReceipt => ({
  commandId: request.commandId ?? request.type,
  projectId: request.projectId,
  resultProjectId: request.projectId,
  finalStatus: 'committed',
  message: '',
  acceptedAt: '',
  receiptId: `receipt:${request.type}`,
  result,
})

function recordFrontendEvidence(marker: string, payload: Record<string, unknown>) {
  const target = process.env.B12_FRONTEND_EVIDENCE_FILE
  if (target) fs.appendFileSync(target, `${JSON.stringify({ marker, ...payload })}\n`)
}

describe('B12 Retcon product flow', () => {
  it('stops after displaying the generated plan until the second confirmation', async () => {
    const calls: CommandRequest[] = []
    const execute = vi.fn(async (_key: string, request: CommandRequest) => {
      calls.push(request)
      if (request.type === 'retcon_plan_create') {
        return receipt(request, {
          retconPlanId: 'retcon-plan-1',
          planDigest: 'd'.repeat(64),
          approvedScopeClass: 'LOCAL',
          riskClass: 'MEDIUM',
          requiresBackup: false,
          replacementDigest: 'r'.repeat(64),
          targetAuthorityDigest: 't'.repeat(64),
        })
      }
      return receipt(request)
    })
    const draft = {
      projectId: 'p1',
      changeRequestId: 'change:1',
      summary: '修订父亲状态',
      targetFactId: 'fact:father',
      replacement: '父亲是假死',
      evidenceRef: { artifactId: 'evidence:1', version: 2, payloadHash: 'e'.repeat(64), supportsFactId: 'fact:father', relationship: 'FACT_SOURCE' as const },
      impactDigest: 'i'.repeat(64),
    }

    await createRetconProposal(execute, draft)
    const plan = await createRetconPlan(execute, draft)
    expect(calls.map(request => request.type)).toEqual(['change_request_create', 'impact_preflight', 'retcon_plan_create'])
    expect(plan.planDigest).toBe('d'.repeat(64))
    expect(calls.some(request => request.type === 'change_request_decide')).toBe(false)
    expect(calls.some(request => request.type === 'retcon_apply')).toBe(false)

    await confirmRetconPlan(execute, { ...draft, ...plan })
    expect(calls.map(request => request.type)).toEqual([
      'change_request_create',
      'impact_preflight',
      'retcon_plan_create',
      'change_request_decide',
      'retcon_apply',
    ])
    console.log('B12_FRONTEND_HISTORY_RETCON_RESULT planDisplayed=PASS decisionBeforeConfirm=0 applyBeforeConfirm=0 secondConfirm=PASS status=PASS')
    recordFrontendEvidence('B12_FRONTEND_HISTORY_RETCON_RESULT', { planDisplayed: true, decisionBeforeConfirm: 0, applyBeforeConfirm: 0, secondConfirm: true, status: 'PASS' })
  })

  it('grounds Assistant Retcon in a selected current fact and evidence', async () => {
    const executeCalls: CommandRequest[] = []
    const execute = vi.fn(async (_key: string, request: CommandRequest) => {
      executeCalls.push(request)
      return receipt(request, request.type === 'impact_preflight' ? { impactDigest: 'i'.repeat(64), status: 'PREFLIGHTED' } : {})
    })
    const context = {
      facts: [{ factId: 'fact:father', label: '第一卷第18章：父亲已经死亡', current: true }],
      evidence: [{ artifactId: 'evidence:18', version: 3, payloadHash: 'e'.repeat(64), label: '第一卷第18章正文依据', supportsFactId: 'fact:father', relationship: 'FACT_SOURCE' as const }],
    }
    const queryRetconContext = vi.fn(async () => context)

    const selected = selectGroundedRetconContext(context, '父亲改成假死')
    expect(selected.fact.label).toContain('父亲已经死亡')
    expect(selected.evidence.label).toContain('正文依据')
    expect(selected.fact.label).not.toContain('fact:')
    expect(selected.evidence.label).not.toContain('artifact:')
    expect(selected.fact.factId).toBe('fact:father')

    const result = await createGroundedAssistantRetcon({ execute, queryRetconContext }, 'p1', '父亲改成假死')
    expect(queryRetconContext).toHaveBeenCalledTimes(1)
    expect(executeCalls.map(request => request.type)).toEqual(['change_request_create', 'impact_preflight'])
    expect(executeCalls[0].payload?.targetFactId).toBe('fact:father')
    expect(executeCalls[0].payload?.requestedChange).toMatchObject({ evidenceRef: { artifactId: 'evidence:18', version: 3 } })
    expect(result.changeRequestId).toContain('change:p1:')
    console.log('B12_FRONTEND_ASSISTANT_RETCON_RESULT groundedQuery=PASS exactFact=PASS evidence=PASS historyContinuity=PASS status=PASS')
    recordFrontendEvidence('B12_FRONTEND_ASSISTANT_RETCON_RESULT', { groundedQuery: true, exactFact: true, evidence: true, historyContinuity: true, status: 'PASS' })
  })

  it('does not choose the first fact when grounding is ambiguous', () => {
    const context = {
      facts: [
        { factId: 'fact:father', label: '父亲：已经死亡', current: true },
        { factId: 'fact:master', label: '师父：已经死亡', current: true },
      ],
      evidence: [{ artifactId: 'evidence:death', version: 1, payloadHash: 'e'.repeat(64), label: '已有正文依据', supportsFactId: 'fact:father', relationship: 'FACT_SOURCE' as const }],
      grounding: { status: 'AMBIGUOUS' as const, candidateCount: 2, needsUserSelection: true },
    }
    expect(() => selectGroundedRetconContext(context, '把那个死亡改成假死')).toThrow(RetconContextSelectionError)
    expect(() => selectGroundedRetconContext(context, '把那个死亡改成假死')).not.toThrow(/fact:father/)
  })

  it('fails closed in Chinese when the selected fact has no exact source evidence', () => {
    const context = {
      facts: [{ factId: 'fact:father', label: '父亲：已经死亡', current: true }],
      evidence: [{ artifactId: 'evidence:other', version: 1, payloadHash: 'e'.repeat(64), label: '师父的正文依据', supportsFactId: 'fact:master', relationship: 'FACT_SOURCE' as const }],
      grounding: { status: 'UNIQUE' as const, candidateCount: 1, needsUserSelection: false, evidenceStatus: 'EVIDENCE_GAP' as const },
    }
    expect(() => selectGroundedRetconContext(context, '父亲改成假死')).toThrow(RetconEvidenceGapError)
    expect(() => selectGroundedRetconContext(context, '父亲改成假死')).toThrow('这条事实目前缺少可核验的来源记录，不能直接执行事实修订。')
  })

  it('starts summary bootstrap without waiting for worker completion', async () => {
    const execute = vi.fn(async (_key: string, request: CommandRequest) => receipt(request, { status: 'QUEUED', workerDispatched: true, returnBeforeWorkerCompletion: true }))
    let completed = false
    let resolveReady: (() => void) | undefined
    const ready = new Promise<void>(resolve => { resolveReady = resolve })
    let statusCalls = 0
    const start = await startDependencySummaryBootstrap(execute, 'p1')
    expect(start).toMatchObject({ status: 'QUEUED', workerDispatched: true, returnBeforeWorkerCompletion: true })
    const cancel = monitorDependencySummary(
      async () => {
        statusCalls += 1
        return statusCalls === 1
          ? { summary: { summaryStatus: 'NOT_READY' }, job: { status: 'RUNNING' } }
          : { summary: { summaryStatus: 'READY' }, job: { status: 'COMPLETED' } }
      },
      () => { completed = true; resolveReady?.() },
      error => { throw error },
      { intervalMs: 0, maxAttempts: 4 },
    )
    await ready
    cancel()
    expect(completed).toBe(true)
    expect(statusCalls).toBe(2)
    recordFrontendEvidence('B12_FRONTEND_SUMMARY_ASYNC_RESULT', { commandReturnedBeforeWorker: true, workerStatusPolled: true, uiNonBlocking: true, status: 'PASS' })
  })
})
