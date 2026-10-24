/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import type { BlockReason, RunState } from '../../src/application/contracts'
import ProjectLibrary from '../../src/ui/pages/ProjectLibrary.vue'
import RuntimeBanner from '../../src/ui/components/RuntimeBanner.vue'
import { productionRunStateForJob } from '../../src/ui/presentation/productionRunState'

const harness = vi.hoisted(() => ({ store: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('../../src/ui/stores/projectLibraryStore', () => ({ useProjectLibraryStore: () => harness.store }))

function mountLibrary(overrides: Record<string, unknown> = {}) {
  const item = {
    id: 'project:runtime-projection', title: '投影状态测试', subtitle: 'runtime projection',
    platform: '平台', genre: '题材', mode: 'DIRECTOR', phase: 'OPENING_PRODUCTION',
    runState: 'IDLE', blockReason: null, signingStatus: 'NOT_STARTED', lifecycle: 'ACTIVE',
    progress: 0, totalCost: 0, totalWords: 0, committedChapters: 0, candidateChapters: 0,
    pendingDecisions: 0, createdAt: '', updatedAt: '', lastOpenedAt: '', operational: true, revision: 1,
    ...overrides,
  }
  harness.store = reactive({
    projection: { totals: { active: 1, archived: 0, producing: 0, waitingDecision: 0, completed: 0, totalCost: 0 } },
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

function mountBanner(state: RunState, reason: BlockReason) {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(RuntimeBanner, { state, reason })
  app.mount(container)
  return { app, container }
}

afterEach(() => { document.body.innerHTML = '' })

describe('cancelled runtime projection display', () => {
  it('keeps the cancelled project idle without a stale warning in both views', async () => {
    const state = productionRunStateForJob('CANCELLED', 'WAITING_DECISION')
    expect(state).toBe('IDLE')
    const library = mountLibrary({ runState: state, blockReason: null })
    await nextTick()
    expect(library.container.textContent).toContain('未运行')
    expect(library.container.querySelector('.project-next.warning-text')).toBeNull()

    const production = mountBanner(state, null)
    expect(production.container.querySelector('.banner-title')?.textContent).toBe('等待开始')
    expect(production.container.textContent).not.toContain('远端请求结果未知')
    library.app.unmount()
    production.app.unmount()
  })

  it('keeps an unresolved unknown decision visible in both views', async () => {
    const unknown: BlockReason = 'UNKNOWN_REMOTE_OUTCOME'
    const state = productionRunStateForJob('WAITING_DECISION', 'IDLE')
    expect(state).toBe('WAITING_DECISION')
    const library = mountLibrary({ runState: state, blockReason: unknown, pendingDecisions: 1 })
    await nextTick()
    expect(library.container.textContent).toContain('待决定 1')
    expect(library.container.querySelector('.project-next.warning-text')?.textContent)
      .toContain('远端请求结果未知，等待处理决定')

    const production = mountBanner(state, unknown)
    expect(production.container.querySelector('.banner-title')?.textContent).toBe('需要你的决定')
    expect(production.container.querySelector('.banner-copy')?.textContent)
      .toBe('远端请求结果未知，等待处理决定。')
    library.app.unmount()
    production.app.unmount()
  })

  it('does not hide other explicit warnings just because the run state is idle', async () => {
    const library = mountLibrary({ runState: 'IDLE', blockReason: 'BUDGET_LIMIT' })
    await nextTick()
    expect(library.container.querySelector('.project-next.warning-text')?.textContent)
      .toContain('预算达到上限')
    library.app.unmount()
  })
})
