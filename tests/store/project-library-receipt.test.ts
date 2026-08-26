import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandReceipt, CommandRequest, FrontendPort, ProjectLibraryProjection } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

describe('ProjectLibraryStore Receipt source binding (B01)', () => {
  function makePort(receipt: CommandReceipt) {
    const projection: ProjectLibraryProjection = {
      projects: [],
      totals: { all: 0, active: 0, archived: 0, producing: 0, waitingDecision: 0, completed: 0, totalCost: 0 },
    }
    return {
      queryProjectLibrary: vi.fn(() => Promise.resolve(projection)),
      executeCommand: vi.fn(() => Promise.resolve(receipt)),
    } as unknown as FrontendPort & { queryProjectLibrary: ReturnType<typeof vi.fn>; executeCommand: ReturnType<typeof vi.fn> }
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetModules()
  })

  it('LIB-RECEIPT-01: same commandId with a wrong source project is rejected', async () => {
    const port = makePort({ commandId: 'cmd-1', projectId: 'C', resultProjectId: 'C', finalStatus: 'committed', message: 'wrong', acceptedAt: '' })
    holder.port = port
    const { useProjectLibraryStore } = await import('../../src/ui/stores/projectLibraryStore')
    const store = useProjectLibraryStore()

    await expect(store.runCommand('duplicate-A', {
      type: 'project_duplicate', commandId: 'cmd-1', projectId: 'A', expectedRevision: 0,
    })).rejects.toThrow('PROTOCOL / PROJECT_BINDING_ERROR')
    expect(port.queryProjectLibrary).not.toHaveBeenCalled()
    expect(store.commandState('duplicate-A').status).toBe('failed')
  })

  it('LIB-RECEIPT-02: a correct source receipt accepts resultProjectId as the duplicate result', async () => {
    const port = makePort({ commandId: 'cmd-2', projectId: 'A', resultProjectId: 'C', finalStatus: 'committed', message: 'ok', acceptedAt: '' })
    holder.port = port
    const { useProjectLibraryStore } = await import('../../src/ui/stores/projectLibraryStore')
    const store = useProjectLibraryStore()

    const receipt = await store.runCommand('duplicate-A', {
      type: 'project_duplicate', commandId: 'cmd-2', projectId: 'A', expectedRevision: 0,
    })
    expect(receipt.resultProjectId).toBe('C')
    expect(store.commandState('duplicate-A').status).toBe('committed')
    expect(port.queryProjectLibrary).toHaveBeenCalledTimes(1)
  })

  it('LIB-RECEIPT-03: a wrong receipt does not refresh or write SUCCESS state', async () => {
    const port = makePort({ commandId: 'cmd-3', projectId: 'C', resultProjectId: 'C', finalStatus: 'committed', message: 'wrong', acceptedAt: '' })
    holder.port = port
    const { useProjectLibraryStore } = await import('../../src/ui/stores/projectLibraryStore')
    const store = useProjectLibraryStore()

    await expect(store.runCommand('archive-A', {
      type: 'project_archive', commandId: 'cmd-3', projectId: 'A', expectedRevision: 0,
    })).rejects.toThrow('PROTOCOL / PROJECT_BINDING_ERROR')
    expect(port.queryProjectLibrary).not.toHaveBeenCalled()
    expect(store.commandState('archive-A').status).not.toBe('committed')
  })

  it('LIB-RECEIPT-04: archive/delete/restore all validate the source project', async () => {
    for (const [index, type] of (['project_archive', 'project_delete', 'project_restore'] as const).entries()) {
      const commandId = `cmd-4-${index}`
      const port = makePort({ commandId, projectId: 'C', resultProjectId: 'C', finalStatus: 'committed', message: 'wrong', acceptedAt: '' })
      holder.port = port
      const { useProjectLibraryStore } = await import('../../src/ui/stores/projectLibraryStore')
      const store = useProjectLibraryStore()
      await expect(store.runCommand(`${type}-A`, { type, commandId, projectId: 'A', expectedRevision: 0 }))
        .rejects.toThrow('PROTOCOL / PROJECT_BINDING_ERROR')
      expect(port.queryProjectLibrary).not.toHaveBeenCalled()
      vi.resetModules()
    }
  })
})
