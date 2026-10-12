/* @vitest-environment happy-dom */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import type { CommandRequest } from '../../src/application/contracts'
import CommandFeedback from '../../src/ui/components/CommandFeedback.vue'
import { commandMessage, productErrorMessage } from '../../src/ui/presentation/productLanguage'

const holder = vi.hoisted(() => ({ port: null as any }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => holder.port }))

const busyText = '项目正在处理后台任务，请稍后再试'

function busyPort() {
  return {
    queryProjectOverview: vi.fn(async () => ({ project: { id: 'p', revision: 1 }, recentChapters: [] })),
    queryProjectLibrary: vi.fn(async () => ({ projects: [], totals: {} })),
    executeCommand: vi.fn()
      .mockRejectedValueOnce(new Error('project command: PROJECT_BUSY'))
      .mockImplementation(async (request: CommandRequest) => ({
        commandId: request.commandId, projectId: 'p', finalStatus: 'rejected', receiptId: 'r', message: '', acceptedAt: '',
      })),
  }
}

let pinia = createPinia()
beforeEach(() => { pinia = createPinia(); setActivePinia(pinia); vi.resetModules() })

describe('FX-10 项目忙碌提示', () => {
  it('Given 后端返回 PROJECT_BUSY When 显示错误 Then 只出现中文提示，不出现英文原文或错误码', () => {
    expect(productErrorMessage('project command: PROJECT_BUSY')).toContain(busyText)
    expect(productErrorMessage('project writer already open')).not.toContain('project writer already open')
    const message = commandMessage({ status: 'failed', message: 'PROJECT_BUSY' })
    expect(message).toContain(busyText)
    expect(message).not.toContain('PROJECT_BUSY')
  })

  it('Given 写命令因 PROJECT_BUSY 失败 When 点击重试 Then 以新的命令 id 重新提交同一请求', async () => {
    holder.port = busyPort()
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    await store.openProject('p')
    const request: CommandRequest = { type: 'thread_responsibility_adjust', projectId: 'p', expectedRevision: 1, commandId: 'cmd-original', idempotencyKey: 'key-original', payload: { action: 'DELETE' } }

    await expect(store.runCommand('adjust', request)).rejects.toThrow('PROJECT_BUSY')
    expect(store.commandState('adjust')).toMatchObject({ status: 'failed', retryable: true })

    await store.retryCommand('adjust')
    expect(holder.port.executeCommand).toHaveBeenCalledTimes(2)
    const [first] = holder.port.executeCommand.mock.calls[0]
    const [second] = holder.port.executeCommand.mock.calls[1]
    expect(second.type).toBe(first.type)
    expect(second.payload).toEqual(first.payload)
    expect(second.commandId).not.toBe(first.commandId)
    expect(second.idempotencyKey).not.toBe(first.idempotencyKey)
    expect(store.commandState('adjust').retryable).toBeFalsy()
  })

  it('Given 命令反馈处于 PROJECT_BUSY When 渲染 Then 显示中文提示和重试按钮，点击后调用重试', async () => {
    holder.port = busyPort()
    const { useProjectStore } = await import('../../src/ui/stores/projectStore')
    const store = useProjectStore()
    const retry = vi.spyOn(store, 'retryCommand').mockResolvedValue(undefined as any)
    const host = document.createElement('div')
    const app = createApp(CommandFeedback, {
      state: { status: 'failed', message: 'project command: PROJECT_BUSY', retryable: true, retryKey: 'adjust' },
    })
    app.use(pinia)
    app.mount(host)
    await nextTick()

    expect(host.textContent).toContain(busyText)
    expect(host.textContent).not.toContain('PROJECT_BUSY')
    expect(host.textContent).not.toContain('project writer already open')
    const button = [...host.querySelectorAll('button')].find(item => item.textContent?.includes('重试'))
    expect(button).toBeTruthy()
    button!.click()
    await vi.waitFor(() => expect(retry).toHaveBeenCalledWith('adjust'))
    app.unmount()
  })
})
