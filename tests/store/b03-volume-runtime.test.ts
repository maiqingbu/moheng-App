import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

const receipt = (request: CommandRequest): CommandReceipt => ({
  commandId: request.commandId ?? request.type,
  projectId: request.projectId,
  resultProjectId: request.projectId,
  finalStatus: 'committed',
  message: '已完成',
  acceptedAt: '2026-09-13T00:00:00Z',
  receiptId: `receipt:${request.commandId ?? request.type}`,
})

describe('B03 volume planning runtime guards', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetModules()
  })

  it.each([
    ['planning_create_volume_intent', 'volume-intent'],
    ['planning_create_volume_plan', 'volume-plan'],
  ] as const)('deduplicates %s for the same project and volume', async (type, key) => {
    let resolveCommand!: (value: CommandReceipt) => void
    const port = {
      queryProjectOverview: vi.fn(async (request: { projectId: string }) => ({
        project: { id: request.projectId, revision: 0, updatedAt: '' },
        pendingDecisionCount: 0,
        recentChapters: [],
        recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: { all: 0, active: 0, archived: 0, producing: 0, waitingDecision: 0, completed: 0, totalCost: 0 } })),
      executeCommand: vi.fn((request: CommandRequest) => new Promise<CommandReceipt>(resolve => {
        resolveCommand = resolve
      })),
    }
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('project-b03')
    const request: CommandRequest = { type, projectId: 'project-b03', payload: { volumeId: 'volume-b03-1' } }

    const first = store.runCommand(key, request)
    await Promise.resolve()
    const second = store.runCommand(key, request)

    expect(port.executeCommand).toHaveBeenCalledTimes(1)
    expect(store.commandState(key).status).toBe('processing')
    expect(store.commandState(key).message).toContain('正在进行')
    const sent = port.executeCommand.mock.calls[0][0] as CommandRequest
    resolveCommand(receipt(sent))
    await expect(first).resolves.toMatchObject({ finalStatus: 'committed' })
    await expect(second).resolves.toMatchObject({ finalStatus: 'committed' })
    expect(port.executeCommand).toHaveBeenCalledTimes(1)
  })
})
