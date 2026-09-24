/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'

import type { HistoryEntry, HistoryProjection } from '../../src/application/contracts'
import HistoryCenter from '../../src/ui/pages/HistoryCenter.vue'

const harness = vi.hoisted(() => ({ store: null as any }))
const mounted: Array<ReturnType<typeof createApp>> = []
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'p' }, query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))

const makeEntry = (id: string, kind: HistoryEntry['kind']): HistoryEntry => ({
  id,
  kind,
  title: id,
  status: 'applied',
  createdAt: '2026-09-23T10:00:00Z',
  summary: id,
  impact: '以提交回执为准。',
  scope: '当前作品',
})

const page = (...items: HistoryEntry[]): HistoryProjection => ({
  entries: { items, nextCursor: null, total: items.length },
  stats: { versions: 1, changes: 1, retcons: 0, pending: 0 },
})

const allPage = page(makeEntry('全部中的版本记录', 'VERSION'), makeEntry('全部中的修改请求', 'CHANGE_REQUEST'))
const versionPage = page(makeEntry('版本筛选记录', 'VERSION'))

async function mountHistory(
  loadHistory: ReturnType<typeof vi.fn>,
  initialHistory: HistoryProjection | null = allPage,
  initialKind: 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON' | null = 'all',
) {
  harness.store = reactive({
    history: initialHistory,
    historyKind: initialKind,
    loadHistory,
    isWorkspaceLoading: () => false,
    commandState: () => ({ status: 'idle', message: '' }),
  })
  const root = document.createElement('div')
  document.body.appendChild(root)
  const app = createApp(HistoryCenter)
  mounted.push(app)
  app.mount(root)
  for (let index = 0; index < 8; index++) await Promise.resolve()
  await nextTick()
  return root
}

function button(root: HTMLElement, label: string) {
  const found = Array.from(root.querySelectorAll('button')).find(item => item.textContent?.trim() === label)
  if (!found) throw new Error(`Missing button: ${label}`)
  return found as HTMLButtonElement
}

afterEach(() => {
  mounted.splice(0).forEach(app => app.unmount())
  document.body.innerHTML = ''
})

describe('History filter loading and recovery', () => {
  it('masks prior rows while a filter loads and shows only the new filter after success', async () => {
    let resolveVersion!: (value: HistoryProjection) => void
    const loadHistory = vi.fn(async (kind: string) => {
      if (kind === 'VERSION') return new Promise<HistoryProjection>(resolve => {
        resolveVersion = value => { harness.store.history = value; resolve(value) }
      })
      return allPage
    })
    const root = await mountHistory(loadHistory)

    button(root, '版本').click()
    await nextTick()

    expect(root.textContent).toContain('正在加载版本记录')
    expect(root.textContent).not.toContain('全部中的修改请求')
    expect(button(root, '版本').disabled).toBe(true)

    resolveVersion(versionPage)
    for (let index = 0; index < 4; index++) await Promise.resolve()
    await nextTick()

    expect(root.textContent).toContain('版本筛选记录')
    expect(root.textContent).not.toContain('全部中的修改请求')
  })

  it('restores the loaded filter and offers retry after a filter query fails', async () => {
    let rejectVersion!: (reason?: unknown) => void
    let resolveRetry!: (value: HistoryProjection) => void
    let versionAttempts = 0
    const loadHistory = vi.fn(async (kind: string) => {
      if (kind !== 'VERSION') return allPage
      versionAttempts += 1
      if (versionAttempts === 1) return new Promise<HistoryProjection>((_resolve, reject) => { rejectVersion = reject })
      return new Promise<HistoryProjection>(resolve => {
        resolveRetry = value => { harness.store.history = value; resolve(value) }
      })
    })
    const root = await mountHistory(loadHistory)

    button(root, '版本').click()
    rejectVersion(new Error('temporary query failure'))
    for (let index = 0; index < 4; index++) await Promise.resolve()
    await nextTick()

    expect(button(root, '全部').classList.contains('active')).toBe(true)
    expect(root.textContent).toContain('仍显示“全部”记录')
    expect(root.textContent).toContain('全部中的修改请求')
    expect(root.textContent).not.toContain('temporary query failure')

    button(root, '重试').click()
    await nextTick()
    resolveRetry(versionPage)
    for (let index = 0; index < 4; index++) await Promise.resolve()
    await nextTick()

    expect(root.textContent).toContain('版本筛选记录')
    expect(root.textContent).not.toContain('全部中的修改请求')
  })

  it('keeps the cached filter label when the default all reload fails after re-entry', async () => {
    const loadHistory = vi.fn(async () => { throw new Error('temporary query failure') })
    const root = await mountHistory(loadHistory, versionPage, 'VERSION')

    expect(button(root, '版本').classList.contains('active')).toBe(true)
    expect(root.textContent).toContain('仍显示“版本”记录')
    expect(root.textContent).toContain('版本筛选记录')
    expect(root.textContent).not.toContain('全部中的修改请求')
    expect(button(root, '重试')).toBeDefined()
  })

  it('shows an error and retry on a cold start, then renders the first loaded page', async () => {
    let rejectFirst!: (reason?: unknown) => void
    let resolveRetry!: (value: HistoryProjection) => void
    let attempts = 0
    const loadHistory = vi.fn(async () => {
      attempts += 1
      if (attempts === 1) return new Promise<HistoryProjection>((_resolve, reject) => { rejectFirst = reject })
      return new Promise<HistoryProjection>(resolve => {
        resolveRetry = value => {
          harness.store.history = value
          harness.store.historyKind = 'all'
          resolve(value)
        }
      })
    })
    const root = await mountHistory(loadHistory, null, null)

    expect(root.textContent).toContain('正在加载全部记录')
    rejectFirst(new Error('cold start unavailable'))
    for (let index = 0; index < 4; index++) await Promise.resolve()
    await nextTick()

    expect(root.querySelector('[role="alert"]')?.textContent).toContain('暂时无法加载历史记录，请重试。')
    expect(root.querySelector('[role="alert"]')?.textContent).not.toContain('仍显示')
    expect(root.textContent).not.toContain('cold start unavailable')
    button(root, '重试').click()
    await nextTick()
    expect(root.textContent).toContain('正在加载全部记录')

    resolveRetry(allPage)
    for (let index = 0; index < 4; index++) await Promise.resolve()
    await nextTick()

    expect(root.textContent).toContain('全部中的修改请求')
    expect(root.querySelector('[role="alert"]')).toBeNull()
  })
})
