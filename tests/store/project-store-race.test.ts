import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

import type { FrontendPort, CommandRequest, CommandReceipt } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

// The project store grabs `getFrontendPort()` at module load, so the port must
// be resolvable the moment the store module is evaluated. We hold it on a
// module-scope holder that the hoisted mock reads lazily, and import the store
// dynamically after assigning the holder.
describe('projectStore workspace race on project switch (B01-T06)', () => {
  interface QueryRecord {
    projectId: string
    scope: string
    deferred: { resolve: (value: unknown) => void; reject: (e: unknown) => void }
  }

  function makePort() {
    const bounded: QueryRecord[] = []

    const deferred = <T>() => {
      let resolve!: (v: T) => void
      let reject!: (e: unknown) => void
      const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
      return { promise, resolve, reject }
    }

    const overview = (projectId: string) => ({
      project: { id: projectId, revision: 0, updatedAt: '' },
      pendingDecisionCount: 0, recentChapters: [], recentArtifacts: [],
      healthSummary: { level: 'healthy', label: '', note: '' },
    })

    const port = {
      // Overview resolves immediately: openProject must settle so the test can
      // reach the workspace-query race. The race we exercise is on the
      // delegated workspace query, not the overview itself.
      queryProjectOverview: vi.fn((request: { projectId: string }) =>
        Promise.resolve(overview(request.projectId))),
      queryMarketStrategy: vi.fn((request: { projectId: string; scope: string }) => {
        const d = deferred<unknown>()
        bounded.push({ projectId: request.projectId, scope: request.scope, deferred: d })
        return d.promise
      }),
      queryProductionCenter: vi.fn(() => Promise.resolve(null)),
      queryProjectSettings: vi.fn(() => Promise.resolve(null)),
      queryBookHealth: vi.fn(() => Promise.resolve(null)),
      queryCanon: vi.fn(() => Promise.resolve(null)),
      queryVolumeList: vi.fn(() => Promise.resolve(null)),
      queryStoryDirections: vi.fn(() => Promise.resolve(null)),
      queryVolumeDetail: vi.fn(() => Promise.resolve(null)),
      queryDecisions: vi.fn(() => Promise.resolve(null)),
      queryReaderVolumes: vi.fn(() => Promise.resolve(null)),
      queryChapterList: vi.fn(() => Promise.resolve(null)),
      queryChapterFinal: vi.fn(() => Promise.resolve(null)),
      queryChapterVersions: vi.fn(() => Promise.resolve(null)),
      queryHistory: vi.fn(() => Promise.resolve(null)),
      searchFulltext: vi.fn(() => Promise.resolve(null)),
      queryMarketOpportunities: vi.fn(() => Promise.resolve([])),
      queryGlobalSettings: vi.fn(() => Promise.resolve(null)),
      executeCommand: vi.fn((_req: CommandRequest): Promise<CommandReceipt> => Promise.resolve({
        commandId: 'cmd', finalStatus: 'committed', message: '', acceptedAt: '', afterRevision: 1,
      })),
      testProvider: vi.fn((): Promise<CommandReceipt> => Promise.resolve({
        commandId: 'cmd', finalStatus: 'committed', message: '', acceptedAt: '',
      })),
      bounded,
    }
    return port as unknown as FrontendPort & { bounded: QueryRecord[] }
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    // The store resolves `getFrontendPort()` at module load, so each test must
    // re-evaluate the module to pick up its own port double.
    vi.resetModules()
  })

  it('drops a late A-market response after the store switches to B', async () => {
    const port = makePort()
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()

    // Open project A (overview resolves) and kick off a market query that stays in flight.
    await store.openProject('A')
    const market = store.loadMarket()
    expect(port.bounded).toHaveLength(1)
    expect(port.bounded[0].projectId).toBe('A')

    // Switch to project B before A's market response lands.
    await store.openProject('B')
    expect(store.currentProjectId).toBe('B')

    // Now release A's market response: it must NOT overwrite B's workspace.
    port.bounded[0].deferred.resolve({
      projectId: 'A', targetPlatform: 'A平台', strategyTitle: 'A策略', readerPromise: '',
      platformPolicyVersion: '', snapshotCollectedAt: '', confidence: 0, lastPulseAt: '',
      nextReview: '', opportunities: [], signals: [], guardrails: [],
    })
    await market

    expect(store.marketStrategy).toBeNull()
  })

  it('keeps an in-flight B query stale and does not let it clobber after switching again', async () => {
    const port = makePort()
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()

    await store.openProject('A')
    const aMarket = store.loadMarket()
    expect(port.bounded[0].projectId).toBe('A')

    // Switch to B; A's market resolves late — must not be written to B.
    await store.openProject('B')
    port.bounded[0].deferred.resolve({ projectId: 'A', targetPlatform: 'A', strategyTitle: 'A策略', readerPromise: '', platformPolicyVersion: '', snapshotCollectedAt: '', confidence: 0, lastPulseAt: '', nextReview: '', opportunities: [], signals: [], guardrails: [] })
    await aMarket
    expect(store.marketStrategy).toBeNull()

    // Now a B market query that lands correctly does write.
    const bMarket = store.loadMarket()
    port.bounded[1].deferred.resolve({ projectId: 'B', targetPlatform: 'B平台', strategyTitle: 'B策略', readerPromise: '', platformPolicyVersion: '', snapshotCollectedAt: '', confidence: 0, lastPulseAt: '', nextReview: '', opportunities: [], signals: [], guardrails: [] })
    await bMarket
    expect(store.marketStrategy?.targetPlatform).toBe('B平台')
  })
})
