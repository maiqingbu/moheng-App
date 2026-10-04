import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import type { FrontendPort, HistoryEntry, HistoryProjection } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

const entry = (id: string): HistoryEntry => ({
  id,
  kind: 'CHANGE_REQUEST',
  source: 'MANUSCRIPT_READER',
  title: id,
  status: 'impact_ready',
  createdAt: '2026-09-23T10:00:00Z',
  summary: id,
  impact: '预计影响当前相关内容。',
  scope: '局部',
})

const versionEntry = (id: string): HistoryEntry => ({
  ...entry(id),
  kind: 'VERSION',
  source: null,
})

const retconEntry = (id: string): HistoryEntry => ({
  ...entry(id),
  kind: 'RETCON',
  source: null,
})

const historyPage = (items: HistoryEntry[], nextCursor: string | null = null): HistoryProjection => ({
  entries: { items, nextCursor, total: items.length },
  stats: { versions: 1, changes: 1, retcons: 1, pending: 1 },
})

async function makeStore(queryHistory: FrontendPort['queryHistory']) {
  const port = {
    queryProjectOverview: vi.fn(async () => ({
      project: { id: 'history-project', revision: 0, updatedAt: '' },
      pendingDecisionCount: 0, recentChapters: [], recentArtifacts: [],
      healthSummary: { level: 'healthy', label: '', note: '' },
    })),
    queryHistory,
  } as unknown as FrontendPort
  holder.port = port
  const { useProjectStore } = await import('../../src/ui/stores/projectStore')
  const store = useProjectStore()
  await store.openProject('history-project')
  return { store, port }
}

describe('History pagination', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetModules()
  })

  it('requests the next cursor and appends rows without losing Reader provenance', async () => {
    const firstPage: HistoryProjection = {
      entries: {
        items: [
          ...Array.from({ length: 25 }, (_, index) => versionEntry(`version-${index}`)),
          ...Array.from({ length: 15 }, (_, index) => entry(`reader-first-${index}`)),
        ],
        nextCursor: '40',
        total: 50,
      },
      stats: { versions: 25, changes: 25, retcons: 0, pending: 25 },
    }
    const secondPage: HistoryProjection = {
      entries: { items: Array.from({ length: 10 }, (_, index) => entry(`second-${index}`)), nextCursor: null, total: 50 },
      stats: { versions: 25, changes: 25, retcons: 0, pending: 25 },
    }
    const queryHistory = vi.fn(async (request: { cursor?: string | null }) => request.cursor ? secondPage : firstPage)
    const { store, port } = await makeStore(queryHistory as FrontendPort['queryHistory'])

    await store.loadHistory('all', true)
    await store.loadHistory('all', false, '40')

    expect(port.queryHistory).toHaveBeenNthCalledWith(1, expect.objectContaining({ cursor: null, kind: 'all' }))
    expect(port.queryHistory).toHaveBeenNthCalledWith(2, expect.objectContaining({ cursor: '40', kind: 'all' }))
    expect(store.history?.entries.items).toHaveLength(50)
    expect(store.history?.entries.items.filter(item => item.kind === 'VERSION')).toHaveLength(25)
    expect(store.history?.entries.items[25].source).toBe('MANUSCRIPT_READER')
    expect(store.history?.entries.items[49].source).toBe('MANUSCRIPT_READER')
    expect(store.history?.entries.nextCursor).toBeNull()
  })

  it('reloads all history after returning from a version-only cache', async () => {
    const allPage = historyPage([versionEntry('version-1'), entry('reader-1')])
    const versionPage = historyPage([versionEntry('version-1')])
    const queryHistory = vi.fn(async (request: { kind?: string }) => request.kind === 'VERSION' ? versionPage : allPage)
    const { store, port } = await makeStore(queryHistory as FrontendPort['queryHistory'])

    await store.loadHistory('VERSION', true)
    expect(store.history?.entries.items.map(item => item.kind)).toEqual(['VERSION'])

    // Re-entering History mounts with its default all filter.
    await store.loadHistory()

    expect(port.queryHistory).toHaveBeenCalledTimes(2)
    expect(port.queryHistory).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: null, kind: 'all' }))
    expect(store.history?.entries.items.map(item => item.kind)).toEqual(['VERSION', 'CHANGE_REQUEST'])
  })

  it('replaces filtered pages so VERSION, CHANGE_REQUEST, RETCON, and all never mix rows', async () => {
    const pages: Record<string, HistoryProjection> = {
      VERSION: historyPage([versionEntry('version-1')]),
      CHANGE_REQUEST: historyPage([entry('reader-1')]),
      RETCON: historyPage([retconEntry('retcon-1')]),
      all: historyPage([versionEntry('version-1'), entry('reader-1'), retconEntry('retcon-1')]),
    }
    const queryHistory = vi.fn(async (request: { kind?: string }) => pages[request.kind ?? 'all'])
    const { store } = await makeStore(queryHistory as FrontendPort['queryHistory'])

    for (const kind of ['VERSION', 'CHANGE_REQUEST', 'RETCON', 'all'] as const) {
      await store.loadHistory(kind, true)
      expect(store.history?.entries.items.every(item => item.kind === kind || kind === 'all')).toBe(true)
    }
    expect(store.history?.entries.items.map(item => item.kind)).toEqual(['VERSION', 'CHANGE_REQUEST', 'RETCON'])
  })

  it('keeps the latest filter result when older filter requests resolve late', async () => {
    let resolveVersion!: (page: HistoryProjection) => void
    let resolveAll!: (page: HistoryProjection) => void
    const queryHistory = vi.fn((request: { kind?: string }) => new Promise<HistoryProjection>(resolve => {
      if (request.kind === 'VERSION') resolveVersion = resolve
      else resolveAll = resolve
    }))
    const { store } = await makeStore(queryHistory as FrontendPort['queryHistory'])

    const versionRequest = store.loadHistory('VERSION', true)
    const allRequest = store.loadHistory('all', true)
    resolveAll(historyPage([versionEntry('version-1'), entry('reader-1')]))
    await allRequest
    resolveVersion(historyPage([versionEntry('version-1')]))
    await versionRequest

    expect(store.history?.entries.items.map(item => item.kind)).toEqual(['VERSION', 'CHANGE_REQUEST'])
  })

  it('resets a stale cursor when the requested filter no longer matches the cached page', async () => {
    const queryHistory = vi.fn(async (request: { kind?: string }) => request.kind === 'VERSION'
      ? historyPage([versionEntry('version-1')], '40')
      : historyPage([entry('reader-1')]))
    const { store, port } = await makeStore(queryHistory as FrontendPort['queryHistory'])

    await store.loadHistory('VERSION', true)
    await store.loadHistory('CHANGE_REQUEST', false, '40')

    expect(port.queryHistory).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: null, kind: 'CHANGE_REQUEST' }))
    expect(store.history?.entries.items.map(item => item.kind)).toEqual(['CHANGE_REQUEST'])
  })
})
