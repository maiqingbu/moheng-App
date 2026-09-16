/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, reactive } from 'vue'
import VolumeWorkspace from '../../src/ui/pages/VolumeWorkspace.vue'

const harness = vi.hoisted(() => ({
  route: { params: { volumeId: 'volume-b04-b06' } },
  store: null as any,
}))

vi.mock('vue-router', () => ({ useRoute: () => harness.route }))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const detail = (chapterStatus: 'official' | 'producing' = 'official') => ({
  volume: {
    id: 'volume-b04-b06', number: 1, title: '真实闭环卷', status: 'READY',
    chapterStart: 1, chapterEnd: 36, committedChapters: chapterStatus === 'official' ? 3 : 3,
    plannedChapters: 36, wordCount: 0, targetWords: 120000, progress: 0, goal: '真实闭环', health: 'healthy', updatedAt: '',
  },
  planVersion: 'v1', contractVersion: 'VolumeContract', stateLedgerVersion: 'v1', arcs: [],
  chapters: {
    items: chapterStatus === 'official'
      ? [{ id: 'chapter:3', number: 3, title: '第3章', volumeId: 'volume-b04-b06', status: 'official', version: 'v1', wordCount: 100, updatedAt: '' }]
      : [{ id: 'chapter:4', number: 4, title: '第4章', volumeId: 'volume-b04-b06', status: 'producing', version: '', wordCount: 0, updatedAt: '' }],
    nextCursor: null, total: 1,
  },
  openObligations: 0, overdueForeshadows: 0, exitEligibility: { ready: false, blockers: [] },
  nextVolumeEligibility: '', runtimeOperation: null,
})

const routeProjection = (currentChapterPlan: unknown = null) => ({
  projectId: 'project-b04-b06', projectRevision: 3, volumeId: 'volume-b04-b06',
  volumeIntentRef: { artifactId: 'intent', version: 1, payloadHash: 'a'.repeat(64) },
  volumePlanRef: { artifactId: 'plan', version: 1, payloadHash: 'b'.repeat(64) }, routeVersion: 1,
  tasks: [], taskReality: [], planningPressure: {}, routeInvariants: [], requiredExitConditions: [],
  capacityBudget: {}, goalContractRefs: [], sourceEvidenceRefs: [], continuityThreads: [], futureReservations: [],
  currentChapterPlan, horizonPreview: [], truncated: false,
})

function mountWorkspace(chapterStatus: 'official' | 'producing', currentChapterPlan: unknown = null) {
  const store = reactive({
    currentProjectId: 'project-b04-b06', volumeDetail: detail(chapterStatus), planningRoute: routeProjection(currentChapterPlan),
    volumeLifecycle: null, advancedMode: false, commandState: vi.fn(() => ({ status: 'idle', message: '' })),
    runCommand: vi.fn(async () => ({ finalStatus: 'committed', message: 'ok' })),
    loadVolumeDetail: vi.fn(async () => store.volumeDetail),
    loadPlanningRoute: vi.fn(async () => store.planningRoute), loadVolumeLifecycle: vi.fn(async () => null),
  })
  harness.store = store
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(VolumeWorkspace)
  app.mount(container)
  return { app, container, store }
}

describe('B04-B06 chapter planning controls', () => {
  afterEach(() => { document.body.innerHTML = '' })

  it('prepares the next chapter through the formal project command', async () => {
    const mounted = mountWorkspace('official')
    const button = mounted.container.querySelector('[data-next-chapter-actions] [data-action="prepare"]') as HTMLButtonElement
    expect(button).not.toBeNull()
    button.click()
    await Promise.resolve()
    expect(mounted.store.runCommand).toHaveBeenCalledWith('chapter-prepare', expect.objectContaining({
      type: 'planning_prepare_next_chapter',
      payload: { volumeId: 'volume-b04-b06' },
    }))
    mounted.app.unmount()
  })

  it('compiles the prepared chapter through the formal ChapterPlan command', async () => {
    const mounted = mountWorkspace('producing')
    const button = mounted.container.querySelector('[data-next-chapter-actions] [data-action="compile"]') as HTMLButtonElement
    expect(button).not.toBeNull()
    button.click()
    await Promise.resolve()
    expect(mounted.store.runCommand).toHaveBeenCalledWith('chapter-plan-compile', expect.objectContaining({
      type: 'planning_compile_chapter_plan',
      payload: { chapterId: 'chapter:4', chapterGoalHint: expect.any(String) },
    }))
    mounted.app.unmount()
  })
})
