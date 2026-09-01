import { describe, expect, it } from 'vitest'
import { TauriFrontendPort } from '../../src/infrastructure/tauri/frontendPort'

describe('B09/H04 lifecycle frontend boundary', () => {
  it('routes lifecycle projection through the formal Tauri query', async () => {
    let command = ''
    let payload: unknown
    const port = new TauriFrontendPort({
      invoke: async (name, args) => {
        command = name
        payload = args
        return {
          projectId: 'p-b09',
          projectRevision: 2,
          volumeId: 'volume:b09:1',
          planningState: { closeReadiness: 'READY' },
          route: {},
          latestClose: null,
          routeReplans: [],
          owner: 'B09::VolumeLifecycleOwner',
        }
      },
    })
    const result = await port.queryVolumeLifecycle({
      projectId: 'p-b09',
      scope: 'volume-lifecycle:volume:b09:1',
      volumeId: 'volume:b09:1',
      pageSize: 30,
    })
    expect(command).toBe('query_volume_lifecycle')
    expect(payload).toEqual({
      request: {
        projectId: 'p-b09',
        scope: 'volume-lifecycle:volume:b09:1',
        volumeId: 'volume:b09:1',
        pageSize: 30,
      },
    })
    expect(result.owner).toBe('B09::VolumeLifecycleOwner')
  })
})
