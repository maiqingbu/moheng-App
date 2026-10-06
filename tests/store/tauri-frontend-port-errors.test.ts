import { describe, expect, it, vi } from 'vitest'
import { TauriFrontendPort } from '../../src/infrastructure/tauri/frontendPort'

describe('TauriFrontendPort native command errors', () => {
  it('preserves backend project errors instead of mapping them to IPC_UNSUPPORTED', async () => {
    const backendError = new Error('project not found')
    const ipc = { invoke: vi.fn().mockRejectedValue(backendError) }
    const port = new TauriFrontendPort(ipc)

    await expect(port.queryProjectOverview({ projectId: 'missing' })).rejects.toBe(backendError)
  })

  it('maps only a native command-name not-found error to IPC_UNSUPPORTED', async () => {
    const ipc = { invoke: vi.fn().mockRejectedValue(new Error('command query_unknown not found')) }
    const port = new TauriFrontendPort(ipc)

    await expect(port.queryProjectOverview({ projectId: 'p' })).rejects.toThrow('IPC_UNSUPPORTED:query_project_overview')
  })
})
