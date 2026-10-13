/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import HistoryCenter from '../../src/ui/pages/HistoryCenter.vue'

const harness = vi.hoisted(() => ({ store: null as any }))
const mounted: Array<ReturnType<typeof createApp>> = []
vi.mock('vue-router', () => ({
  useRoute: () => ({
    params: { id: 'project-fx6' },
    query: {
      createChangeRequest: '1',
      continuityThreadRef: 'thread:promise:door',
      continuityDescription: '处理门后的承诺。',
    },
  }),
  useRouter: () => ({ push: vi.fn() }),
}))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const projection = () => ({
  entries: { items: [], nextCursor: null, total: 0 },
  stats: { versions: 0, changes: 0, retcons: 0, pending: 0 },
})

afterEach(() => {
  mounted.splice(0).forEach(app => app.unmount())
  document.body.innerHTML = ''
})

describe('FX-6 continuity ChangeRequest navigation', () => {
  it('opens the existing change form and binds the selected continuity thread as a reference', async () => {
    const calls: Array<{ key: string; request: Record<string, any> }> = []
    harness.store = reactive({
      currentProjectId: 'project-fx6',
      history: projection(),
      historyKind: 'all',
      loadHistory: vi.fn(async () => { harness.store.history = projection(); return harness.store.history }),
      isWorkspaceLoading: () => false,
      commandState: () => ({ status: 'idle', message: '' }),
      withdrawRequests: null,
      loadWithdrawRequests: vi.fn(async () => null),
      confirmCommandResult: vi.fn(),
      runCommand: vi.fn(async (key: string, request: Record<string, any>) => {
        calls.push({ key, request })
        return { finalStatus: 'committed', result: { status: 'PREFLIGHTED', impactDigest: 'd'.repeat(64), scopeClass: 'LOCAL' } }
      }),
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(HistoryCenter)
    mounted.push(app)
    app.mount(root)
    for (let index = 0; index < 8; index++) await Promise.resolve()
    await nextTick()

    expect(document.querySelector<HTMLTextAreaElement>('.modal-wrap textarea')?.value).toContain('处理门后的承诺')
    const submit = [...document.querySelectorAll('button')].find(button => button.textContent?.includes('开始影响分析')) as HTMLButtonElement
    expect(submit).toBeDefined()
    submit.click()
    for (let index = 0; index < 12; index++) await Promise.resolve()
    await nextTick()

    expect(calls[0].request.type).toBe('change_request_create')
    expect(calls[0].request.payload.targetRefs).toEqual(['thread:promise:door'])
    expect(calls[0].request.payload.sourceRefs).toEqual(['thread:promise:door'])
    expect(calls[0].request.payload.changeType).toBe('CHANGE')
  })
})
