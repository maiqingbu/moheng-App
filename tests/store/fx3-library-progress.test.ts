/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import ProjectLibrary from '../../src/ui/pages/ProjectLibrary.vue'

const harness = vi.hoisted(() => ({ store: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('../../src/ui/stores/projectLibraryStore', () => ({ useProjectLibraryStore: () => harness.store }))

function mountLibrary(progress: number) {
  const item = {
    id: 'project:fx3', title: '测试作品', subtitle: '进度显示测试', platform: '平台', genre: '题材', mode: 'DIRECTOR',
    phase: 'OPENING_PRODUCTION', runState: 'IDLE', blockReason: null, signingStatus: 'NOT_STARTED', lifecycle: 'ACTIVE',
    progress, totalCost: 0, totalWords: 0, committedChapters: 0, candidateChapters: 0, pendingDecisions: 0,
    createdAt: '', updatedAt: '', lastOpenedAt: '', operational: true, revision: 1,
  }
  harness.store = reactive({
    projection: { totals: { active: 1, producing: 0, waitingDecision: 0, totalCost: 0 } },
    activeProjects: [item], archivedProjects: [], loadError: '',
    bootstrap: vi.fn(async () => undefined), refresh: vi.fn(async () => undefined),
    commandState: vi.fn(() => ({ status: 'idle', message: '' })), runCommand: vi.fn(),
  })
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(ProjectLibrary)
  app.mount(container)
  return { app, container }
}

afterEach(() => { document.body.innerHTML = '' })

describe('FX-3 progress presentation', () => {
  it('fx3-library-progress-percent displays 0.63 as 63% in text and bar width', async () => {
    const rendered = mountLibrary(0.63)
    await nextTick()

    expect(rendered.container.querySelector('.project-progress-row b')?.textContent).toBe('63%')
    expect((rendered.container.querySelector('.project-progress-row .progress > span') as HTMLElement).style.width).toBe('63%')
    rendered.app.unmount()
  })

  it('formats progress ratios through the shared percentage formatter', async () => {
    const { formatProgressPercent } = await vi.importActual<typeof import('../../src/ui/presentation/progress')>('../../src/ui/presentation/progress')
    expect(formatProgressPercent(0.63)).toBe('63%')
  })
})
