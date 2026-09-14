import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'
import { createVolumePlanningRuntimePoller } from '../../src/ui/runtime/volumePlanningRuntimeSync'
import { executeVolumePlanningSequence } from '../../src/ui/runtime/volumePlanningOrchestration'

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
  });

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
  });

  it('orchestrates Store intent then plan with an independent runtime poller', async () => {
    const calls: string[] = []
    const port = {
      queryProjectOverview: vi.fn(async (request: { projectId: string }) => ({
        project: { id: request.projectId, revision: calls.length, updatedAt: '' },
        pendingDecisionCount: 0,
        recentChapters: [],
        recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: { all: 0, active: 0, archived: 0, producing: 0, waitingDecision: 0, completed: 0, totalCost: 0 } })),
      executeCommand: vi.fn(async (request: CommandRequest) => {
        calls.push(request.type)
        return receipt(request)
      }),
      queryPlanRoute: vi.fn(async () => ({
        projectId: 'project-b03', projectRevision: 2, volumeId: 'volume-b03-1',
        volumeIntentRef: { artifactId: 'intent', version: 1, payloadHash: 'a'.repeat(64) },
        volumePlanRef: { artifactId: 'plan', version: 1, payloadHash: 'b'.repeat(64) },
        routeVersion: 1, tasks: [1, 2, 3, 4, 5].map(index => ({ taskId: `task:${index}` })),
        taskReality: [], planningPressure: {}, truncated: false,
      })),
    }
    holder.port = port
    const starts: string[] = []
    const stops: string[] = []
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('project-b03')
    await executeVolumePlanningSequence({
      startIntent: () => { starts.push('intent') },
      stopIntent: () => { stops.push('intent') },
      startPlan: () => { starts.push('plan') },
      stopPlan: () => { stops.push('plan') },
      runIntent: () => store.runCommand('volume-intent', { type: 'planning_create_volume_intent', projectId: 'project-b03', payload: { volumeId: 'volume-b03-1' } }),
      runPlan: () => store.runCommand('volume-plan', { type: 'planning_create_volume_plan', projectId: 'project-b03', payload: { volumeId: 'volume-b03-1' } }),
    })
    const route = await store.loadPlanningRoute('volume-b03-1')
    expect(calls).toEqual(['planning_create_volume_intent', 'planning_create_volume_plan'])
    expect(starts).toEqual(['intent', 'plan'])
    expect(stops).toEqual(['intent', 'plan'])
    expect(route?.tasks).toHaveLength(5)
  })

  it('observes Intent terminal then Plan active without a single-poller gap', async () => {
    const base = { volume: { id: 'volume-b03-1', number: 1, title: '卷', status: 'PLANNING', chapterStart: 1, chapterEnd: 12, committedChapters: 0, plannedChapters: 12, wordCount: 0, targetWords: 12000, progress: 0, goal: '', health: 'healthy', updatedAt: '' }, planVersion: '未生成', contractVersion: 'VolumeContract', stateLedgerVersion: '未生成', arcs: [], chapters: { items: [], nextCursor: null, total: 0 }, openObligations: 0, overdueForeshadows: 0, exitEligibility: { ready: false, blockers: [] }, nextVolumeEligibility: '' } as const
    const observed: string[] = []
    let intentIndex = 0
    const intentFrames = [
      { jobId: 'intent-job', kind: 'B03::VOLUME_INTENT', phase: 'BUILD_VOLUME_GOAL', status: 'RUNNING', active: true },
      { jobId: 'intent-job', kind: 'B03::VOLUME_INTENT', phase: 'COMPLETED', status: 'COMPLETED', active: false },
    ]
    const intentPoller = createVolumePlanningRuntimePoller({
      intervalMs: 100,
      query: async () => ({ ...base, runtimeOperation: intentFrames[Math.min(intentIndex++, intentFrames.length - 1)] }),
      onProjection: projection => { observed.push(projection.runtimeOperation?.kind ?? '') },
    })
    intentPoller.start({ waitForActive: true })
    await new Promise(resolve => setTimeout(resolve, 230))
    intentPoller.stop()
    let planIndex = 0
    const planFrames = [
      { jobId: 'plan-job', kind: 'B03::VOLUME_PLAN', phase: 'BUILD_TASK_ROUTE', status: 'RUNNING', active: true },
      { jobId: 'plan-job', kind: 'B03::VOLUME_PLAN', phase: 'COMPLETED', status: 'COMPLETED', active: false },
    ]
    const planPoller = createVolumePlanningRuntimePoller({
      intervalMs: 100,
      query: async () => ({ ...base, runtimeOperation: planFrames[Math.min(planIndex++, planFrames.length - 1)] }),
      onProjection: projection => { observed.push(projection.runtimeOperation?.kind ?? '') },
    })
    planPoller.start({ waitForActive: true })
    await new Promise(resolve => setTimeout(resolve, 230))
    planPoller.stop()
    expect(observed).toContain('B03::VOLUME_INTENT')
    expect(observed).toContain('B03::VOLUME_PLAN')
  })
})
