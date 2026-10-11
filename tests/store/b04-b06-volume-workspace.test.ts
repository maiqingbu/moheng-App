/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import VolumeWorkspace from '../../src/ui/pages/VolumeWorkspace.vue'

const harness = vi.hoisted(() => ({
  route: { params: { volumeId: 'volume-b04-b06' } },
  store: null as any,
  routerPush: vi.fn(),
}))

vi.mock('vue-router', () => ({ useRoute: () => harness.route, useRouter: () => ({ push: harness.routerPush }) }))
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

function mountWorkspace(
  chapterStatus: 'official' | 'producing',
  currentChapterPlan: unknown = null,
  budgetFailure: Record<string, unknown> | null = null,
) {
  const store = reactive({
    currentProjectId: 'project-b04-b06', volumeDetail: detail(chapterStatus), planningRoute: routeProjection(currentChapterPlan),
    continuityProjection: null as any, continuityVolumeId: 'volume-b04-b06',
    volumeLifecycle: null, advancedMode: false, commandState: vi.fn((key: string) => key === 'chapter-plan-compile' && budgetFailure
      ? { status: 'failed', message: '本章硬性资料超出预算。', contextBudget: budgetFailure }
      : { status: 'idle', message: '' }),
    runCommand: vi.fn(async () => ({ finalStatus: 'committed', message: 'ok' })),
    loadVolumeDetail: vi.fn(async () => store.volumeDetail),
    loadPlanningRoute: vi.fn(async () => store.planningRoute), loadVolumeLifecycle: vi.fn(async () => null),
    loadContinuity: vi.fn(async () => null),
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

  it('routes A/B continuity changes through the existing ChangeRequest flow', async () => {
    harness.routerPush.mockClear()
    const mounted = mountWorkspace('official')
    mounted.store.continuityProjection = {
      projectId: 'project-b04-b06', projectRevision: 4, truncated: false,
      threads: [{
        threadId: 'thread:promise:door', threadClass: 'OBLIGATION', description: '处理门后的承诺。',
        fulfillmentObject: null, logicalStatus: 'OPEN', sourceEventRef: 'event:event:fact:door',
        evidenceRefs: [], participantEntities: [], resumeCondition: { kind: 'IN_VOLUME', value: 'volume:2' },
        authorityLevel: 'B', currentDisposition: 'HOLD', sourceRefs: [],
      }],
    }
    await nextTick()
    mounted.container.querySelector<HTMLButtonElement>('.continuity-projection button')!.click()
    expect(harness.routerPush).toHaveBeenCalledWith({
      name: 'history',
      params: { id: 'project-b04-b06' },
      query: {
        createChangeRequest: '1',
        continuityThreadRef: 'thread:promise:door',
        continuityDescription: '处理门后的承诺。',
      },
    })
    expect(mounted.store.runCommand).not.toHaveBeenCalled()
    mounted.app.unmount()
  })

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

  it('shows oversized HARD sources and manually recompiles only the selected ChapterPlan scope', async () => {
    const mounted = mountWorkspace('producing', null, {
      usedTokens: 4200,
      limitTokens: 4096,
      serializedBytes: 33000,
      limitBytes: 32768,
      scope: 'CHAPTER:chapter:4',
      budgetPolicyVersion: 'b06-hard-budget.r2',
      sourceSizes: [
        { kind: 'TASK', sourceRef: 'task:required', selectionKind: 'TASK', selectionRef: 'task:required', estimatedTokens: 700, serializedBytes: 2100, adjustable: true },
        { kind: 'TASK', sourceRef: 'task:optional', selectionKind: 'TASK', selectionRef: 'task:optional', estimatedTokens: 500, serializedBytes: 1500, adjustable: true },
        { kind: 'CONTINUITY', sourceRef: 'thread:promise', selectionKind: 'RESPONSIBILITY', selectionRef: 'thread:promise', estimatedTokens: 300, serializedBytes: 900, adjustable: true },
        { kind: 'RESERVATION', sourceRef: 'reservation:later', selectionKind: 'RESERVATION', selectionRef: 'reservation:later', estimatedTokens: 150, serializedBytes: 450, adjustable: true },
        { kind: 'ENTITY', sourceRef: 'entity:lead', selectionKind: 'ENTITY', selectionRef: 'entity:lead', estimatedTokens: 80, serializedBytes: 240, adjustable: true },
      ],
    })
    await Promise.resolve()
    const panel = mounted.container.querySelector('[data-hard-budget-scope-adjustment]')
    expect(panel?.textContent).toContain('task:optional')
    expect(panel?.textContent).toContain('500 token / 1500 字节')
    const optionalTask = [...panel!.querySelectorAll('label')]
      .find(label => label.textContent?.includes('task:optional'))
      ?.querySelector('input') as HTMLInputElement
    optionalTask.click()
    await Promise.resolve()
    const recompile = panel!.querySelector('[data-action="recompile-narrowed-scope"]') as HTMLButtonElement
    recompile.click()
    await Promise.resolve()
    await Promise.resolve()
    expect(mounted.store.runCommand).toHaveBeenCalledWith('chapter-plan-compile', expect.objectContaining({
      type: 'planning_compile_chapter_plan',
      payload: expect.objectContaining({
        chapterId: 'chapter:4',
        scopeSelection: {
          taskRefs: ['task:required'],
          responsibilityRefs: ['thread:promise'],
          reservationRefs: ['reservation:later'],
          entityRefs: ['entity:lead'],
        },
      }),
    }))
    mounted.app.unmount()
  })
})
