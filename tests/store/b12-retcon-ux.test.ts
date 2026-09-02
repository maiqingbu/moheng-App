import { describe, expect, it, vi } from 'vitest'
import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'
import {
  confirmRetconPlan,
  createGroundedAssistantRetcon,
  createRetconPlan,
  createRetconProposal,
  selectGroundedRetconContext,
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
      evidenceRef: { artifactId: 'evidence:1', version: 2, payloadHash: 'e'.repeat(64) },
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
  })

  it('grounds Assistant Retcon in a selected current fact and evidence', async () => {
    const executeCalls: CommandRequest[] = []
    const execute = vi.fn(async (_key: string, request: CommandRequest) => {
      executeCalls.push(request)
      return receipt(request, request.type === 'impact_preflight' ? { impactDigest: 'i'.repeat(64), status: 'PREFLIGHTED' } : {})
    })
    const context = {
      facts: [{ factId: 'fact:father', label: '第一卷第18章：父亲已经死亡', current: true }],
      evidence: [{ artifactId: 'evidence:18', version: 3, payloadHash: 'e'.repeat(64), label: '第一卷第18章正文依据' }],
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
  })
})
