import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { AssistantRepairPlan, CommandRequest } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as any }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => holder.port }))
beforeEach(() => { setActivePinia(createPinia()); vi.resetModules() })

describe('RV-19 repair request boundary', () => {
  it('sends only finding refs and operation kinds, then authorizes by plan id and digest', async () => {
    const queriedPlan: AssistantRepairPlan = {
      action: 'EXECUTABLE',
      repairPlanId: 'assistant-repair:fixture',
      projectId: 'p',
      basedOnProjectRevision: 0,
      diagnosis: { claim: '当前作品展示需要重新整理。' },
      findingRefs: ['assistant-finding:fixture'],
      targetAuthorityOwner: 'B04::ProjectionOwner',
      proposedOperations: [{ operation: 'rebuild_projection', target: '项目展示', params: {}, postconditions: {} }],
      affectedArtifacts: ['项目展示'],
      affectedScopes: ['当前作品'],
      riskLevel: 'LOW',
      requiresBackup: false,
      expectedPostconditions: [{ kind: 'projection_current', expected: true }],
      operationDigest: 'a'.repeat(64),
      createdAt: '2026-10-06T00:00:00Z',
    }
    const persistedPlan = { ...queriedPlan, diagnosis: { claim: '后端重建的当前诊断。' } }
    const requests: CommandRequest[] = []
    holder.port = {
      queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 0 }, recentChapters: [] })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
      queryAssistantRepairPlan: vi.fn(async () => queriedPlan),
      queryProductionCenter: vi.fn(async () => ({ signing: { status: 'NOT_STARTED' } })),
      executeCommand: vi.fn(async (request: CommandRequest) => {
        requests.push(request)
        if (request.type === 'repair_plan_create') {
          return { commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'plan-receipt', message: '', acceptedAt: '', result: { repairPlan: persistedPlan } }
        }
        if (request.type === 'repair_authorize') {
          return { commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'auth-receipt', message: '', acceptedAt: '' }
        }
        return { commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'execute-receipt', message: '', acceptedAt: '', receipt: { status: 'REPAIR_VERIFIED' } }
      }),
    }
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')

    const plan = await store.prepareAssistantRepair('修复当前作品展示')
    expect(plan).toEqual(persistedPlan)
    await store.authorizeAssistantRepair(plan)

    const planRequest = requests.find(request => request.type === 'repair_plan_create')!
    expect(planRequest.payload).toEqual({
      findingRefs: ['assistant-finding:fixture'],
      requestedOperationKinds: ['rebuild_projection'],
    })
    const authorizationRequest = requests.find(request => request.type === 'repair_authorize')!
    expect(authorizationRequest.payload).toEqual({
      repairPlanId: 'assistant-repair:fixture',
      operationDigest: 'a'.repeat(64),
    })
  })

  it('rereads a lost authorization response and continues without resubmitting authorization', async () => {
    const plan: AssistantRepairPlan = {
      action: 'EXECUTABLE',
      repairPlanId: 'assistant-repair:confirmed-auth',
      projectId: 'p',
      basedOnProjectRevision: 0,
      diagnosis: { claim: '当前展示需要重新整理。' },
      findingRefs: ['finding:confirmed-auth'],
      targetAuthorityOwner: 'B04::ProjectionOwner',
      proposedOperations: [{ operation: 'rebuild_projection', target: '项目展示', params: {}, postconditions: {} }],
      affectedArtifacts: ['项目展示'],
      affectedScopes: ['当前作品'],
      riskLevel: 'LOW',
      requiresBackup: false,
      expectedPostconditions: [{ kind: 'projection_current', expected: true }],
      operationDigest: 'b'.repeat(64),
      createdAt: '2026-10-06T00:00:00Z',
    }
    const requests: CommandRequest[] = []
    holder.port = {
      queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 0 }, recentChapters: [] })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
      queryProductionCenter: vi.fn(async () => ({ signing: { status: 'NOT_STARTED' } })),
      queryCommandReceipt: vi.fn(async ({ idempotencyKey }: { projectId: string; idempotencyKey: string }) => {
        const command = requests.find(request => request.type === 'repair_authorize')!
        return {
          commandId: command.commandId,
          projectId: 'p',
          idempotencyKey,
          finalStatus: 'committed' as const,
          receiptId: 'authorization-receipt',
          message: '授权已记录。',
          acceptedAt: '2026-10-06T00:00:00Z',
        }
      }),
      executeCommand: vi.fn(async (request: CommandRequest) => {
        requests.push(request)
        if (request.type === 'repair_authorize') throw new Error('transport response lost')
        return {
          commandId: request.commandId,
          projectId: 'p',
          finalStatus: 'committed' as const,
          receiptId: 'execution-receipt',
          message: '修复已验证。',
          acceptedAt: '2026-10-06T00:00:00Z',
          receipt: { status: 'REPAIR_VERIFIED' },
        }
      }),
    }
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')

    await expect(store.authorizeAssistantRepair(plan)).rejects.toThrow('transport response lost')
    expect(store.commandState('assistant-authorize')).toMatchObject({ status: 'processing', confirmation: 'pending' })
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(0)

    await store.confirmCommandResult('assistant-authorize')
    expect(store.commandState('assistant-authorize')).toMatchObject({ status: 'committed', confirmation: 'confirmed', receiptId: 'authorization-receipt' })
    await store.authorizeAssistantRepair(plan)
    expect(requests.filter(request => request.type === 'repair_authorize')).toHaveLength(1)
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(1)
  })
})
