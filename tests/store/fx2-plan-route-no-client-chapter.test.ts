/* @vitest-environment happy-dom */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick } from 'vue'
import { createPinia, setActivePinia } from 'pinia'

const harness = vi.hoisted(() => ({
  route: { params: { volumeId: 'volume:fx2:2' } },
  port: null as any,
}))

vi.mock('vue-router', () => ({ useRoute: () => harness.route }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => harness.port }))

describe('FX-2 planning route requests', () => {
  let app: ReturnType<typeof createApp> | null = null
  let pinia: ReturnType<typeof createPinia>

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.resetModules()
  })

  afterEach(() => {
    app?.unmount()
    app = null
    document.body.innerHTML = ''
    harness.port = null
  })

  it('fx2-plan-route-no-client-chapter sends no chapter number and refresh preserves the route', async () => {
    const routeProjection = {
      projectId: 'project:fx2',
      projectRevision: 9,
      volumeId: 'volume:fx2:2',
      volumeIntentRef: { artifactId: 'intent:fx2:2', version: 1, payloadHash: 'a'.repeat(64) },
      volumePlanRef: { artifactId: 'plan:fx2:2', version: 1, payloadHash: 'b'.repeat(64) },
      routeVersion: 1,
      tasks: [],
      taskReality: [],
      planningPressure: { remainingChapterCapacity: 7, overdueThreadPressure: 0 },
      routeInvariants: [],
      requiredExitConditions: [],
      capacityBudget: { maxChapters: 10 },
      goalContractRefs: [],
      sourceEvidenceRefs: [],
      continuityThreads: [],
      futureReservations: [],
      currentChapterPlan: { artifactId: 'chapter-plan:fx2:14', version: 1, payloadHash: 'c'.repeat(64) },
      horizonPreview: [],
      truncated: false,
    }
    const volumeDetail = {
      volume: {
        id: 'volume:fx2:2', number: 2, title: '第二卷', status: 'PRODUCING',
        chapterStart: 11, chapterEnd: 20, committedChapters: 3, plannedChapters: 10,
        wordCount: 300, targetWords: 10000, progress: 0.3, goal: '推进目标', health: 'healthy', updatedAt: '',
      },
      planVersion: 'v1', contractVersion: 'VolumeContract', stateLedgerVersion: 'v1', arcs: [],
      chapters: {
        items: [{ id: 'chapter:fx2:14', number: 14, title: '第14章', volumeId: 'volume:fx2:2', status: 'producing', version: '', wordCount: 0, updatedAt: '' }],
        nextCursor: null, total: 1,
      },
      openObligations: 0, overdueForeshadows: 0, exitEligibility: { ready: false, blockers: [] },
      nextVolumeEligibility: '', runtimeOperation: null,
    }
    const requests: Array<Record<string, unknown>> = []
    harness.port = {
      queryProjectOverview: vi.fn(async () => ({
        project: { id: 'project:fx2', revision: 9, updatedAt: '' },
        pendingDecisionCount: 0, recentChapters: [], recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryProjectLibrary: vi.fn(async () => ({
        projects: [], totals: { all: 0, active: 0, archived: 0, producing: 0, waitingDecision: 0, completed: 0, totalCost: 0 },
      })),
      queryVolumeDetail: vi.fn(async () => volumeDetail),
      queryVolumeLifecycle: vi.fn(async () => null),
      queryPlanRoute: vi.fn(async (request: Record<string, unknown>) => {
        requests.push(request)
        return routeProjection
      }),
      executeCommand: vi.fn(async (request: Record<string, unknown>) => ({
        commandId: request.commandId,
        projectId: request.projectId,
        idempotencyKey: request.idempotencyKey,
        receiptId: 'receipt:fx2-refresh',
        finalStatus: 'committed',
        message: '已完成',
        acceptedAt: '2026-09-21T00:00:00Z',
      })),
    }

    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    store.currentProjectId = 'project:fx2'
    store.overview = {
      project: { id: 'project:fx2', revision: 9, updatedAt: '' },
      pendingDecisionCount: 0, recentChapters: [], recentArtifacts: [],
      healthSummary: { level: 'healthy', label: '', note: '' },
    } as any
    const { default: VolumeWorkspace } = await import('../../src/ui/pages/VolumeWorkspace.vue')
    const container = document.createElement('div')
    document.body.appendChild(container)
    app = createApp(VolumeWorkspace)
    app.use(pinia)
    app.mount(container)

    await new Promise(resolve => setTimeout(resolve, 0))
    await nextTick()
    expect(requests).toHaveLength(1)
    expect(requests[0]).not.toHaveProperty('currentChapter')
    const firstRoute = JSON.parse(JSON.stringify(store.planningRoute))

    // A committed command runs the store's refreshLoadedWorkspaces path.
    await store.runCommand('fx2-refresh', {
      type: 'planning_prepare_next_chapter',
      projectId: 'project:fx2',
      payload: { volumeId: 'volume:fx2:2' },
    })
    await nextTick()
    expect(requests).toHaveLength(2)
    expect(requests[1]).not.toHaveProperty('currentChapter')
    expect(store.planningRoute).toEqual(firstRoute)
  })
})
