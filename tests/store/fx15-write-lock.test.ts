/* @vitest-environment happy-dom */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandRequest } from '../../src/application/contracts'
import { productErrorMessage, receiptSummary } from '../../src/ui/presentation/productLanguage'

const holder = vi.hoisted(() => ({ port: null as any }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => holder.port }))

let pinia = createPinia()
beforeEach(() => { pinia = createPinia(); setActivePinia(pinia); vi.resetModules() })

function port(isActive: boolean) {
  return {
    queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 1 }, recentChapters: [] })),
    queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
    queryProductionCenter: vi.fn(async () => ({ isActive, jobStatus: isActive ? 'RUNNING' : 'COMPLETED', tasks: [] })),
    executeCommand: vi.fn(async (request: CommandRequest) => ({
      commandId: request.commandId, projectId: 'p', finalStatus: 'accepted', actionStatus: 'PRODUCTION_CONTROL_REQUESTED', message: '', receiptId: '', acceptedAt: '',
    })),
  }
}

describe('FX-15 生成期间只读', () => {
  it('Given 本章正在生成 When 发起保存类操作 Then 立即提示只读且不发送请求；暂停仍可发送', async () => {
    holder.port = port(true)
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    await store.loadProduction(true)
    expect(store.writeLocked).toBe(true)

    await expect(store.runCommand('adjust', { type: 'thread_responsibility_adjust', expectedRevision: 1, payload: { action: 'DELETE' } }))
      .rejects.toThrow('PROJECT_WRITE_LOCKED')
    expect(holder.port.executeCommand).not.toHaveBeenCalled()
    expect(store.commandState('adjust').status).toBe('blocked')

    await store.runCommand('pause', { type: 'production_pause', expectedRevision: 1, payload: {} })
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(1)
  })

  it('Given 没有在生成 When 发起保存类操作 Then 正常发送', async () => {
    holder.port = port(false)
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    await store.loadProduction(true)
    expect(store.writeLocked).toBe(false)
    await store.runCommand('adjust', { type: 'thread_responsibility_adjust', expectedRevision: 1, payload: { action: 'DELETE' } }).catch(() => undefined)
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(1)
  })

  it('Given AI 建议正在生成 When 发起保存类操作 Then 同样只读', async () => {
    holder.port = port(false)
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    let finish!: () => void
    const running = store.withProjectBusy(() => new Promise<void>(resolve => { finish = resolve }))
    expect(store.writeLocked).toBe(true)
    finish()
    await running
    expect(store.writeLocked).toBe(false)
  })

  it('Given AI 建议命令正在执行 When 发起正式写入 Then 写入被拦截直到建议完成', async () => {
    const activePort = port(false)
    let finish!: () => void
    activePort.executeCommand = vi.fn((request: CommandRequest) => {
      if (request.type === 'pitch_regenerate') {
        return new Promise(resolve => {
          finish = () => resolve({ commandId: request.commandId, projectId: 'p', finalStatus: 'accepted' })
        })
      }
      return Promise.resolve({ commandId: request.commandId, projectId: 'p', finalStatus: 'accepted' })
    }) as typeof activePort.executeCommand
    holder.port = activePort
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')

    const generating = store.runCommand('pitch', { type: 'pitch_regenerate', expectedRevision: 1, payload: {} })
    expect(store.writeLocked).toBe(true)
    await expect(store.runCommand('adjust', { type: 'thread_responsibility_adjust', expectedRevision: 1, payload: { action: 'DELETE' } }))
      .rejects.toThrow('PROJECT_WRITE_LOCKED')
    await expect(store.runCommand('start', { type: 'production_start', expectedRevision: 1, payload: { chapterId: 'chapter:next' } }))
      .rejects.toThrow('PROJECT_WRITE_LOCKED')
    expect(activePort.executeCommand).toHaveBeenCalledTimes(1)

    finish()
    await generating
    expect(store.writeLocked).toBe(false)
  })

  it('Given 只读与生产控制回执 When 显示 Then 全部是中文说明', () => {
    expect(productErrorMessage('PROJECT_WRITE_LOCKED')).toContain('只读')
    for (const actionStatus of [
      'PRODUCTION_CONTROL_REQUESTED', 'PRODUCTION_CONTROL_REPLAYED',
      'PRODUCTION_CONTROL_NOT_ACCEPTED', 'PRODUCTION_CONTROL_IDEMPOTENCY_CONFLICT',
      'PRODUCTION_CONTROL_PENDING',
      'PRODUCTION_PAUSED', 'PRODUCTION_CANCELLED',
    ]) {
      const summary = receiptSummary({ finalStatus: 'accepted', actionStatus })
      expect(summary.title).toMatch(/[一-鿿]/)
      expect(summary.detail).not.toContain(actionStatus)
    }
    expect(receiptSummary({ finalStatus: 'accepted', actionStatus: 'PRODUCTION_CONTROL_REQUESTED', control: 'PAUSE' }).detail).toContain('暂停')
    expect(receiptSummary({ finalStatus: 'blocked', actionStatus: 'PRODUCTION_CONTROL_NOT_ACCEPTED', control: 'CANCEL' }).detail).toContain('取消未生效')
    expect(receiptSummary({ finalStatus: 'blocked', actionStatus: 'PRODUCTION_CONTROL_PENDING' }).detail).toContain('等待')
  })

  it('Given 只读 When 渲染顶部提示 Then 显示只读说明', async () => {
    const { default: WriteLockBanner } = await import('../../src/ui/components/WriteLockBanner.vue')
    const host = document.createElement('div')
    const app = createApp(WriteLockBanner, { locked: true })
    app.mount(host)
    await nextTick()
    expect(host.textContent).toContain('本作品正在生成，暂时只读')
    app.unmount()
  })
})
