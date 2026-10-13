/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

const harness = vi.hoisted(() => ({ store: null as any }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'project-fx12' }, query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const pendingItems = [
  { changeRequestId: 'cr:fx12:user', responsibilityRef: 'thread:promise:door', summary: '申请撤销B级责任：门后的承诺', origin: 'USER_DELETE', createdAt: '2026-10-13T00:00:00Z' },
  { changeRequestId: 'cr:fx12:audit', responsibilityRef: 'thread:promise:bridge', summary: '审计发现既有连续性责任可能不再适用，等待用户确认。', origin: 'CHAPTER_AUDIT', createdAt: '2026-10-13T00:00:01Z' },
]
const mounted: Array<ReturnType<typeof createApp>> = []

afterEach(() => {
  mounted.splice(0).forEach(app => app.unmount())
  document.body.innerHTML = ''
})

async function flush() {
  for (let index = 0; index < 10; index++) await Promise.resolve()
  await nextTick()
}

describe('FX-12 撤销责任请求', () => {
  it('Given 两类待确认撤销请求 When 打开历史中心 Then 分别显示来源并可批准或保留，不显示内部编号', async () => {
    const { default: HistoryCenter } = await import('../../src/ui/pages/HistoryCenter.vue')
    harness.store = reactive({
      currentProjectId: 'project-fx12',
      history: { entries: { items: [], nextCursor: null, total: 0 }, stats: { versions: 0, changes: 0, retcons: 0, pending: 0 } },
      historyKind: 'all',
      loadHistory: vi.fn(async () => harness.store.history),
      isWorkspaceLoading: () => false,
      commandState: () => ({ status: 'idle', message: '' }),
      confirmCommandResult: vi.fn(),
      runCommand: vi.fn(),
      withdrawRequests: { items: pendingItems, truncated: false },
      loadWithdrawRequests: vi.fn(async () => harness.store.withdrawRequests),
      decideWithdrawRequest: vi.fn(async () => undefined),
    })
    const root = document.createElement('div')
    document.body.appendChild(root)
    const app = createApp(HistoryCenter)
    mounted.push(app)
    app.mount(root)
    await flush()

    const section = document.querySelector('[data-withdraw-requests]')
    expect(section?.textContent).toContain('待确认的撤销责任请求')
    expect(section?.textContent).toContain('门后的承诺')
    expect(section?.textContent).toContain('你删除的责任')
    expect(section?.textContent).toContain('章节审核发现')
    expect(section?.textContent).not.toMatch(/thread:|cr:fx12|WITHDRAW|USER_DELETE|CHAPTER_AUDIT/)
    expect(harness.store.loadWithdrawRequests).toHaveBeenCalled()

    const buttons = [...(section?.querySelectorAll('button') ?? [])]
    buttons.find(button => button.textContent?.includes('批准撤销'))!.click()
    buttons.filter(button => button.textContent?.includes('保留责任'))[1]!.click()
    await flush()
    expect(harness.store.decideWithdrawRequest).toHaveBeenNthCalledWith(1, 'cr:fx12:user', 'APPROVE')
    expect(harness.store.decideWithdrawRequest).toHaveBeenNthCalledWith(2, 'cr:fx12:audit', 'REJECT')
  })

  it('Given 撤销请求已处理或责任已失效 When 显示错误 Then 只出现中文提示', () => {
    for (const code of ['RESPONSIBILITY_WITHDRAW_NOT_PENDING', 'RESPONSIBILITY_WITHDRAW_NOT_FOUND', 'B08_RESPONSIBILITY_NOT_ACTIVE']) {
      const message = productErrorMessage(`project command: ${code}`)
      expect(message).toMatch(/[一-鿿]/)
      expect(message).not.toContain(code)
      expect(message).not.toBe('操作未完成，请刷新当前作品后重试。')
    }
  })
})
