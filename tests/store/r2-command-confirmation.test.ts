import { beforeEach, describe, expect, it, vi } from 'vitest'
import fs from 'node:fs'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandRequest } from '../../src/application/contracts'
const holder = vi.hoisted(() => ({ port: null as any }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => holder.port }))
beforeEach(() => { setActivePinia(createPinia()); vi.resetModules() })

describe('R2 正式回执后的查询失败', () => {
  it('Given 已提交回执 When 重新查询失败 Then 保留回执并标记结果待确认，不能重复发送命令', async () => {
    holder.port = {
      queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 1 }, recentChapters: [] })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
      queryProductionCenter: vi.fn(async () => ({ signing: { status: 'NOT_STARTED' } })),
      executeCommand: vi.fn(async (request: CommandRequest) => ({ commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'r', message: '', acceptedAt: '' })),
    }
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    await store.loadProduction()
    holder.port.queryProductionCenter.mockRejectedValue(new Error('read unavailable'))
    await expect(store.runCommand('produce', { type: 'production_start', payload: { chapterId: 'c' } })).resolves.toMatchObject({ receiptId: 'r' })
    expect(store.commandState('produce')).toMatchObject({ status: 'committed', confirmation: 'pending' })
    expect(store.commandState('produce').message).toContain('结果待确认')
    await expect(store.runCommand('produce', { type: 'production_start', payload: { chapterId: 'c' } })).rejects.toThrow('COMMAND_RESULT_UNCONFIRMED')
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(1)
    holder.port.queryProductionCenter.mockResolvedValue({ signing: { status: 'NOT_STARTED' } })
    await store.confirmCommandResult('produce')
    expect(store.commandState('produce')).toMatchObject({ status: 'committed', confirmation: 'confirmed' })
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(1)
  })

  it('Given ordinary Change commit response is lost When manually retried Then show pending and reuse the same operation identity', async () => {
    const requests: CommandRequest[] = []
    let loseFirstResponse = true
    holder.port = {
      queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 1 }, recentChapters: [] })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
      queryProductionCenter: vi.fn(async () => ({ signing: { status: 'NOT_STARTED' } })),
      executeCommand: vi.fn(async (request: CommandRequest) => {
        requests.push(request)
        if (request.type === 'change_request_apply' && loseFirstResponse) {
          loseFirstResponse = false
          throw new Error('simulated response lost after commit')
        }
        return {
          commandId: request.commandId,
          projectId: 'p',
          finalStatus: 'committed',
          receiptId: 'receipt:change:1',
          result: { status: 'REPLAYED' },
          postverify: { status: 'VERIFIED' },
          message: '',
          acceptedAt: '',
        }
      }),
    }
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    const request: CommandRequest = {
      type: 'change_request_apply',
      projectId: 'p',
      commandId: 'b12-change-apply:p:change:1',
      idempotencyKey: 'b12-change-apply:p:change:1',
      payload: { changeRequestId: 'change:1', impactDigest: 'digest:1' },
    }

    await expect(store.runCommand('history-apply', request)).rejects.toThrow('simulated response lost')
    expect(store.commandState('history-apply')).toMatchObject({ status: 'processing', confirmation: 'pending' })
    expect(store.commandState('history-apply').message).toContain('结果待确认')
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(1)

    const replay = await store.runCommand('history-apply', request)
    expect(replay.result).toEqual({ status: 'REPLAYED' })
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(2)
    expect(requests.map(item => item.idempotencyKey)).toEqual([
      'b12-change-apply:p:change:1',
      'b12-change-apply:p:change:1',
    ])
    const evidenceFile = process.env.B12_FRONTEND_EVIDENCE_FILE
    if (evidenceFile) {
      fs.appendFileSync(evidenceFile, `${JSON.stringify({
        marker: 'B12_ORDINARY_CHANGE_UNKNOWN_RESULT',
        unknownIsPending: true,
        noAutomaticResubmit: true,
        manualRetryUsesSameIdempotencyKey: requests[0].idempotencyKey === requests[1].idempotencyKey,
        replayReceiptReturned: replay.result?.status === 'REPLAYED',
        status: 'PASS',
      })}\n`)
    }
  })
})

