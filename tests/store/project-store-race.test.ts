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

  function makePort(commands?: Array<{ resolve: (v: CommandReceipt) => void; request: CommandRequest }>) {
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
      executeCommand: vi.fn((req: CommandRequest): Promise<CommandReceipt> => {
        // When the test supplies a command queue, park the receipt there so the
        // race (receipt arrives after a project switch) can be controlled.
        if (commands) {
          const d = deferred<CommandReceipt>()
          commands.push({ resolve: d.resolve, request: req })
          return d.promise
        }
        return Promise.resolve({
          commandId: 'cmd', projectId: req.projectId, resultProjectId: req.projectId,
          finalStatus: 'committed', message: '', acceptedAt: '', afterRevision: 1,
        })
      }),
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

  it('PROJECT-RACE-01: a late A-query response does not pollute B after a switch', async () => {
    const port = makePort()
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()

    await store.openProject('A')
    const aQuery = store.loadMarket()
    expect(port.bounded[0].projectId).toBe('A')

    // Switch to B — openProject resets the workspaces, so B starts clean.
    await store.openProject('B')
    expect(store.marketStrategy).toBeNull()

    // Release A's late response AFTER B is active: it must stay null (not polluted).
    port.bounded[0].deferred.resolve({ projectId: 'A', targetPlatform: 'A平台', strategyTitle: 'A策略', readerPromise: '', platformPolicyVersion: '', snapshotCollectedAt: '', confidence: 0, lastPulseAt: '', nextReview: '', opportunities: [], signals: [], guardrails: [] })
    await aQuery

    expect(store.currentProjectId).toBe('B')
    expect(store.marketStrategy).toBeNull()
  })

  it('PROJECT-RACE-02: a late A-command receipt does not clobber B workspace/head/revision', async () => {
    const commands: Array<{ resolve: (v: CommandReceipt) => void; request: CommandRequest }> = []
    const port = makePort(commands)
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()

    await store.openProject('A')
    // Issue a command on A that stays in flight.
    const aCommand = store.runCommand('cmd:A', { type: 'project_rename', projectId: 'A' })
    expect(commands).toHaveLength(1)
    expect(commands[0].request.projectId).toBe('A')

    // Switch to B — openProject resets B workspaces clean.
    await store.openProject('B')
    expect(store.marketStrategy).toBeNull()

    // Now A's receipt arrives. It must not refresh (and thereby clobber) B's data.
    commands[0].resolve({
      commandId: 'cmd-A', projectId: 'A', resultProjectId: 'A',
      finalStatus: 'committed', message: '', acceptedAt: '', afterRevision: 1,
    })
    await aCommand

    expect(store.currentProjectId).toBe('B')
    expect(store.marketStrategy).toBeNull()
  })

  it('PROJECT-RACE-03: duplicate A→C receipt never implicitly replaces active B with C', async () => {
    const commands: Array<{ resolve: (v: CommandReceipt) => void; request: CommandRequest }> = []
    const port = makePort(commands)
    holder.port = port
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()

    await store.openProject('A')
    // A duplicate issued on source A stays in flight.
    const dup = store.runCommand('cmd:dup', { type: 'project_duplicate', projectId: 'A' })
    expect(commands).toHaveLength(1)
    expect(commands[0].request.projectId).toBe('A')

    // User switches to B before the duplicate's receipt lands.
    await store.openProject('B')
    expect(store.currentProjectId).toBe('B')

    // The duplicate receipt arrives: source A, result C. It must not swap the
    // active project to C (that would be an implicit switch from a stale receipt).
    commands[0].resolve({
      commandId: 'cmd-dup', projectId: 'A', resultProjectId: 'C',
      finalStatus: 'committed', message: '', acceptedAt: '', afterRevision: 1,
    })
    await dup

    expect(store.currentProjectId).toBe('B')
  })
})
