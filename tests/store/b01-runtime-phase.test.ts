import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import type { CommandReceipt, CommandRequest, ProductionCenterProjection } from '../../src/application/contracts'
import { createProductionRuntimePoller } from '../../src/ui/runtime/productionRuntimeSync'

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

describe('B01 runtime phase synchronization', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    setActivePinia(createPinia())
    vi.resetModules()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('updates every backend phase and stops polling after COMPLETED', async () => {
    const phases = ['CONTEXT', 'PLAN', 'RENDER', 'AUDIT', 'GATE', 'COMMIT', 'RECONCILE', 'COMPLETED']
    let phaseIndex = 0
    const query = vi.fn(async () => {
      const productionStage = phases[phaseIndex++]
      return {
        jobStatus: productionStage === 'COMPLETED' ? 'COMPLETED' : 'RUNNING',
        productionStage,
      } as unknown as ProductionCenterProjection
    })
    const seen: string[] = []
    const poller = createProductionRuntimePoller({
      intervalMs: 100,
      query,
      onProjection: projection => seen.push(String(projection.productionStage)),
    })

    poller.start()
    await vi.advanceTimersByTimeAsync(800)

    expect(query).toHaveBeenCalledTimes(8)
    expect(seen).toEqual(phases)
    expect(poller.isRunning()).toBe(false)
    await vi.advanceTimersByTimeAsync(500)
    expect(query).toHaveBeenCalledTimes(8)
  })

  it('does not stop on a stale terminal snapshot before the new job becomes active', async () => {
    const snapshots = [
      { jobStatus: 'COMPLETED', productionStage: 'COMPLETED' },
      { jobStatus: 'RUNNING', productionStage: 'CONTEXT' },
      { jobStatus: 'COMPLETED', productionStage: 'COMPLETED' },
    ] as unknown as ProductionCenterProjection[]
    const query = vi.fn(async () => snapshots.shift() ?? snapshots.at(-1)!)
    const poller = createProductionRuntimePoller({ query, intervalMs: 100, onProjection: () => undefined })

    poller.start({ waitForActive: true })
    await vi.advanceTimersByTimeAsync(200)

    expect(query).toHaveBeenCalledTimes(3)
    expect(poller.isRunning()).toBe(false)
  })

  it('deduplicates a production_start mutation while the first command is in flight', async () => {
    let resolveCommand!: (value: CommandReceipt) => void
    const port = {
      queryProjectOverview: vi.fn(async (request: { projectId: string }) => ({
        project: { id: request.projectId, revision: 0, updatedAt: '' },
        pendingDecisionCount: 0,
        recentChapters: [],
        recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryProjectLibrary: vi.fn(async () => ({ items: [], nextCursor: null, total: 0 })),
      executeCommand: vi.fn((request: CommandRequest) => new Promise<CommandReceipt>(resolve => {
        resolveCommand = resolve
      })),
    }
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('project-b01')
    const request: CommandRequest = {
      type: 'production_start',
      projectId: 'project-b01',
      payload: { chapterId: 'chapter-b01-1', executionMode: 'OPENING_BOOTSTRAP' },
    }

    const first = store.runCommand('production-start', request)
    await Promise.resolve()
    const second = store.runCommand('production-start', request)

    expect(port.executeCommand).toHaveBeenCalledTimes(1)
    expect(store.commandState('production-start').status).toBe('processing')
    resolveCommand(receipt(port.executeCommand.mock.calls[0][0] as CommandRequest))
    await expect(first).resolves.toMatchObject({ finalStatus: 'committed' })
    await expect(second).resolves.toMatchObject({ finalStatus: 'committed' })
    expect(port.executeCommand).toHaveBeenCalledTimes(1)
  })

  it('keeps a running command alive across a route/project switch', async () => {
    let resolveCommand!: (value: CommandReceipt) => void
    const port = {
      queryProjectOverview: vi.fn(async (request: { projectId: string }) => ({
        project: { id: request.projectId, revision: 0, updatedAt: '' },
        pendingDecisionCount: 0,
        recentChapters: [],
        recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryProjectLibrary: vi.fn(async () => ({ items: [], nextCursor: null, total: 0 })),
      executeCommand: vi.fn((request: CommandRequest) => new Promise<CommandReceipt>(resolve => {
        resolveCommand = resolve
      })),
    }
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('project-a')
    const request: CommandRequest = {
      type: 'production_start',
      projectId: 'project-a',
      payload: { chapterId: 'chapter-a-1', executionMode: 'OPENING_BOOTSTRAP' },
    }
    const command = store.runCommand('production-start', request)
    await store.openProject('project-b')
    resolveCommand(receipt(port.executeCommand.mock.calls[0][0] as CommandRequest))
    await command

    expect(port.executeCommand).toHaveBeenCalledTimes(1)
    expect(store.currentProjectId).toBe('project-b')
  })
})
