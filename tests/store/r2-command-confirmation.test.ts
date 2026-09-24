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
