import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import type { FrontendPort, HealthSourcePage } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

describe('B13 health factor source pagination', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetModules()
  })

  it('requests the next source cursor and preserves the global total', async () => {
    const firstPage: HealthSourcePage = {
      items: Array.from({ length: 50 }, (_, index) => ({ kind: 'volume-plan', ref: `volume:${index}` })),
      total: 55,
      truncated: true,
      nextCursor: 'cursor:50',
    }
    const secondPage: HealthSourcePage = {
      items: Array.from({ length: 5 }, (_, index) => ({ kind: 'volume-plan', ref: `volume:${index + 50}` })),
      total: 55,
      truncated: false,
      nextCursor: null,
    }
    const queryBookHealthFactorSources = vi.fn(async (request: { cursor?: string | null }) =>
      request.cursor ? secondPage : firstPage,
    )
    holder.port = {
      queryProjectOverview: vi.fn(async () => ({
        project: { id: 'health-project', revision: 7, updatedAt: '' },
        pendingDecisionCount: 0,
        recentChapters: [],
        recentArtifacts: [],
        healthSummary: { level: 'healthy', label: '', note: '' },
      })),
      queryBookHealthFactorSources,
    } as unknown as FrontendPort

    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('health-project')

    await store.loadHealthFactorSources('ROUTE_DRIFT')
    await store.loadHealthFactorSources('ROUTE_DRIFT', 'cursor:50')

    expect(queryBookHealthFactorSources).toHaveBeenNthCalledWith(1, expect.objectContaining({
      projectId: 'health-project',
      scope: 'health:sources:ROUTE_DRIFT',
      pageSize: 50,
      factorCode: 'ROUTE_DRIFT',
      cursor: null,
    }))
    expect(queryBookHealthFactorSources).toHaveBeenNthCalledWith(2, expect.objectContaining({
      projectId: 'health-project',
      factorCode: 'ROUTE_DRIFT',
      cursor: 'cursor:50',
    }))
    expect(store.healthFactorSourcePages.ROUTE_DRIFT?.items).toHaveLength(5)
    expect(store.healthFactorSourcePages.ROUTE_DRIFT?.total).toBe(55)
    expect(store.healthFactorSourcePages.ROUTE_DRIFT?.nextCursor).toBeNull()
  })
})
