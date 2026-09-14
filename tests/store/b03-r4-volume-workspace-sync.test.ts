/* @vitest-environment happy-dom */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, reactive } from 'vue'
import VolumeWorkspace from '../../src/ui/pages/VolumeWorkspace.vue'

const harness = vi.hoisted(() => ({
  route: { params: { volumeId: 'volume-r4' } },
  store: null as any,
}))

vi.mock('vue-router', () => ({
  useRoute: () => harness.route,
}))

vi.mock('../../src/ui/stores/projectStore', () => ({
  useProjectStore: () => harness.store,
}))

const settle = async () => {
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

const projection = (operation: Record<string, unknown>) => ({
  volume: {
    id: 'volume-r4', number: 1, title: 'R4 测试卷', status: 'PLANNING',
    chapterStart: 1, chapterEnd: 12, committedChapters: 0, plannedChapters: 12,
    wordCount: 0, targetWords: 12000, progress: 0, goal: '验证导航态同步',
    health: 'healthy', updatedAt: '',
  },
  planVersion: '未生成', contractVersion: 'VolumeContract', stateLedgerVersion: '未生成',
  arcs: [], chapters: { items: [], nextCursor: null, total: 0 }, openObligations: 0,
  overdueForeshadows: 0, exitEligibility: { ready: false, blockers: [] },
  nextVolumeEligibility: '', runtimeOperation: operation,
})

const operation = (overrides: Record<string, unknown> = {}) => ({
  jobId: 'job:b03:r4', kind: 'B03::VOLUME_PLAN', phase: 'BUILD_TASK_ROUTE',
  status: 'RUNNING', active: true,
  steps: [{ id: 'phase', label: 'BUILD_TASK_ROUTE', status: 'active' }],
  ...overrides,
})

const mountWorkspace = (frames: Array<Record<string, unknown>>) => {
  const queried: Array<Record<string, unknown>> = []
  let pollIndex = 1
  const store = reactive({
    currentProjectId: 'project-r4',
    volumeDetail: null as any,
    planningRoute: null,
    volumeLifecycle: null,
    advancedMode: false,
    commandState: vi.fn(() => ({ status: 'idle', message: '' })),
    runCommand: vi.fn(),
    loadPlanningRoute: vi.fn(async () => null),
    loadVolumeLifecycle: vi.fn(async () => null),
    loadVolumeDetail: vi.fn(async (_volumeId: string, force = false) => {
      const next = frames[force ? Math.min(pollIndex++, frames.length - 1) : 0]
      queried.push(next)
      store.volumeDetail = next
      return next
    }),
  })
  harness.store = store
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(VolumeWorkspace)
  app.mount(container)
  return { app, container, store, queried }
}

const advancePoll = async () => {
  await vi.advanceTimersByTimeAsync(750)
  await settle()
}

describe('B03-R4 VolumeWorkspace runtime synchronization', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    document.body.innerHTML = ''
  })

  it.each([
    ['Intent', 'B03::VOLUME_INTENT'],
    ['Plan', 'B03::VOLUME_PLAN'],
  ])('starts a read-only poller when entering during active %s', async (_label, kind) => {
    const mounted = mountWorkspace([
      projection(operation({ kind, phase: kind === 'B03::VOLUME_INTENT' ? 'BUILD_VOLUME_GOAL' : 'BUILD_TASK_ROUTE' })),
      projection(operation({ kind, status: 'COMPLETED', phase: 'COMPLETED', active: false, steps: [] })),
    ])
    await settle()

    expect(mounted.store.loadVolumeDetail).toHaveBeenCalledWith('volume-r4')
    expect(mounted.store.loadVolumeDetail).not.toHaveBeenCalledWith('volume-r4', true)
    expect(vi.getTimerCount()).toBe(1)
    expect(mounted.store.runCommand).not.toHaveBeenCalled()
    mounted.app.unmount()
  })

  it('observes every backend phase after navigation without restarting the mutation', async () => {
    const phases = ['BUILD_TASK_ROUTE', 'VALIDATE_DEPENDENCIES', 'CALCULATE_CAPACITY', 'VALIDATE_ROUTE', 'COMMIT_VOLUME_PLAN', 'COMPLETED']
    const mounted = mountWorkspace(phases.map((phase, index) => projection(operation({
      phase,
      status: phase === 'COMPLETED' ? 'COMPLETED' : 'RUNNING',
      active: phase !== 'COMPLETED',
      steps: [{ id: phase, label: phase, status: phase === 'COMPLETED' ? 'done' : 'active' }],
    }))))
    await settle()

    expect(mounted.container.textContent).toContain('BUILD_TASK_ROUTE')
    for (const phase of phases.slice(1)) {
      await advancePoll()
      expect(mounted.store.volumeDetail.runtimeOperation.phase).toBe(phase)
      expect(mounted.container.textContent).toContain(phase)
    }
    expect(mounted.store.runCommand).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
    const queryCount = mounted.store.loadVolumeDetail.mock.calls.length
    await advancePoll()
    expect(mounted.store.loadVolumeDetail).toHaveBeenCalledTimes(queryCount)
    mounted.app.unmount()
  })

  it('stops polling and removes the active animation after COMPLETED', async () => {
    const mounted = mountWorkspace([
      projection(operation()),
      projection(operation({ phase: 'COMPLETED', status: 'COMPLETED', active: false, steps: [{ id: 'done', label: '完成', status: 'done' }] })),
    ])
    await settle()
    await advancePoll()

    expect(mounted.container.querySelectorAll('.timeline-item.active')).toHaveLength(0)
    expect(mounted.container.querySelectorAll('.timeline-item.done')).toHaveLength(1)
    expect(vi.getTimerCount()).toBe(0)
    mounted.app.unmount()
  })

  it('renders FAILED with no active item and stops polling', async () => {
    const mounted = mountWorkspace([
      projection(operation()),
      projection(operation({ phase: 'VALIDATE_ROUTE', status: 'FAILED', active: false, steps: [
        { id: 'failed', label: '路线校验失败', status: 'failed' },
      ] })),
    ])
    await settle()
    await advancePoll()

    expect(mounted.container.querySelectorAll('.timeline-item.failed')).toHaveLength(1)
    expect(mounted.container.querySelectorAll('.timeline-item.active')).toHaveLength(0)
    expect(vi.getTimerCount()).toBe(0)
    mounted.app.unmount()
  })

  it('cleans up the workspace poller on unmount without an interval leak', async () => {
    const mounted = mountWorkspace([
      projection(operation()),
      projection(operation({ phase: 'VALIDATE_DEPENDENCIES' })),
    ])
    await settle()
    expect(vi.getTimerCount()).toBe(1)
    const queryCount = mounted.store.loadVolumeDetail.mock.calls.length

    mounted.app.unmount()
    expect(vi.getTimerCount()).toBe(0)
    await advancePoll()
    expect(mounted.store.loadVolumeDetail).toHaveBeenCalledTimes(queryCount)
  })
})
