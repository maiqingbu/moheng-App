import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandReceipt, FrontendPort, ProviderSettingsInput } from '../../src/application/contracts'

const holder = vi.hoisted(() => ({ port: null as unknown }))

vi.mock('../../src/app/providers/runtimePort', () => ({
  getFrontendPort: () => holder.port,
}))

beforeEach(() => {
  setActivePinia(createPinia())
  vi.resetModules()
})

describe('设置页钥匙串等待状态', () => {
  it('Given saved key is used When provider test is pending Then UI state explains the system authorization wait', async () => {
    let resolveReceipt!: (receipt: CommandReceipt) => void
    const pendingReceipt = new Promise<CommandReceipt>(resolve => { resolveReceipt = resolve })
    const port = {
      testProvider: vi.fn(() => pendingReceipt),
    } as unknown as FrontendPort & { testProvider: ReturnType<typeof vi.fn> }
    holder.port = port

    const { useProjectLibraryStore } = await import('../../src/ui/stores/projectLibraryStore')
    const store = useProjectLibraryStore()
    const settings = {
      providerId: 'openai-compatible',
      modelId: 'mimo-v2.6-flash',
      endpoint: 'https://provider.invalid/v1',
      enabled: true,
    } as ProviderSettingsInput

    const request = store.testProvider('provider-test', settings)
    expect(store.commandState('provider-test')).toMatchObject({
      status: 'submitting',
      message: '正在等待系统钥匙串授权',
    })

    resolveReceipt({ commandId: 'provider-test', finalStatus: 'committed', message: 'done', acceptedAt: '' })
    await request
  })
})