describe('B-R2-02 H03 前端未知结果恢复', () => {
  const plan = {
    action: 'EXECUTABLE' as const,
    repairPlanId: 'repair-plan:p',
    basedOnProjectRevision: 2,
    operationDigest: 'digest:repair:p',
    diagnosis: { claim: '需要重新核对' },
    affectedScopes: ['PROJECT'],
    requiresBackup: false,
    riskLevel: 'LOW',
  }
  const durableProjection = (status: string) => ({
    repair: {
      repairPlanId: plan.repairPlanId,
      basedOnProjectRevision: 2,
      diagnosis: plan.diagnosis,
      findingRefs: [],
      targetAuthorityOwner: 'B12::ChangeRetconOwner',
      proposedOperations: [],
      affectedArtifacts: [],
      affectedScopes: ['PROJECT'],
      riskLevel: 'LOW',
      requiresBackup: false,
      expectedPostconditions: [],
      operationDigest: plan.operationDigest,
      status,
      authorizationId: 'assistant-authorization:repair-plan:p',
      consumedAt: '2026-09-30T12:00:00Z',
      executionReceipt: {
        receiptRef: 'repair-receipt:assistant-authorization:repair-plan:p',
        projectId: 'p',
        repairPlanId: plan.repairPlanId,
        authorizationId: 'assistant-authorization:repair-plan:p',
        beforeRevision: 2,
        afterRevision: 3,
        operationDigest: plan.operationDigest,
        owner: 'B12::ChangeRetconOwner',
        ownerReceiptRef: 'commit-receipt:p:change:1',
        executedAt: '2026-09-30T12:00:00Z',
        status,
        postconditions: [{ status: status === 'REPAIR_VERIFIED' ? 'PASS' : status }],
      },
    },
  })
  const openStore = async () => {
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    return store
  }
  const basePort = (projection: () => unknown, execute: (request: CommandRequest) => unknown) => ({
    queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 2 }, recentChapters: [] })),
    queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
    queryProductionCenter: vi.fn(async () => projection()),
    executeCommand: vi.fn(async (request: CommandRequest) => execute(request)),
  })

  it('Given repair_execute 已落库但 IPC 回执丢失 When 查询到匹配 H03 回执 Then 按回执恢复且不重复执行', async () => {
    const projection = durableProjection('REPAIR_VERIFIED')
    const requests: CommandRequest[] = []
    holder.port = basePort(() => projection, request => {
      requests.push(request)
      if (request.type === 'repair_authorize') {
        return { commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'auth-receipt', message: '', acceptedAt: '' }
      }
      throw new Error('simulated repair response lost after durable commit')
    })
    const store = await openStore()

    const result = await store.authorizeAssistantRepair(plan)

    expect(result).toMatchObject({ finalStatus: 'committed', receiptId: 'repair-receipt:assistant-authorization:repair-plan:p' })
    expect(result.receipt?.status).toBe('REPAIR_VERIFIED')
    expect(store.commandState('assistant-execute')).toMatchObject({ status: 'committed', confirmation: 'confirmed' })
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(1)
  })

  it('Given repair_execute 回执丢失且 durable query 暂不可用 When 手动重新读取 Then 保持待确认且不重发', async () => {
    let queryAvailable = false
    const projection = durableProjection('REPAIR_VERIFIED')
    const requests: CommandRequest[] = []
    holder.port = basePort(() => {
      if (!queryAvailable) throw new Error('simulated durable query unavailable')
      return projection
    }, request => {
      requests.push(request)
      if (request.type === 'repair_authorize') {
        return { commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'auth-receipt', message: '', acceptedAt: '' }
      }
      throw new Error('simulated repair response lost')
    })
    const store = await openStore()

    await expect(store.authorizeAssistantRepair(plan)).rejects.toThrow('simulated repair response lost')
    expect(store.commandState('assistant-execute')).toMatchObject({ status: 'processing', confirmation: 'pending' })
    expect(store.commandState('assistant-execute').message).toContain('结果待确认')
    await expect(store.runCommand('production-repair-execute', {
      type: 'repair_execute',
      projectId: 'p',
      payload: { repairPlanId: plan.repairPlanId, authorizationId: 'assistant-authorization:repair-plan:p' },
    })).rejects.toThrow('COMMAND_RESULT_UNCONFIRMED')
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(1)
    await store.confirmCommandResult('assistant-execute')
    expect(store.commandState('assistant-execute')).toMatchObject({ status: 'processing', confirmation: 'pending' })
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(1)

    queryAvailable = true
    await store.confirmCommandResult('assistant-execute')
    expect(store.commandState('assistant-execute')).toMatchObject({ status: 'committed', confirmation: 'confirmed', receiptId: 'repair-receipt:assistant-authorization:repair-plan:p' })
    expect(requests.filter(request => request.type === 'repair_execute')).toHaveLength(1)
  })

  it('Given reverify 已把原回执更新为 VERIFIED 但 IPC 回执丢失 When 恢复 Then 只读取原回执', async () => {
    const requests: CommandRequest[] = []
    holder.port = basePort(() => durableProjection('REPAIR_VERIFIED'), request => {
      requests.push(request)
      throw new Error('simulated reverify response lost after receipt update')
    })
    const store = await openStore()

    const result = await store.reverifyAssistantRepair(plan)

    expect(result.receipt?.status).toBe('REPAIR_VERIFIED')
    expect(store.commandState('assistant-reverify')).toMatchObject({ status: 'committed', confirmation: 'confirmed' })
    expect(requests).toHaveLength(1)
    expect(requests[0]).toMatchObject({
      type: 'repair_execute',
      payload: { repairPlanId: 'repair-plan:p', authorizationId: 'assistant-authorization:repair-plan:p' },
    })
  })
})
