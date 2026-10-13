import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandRequest } from '../../src/application/contracts'

const harness = vi.hoisted(() => ({ port: null as any }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => harness.port }))

const pendingItems = [
  { changeRequestId: 'cr:fx12:user', responsibilityRef: 'thread:promise:door', summary: '申请撤销B级责任：门后的承诺', origin: 'USER_DELETE', createdAt: '2026-10-13T00:00:00Z' },
  { changeRequestId: 'cr:fx12:audit', responsibilityRef: 'thread:promise:bridge', summary: '审计发现既有连续性责任可能不再适用，等待用户确认。', origin: 'CHAPTER_AUDIT', createdAt: '2026-10-13T00:00:01Z' },
]
beforeEach(() => { setActivePinia(createPinia()); vi.resetModules() })

describe('FX-12 撤销请求 store', () => {
  it('Given 撤销请求 When 在 store 中决定 Then 提交 thread_withdraw_decide 并重新读取待确认列表', async () => {
    harness.port = {
      queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 3 }, recentChapters: [] })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
      queryPendingWithdrawRequests: vi.fn()
        .mockResolvedValueOnce({ projectId: 'p', projectRevision: 3, items: pendingItems, truncated: false })
        .mockResolvedValueOnce({ projectId: 'p', projectRevision: 4, items: pendingItems.slice(1), truncated: false }),
      queryHistory: vi.fn(async () => ({ entries: { items: [], nextCursor: null, total: 0 }, stats: { versions: 0, changes: 0, retcons: 0, pending: 0 } })),
      executeCommand: vi.fn(async (request: CommandRequest) => ({
        commandId: request.commandId, projectId: 'p', finalStatus: 'rejected', receiptId: 'r', message: '', acceptedAt: '',
      })),
    }
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    await store.loadWithdrawRequests()
    expect(store.withdrawRequests?.items).toHaveLength(2)

    await store.decideWithdrawRequest('cr:fx12:user', 'APPROVE')
    const [request] = harness.port.executeCommand.mock.calls[0]
    expect(request.type).toBe('thread_withdraw_decide')
    expect(request.payload).toEqual({ changeRequestId: 'cr:fx12:user', decision: 'APPROVE' })
    expect(harness.port.queryPendingWithdrawRequests).toHaveBeenCalledTimes(2)
    expect(store.withdrawRequests?.items).toHaveLength(1)
  })

})
