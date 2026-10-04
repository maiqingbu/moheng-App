/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, h, nextTick, reactive, watch } from 'vue'
import { createMemoryHistory, createRouter, RouterView, useRoute } from 'vue-router'
import ManuscriptReader from '../../src/ui/pages/ManuscriptReader.vue'

const harness = vi.hoisted(() => ({ store: null as any }))
const mounted: Array<{ app: ReturnType<typeof createApp>; root: HTMLElement; router: ReturnType<typeof createRouter> }> = []

vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))
vi.mock('../../src/ui/components/PageHeader.vue', async () => {
  const { h } = await import('vue')
  return {
    default: {
      props: ['title', 'subtitle'],
      setup(props: { title: string }, context: { slots: Record<string, (() => unknown) | undefined> }) {
        return () => h('header', [h('h1', props.title), context.slots.default?.()])
      },
    },
  }
})
vi.mock('../../src/ui/components/CommandFeedback.vue', async () => {
  const { h } = await import('vue')
  return { default: { setup: () => () => h('div') } }
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function settle() {
  for (let index = 0; index < 8; index++) await Promise.resolve()
  await nextTick()
}

function makeStore() {
  const store = reactive({
    currentProjectId: 'project-A',
    readerVolumes: { items: [], nextCursor: null, total: 0 },
    chapterLists: {},
    activeChapter: null,
    chapterVersions: { items: [], nextCursor: null, total: 0 },
    searchResults: { items: [], nextCursor: null, total: 0 },
    commandState: vi.fn(() => ({ status: 'idle', message: '' })),
    loadReaderVolumes: vi.fn(async () => store.readerVolumes),
    loadExportCapability: vi.fn(async (projectId: string = store.currentProjectId): Promise<any> => ({
      available: projectId === 'project-A',
      reason: projectId === 'project-A' ? null : 'NO_CURRENT_FINAL_CHAPTERS',
      supportedFormats: projectId === 'project-A' ? ['TXT'] : [],
      currentFinalChapterCount: projectId === 'project-A' ? 1 : 0,
    })),
    loadLatestExportReceipt: vi.fn(async (_projectId?: string): Promise<any> => ({ status: 'NONE' })),
    createExport: vi.fn(async (..._args: any[]): Promise<any> => ({ status: 'NONE' })),
    queryExportReceipt: vi.fn(async (..._args: any[]): Promise<any> => ({ status: 'NONE' })),
    search: vi.fn(async () => undefined),
    runCommand: vi.fn(),
  })
  return store
}

async function mountReader(store = makeStore()) {
  harness.store = store
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/projects/:id/reader', component: ManuscriptReader },
      { path: '/projects/:id/other', component: { render: () => h('div', 'other route') } },
    ],
  })
  const Host = defineComponent({
    setup() {
      const route = useRoute()
      watch(() => route.params.id, id => {
        if (typeof id === 'string') store.currentProjectId = id
      }, { immediate: true })
      return () => h(RouterView)
    },
  })
  const root = document.createElement('div')
  document.body.append(root)
  const app = createApp(Host)
  app.use(router)
  await router.push('/projects/project-A/reader')
  await router.isReady()
  app.mount(root)
  await settle()
  const mountedReader = { app, root, router }
  mounted.push(mountedReader)
  return mountedReader
}

function exportButton(root: HTMLElement) {
  const button = Array.from(root.querySelectorAll('button')).find(item => item.textContent?.includes('导出正式成品 TXT'))
  if (!button) throw new Error('正式导出按钮未渲染')
  return button as HTMLButtonElement
}

const completedReceipt = (projectId: string) => ({
  schema: 'moheng.export-receipt.v1',
  status: 'COMPLETED' as const,
  receiptId: `export-receipt:${projectId}`,
  requestId: `request:${projectId}`,
  projectId,
  format: 'TXT' as const,
  fileName: 'book.txt',
  finalPath: `/${projectId}/exports/book.txt`,
  createdAtEpochMs: 1,
  elapsedMs: 1,
  sourceProjectRevision: 0,
  chapters: [],
  sourceCharacterCount: 1,
  outputCharacterCount: 1,
  outputByteCount: 1,
  sha256: 'a'.repeat(64),
  maxChunkCharacters: 1,
  maxWorkingBufferBytes: 1,
})

afterEach(() => {
  mounted.splice(0).forEach(({ app, root }) => {
    app.unmount()
    root.remove()
  })
  document.body.innerHTML = ''
  vi.useRealTimers()
})

describe('Reader export project and async request binding', () => {
  it('does not show project A receipt or capability after a late create response arrives in project B', async () => {
    const pending = deferred<any>()
    const store = makeStore()
    store.createExport.mockReturnValue(pending.promise)
    const mountedReader = await mountReader(store)
    expect(exportButton(mountedReader.root).disabled).toBe(false)

    exportButton(mountedReader.root).click()
    await settle()
    await mountedReader.router.push('/projects/project-B/reader')
    await settle()
    pending.resolve(completedReceipt('project-A'))
    await settle()

    expect({
      currentProject: store.currentProjectId,
      capabilityRefreshedForB: store.loadExportCapability.mock.calls.some(([id]) => id === 'project-B'),
      createBoundToA: store.createExport.mock.calls.every(([requestId, format, id]) => id === 'project-A'),
      exportDisabled: exportButton(mountedReader.root).disabled,
      projectAReceiptVisible: mountedReader.root.textContent?.includes('/project-A/exports/book.txt'),
    }).toEqual({
      currentProject: 'project-B',
      capabilityRefreshedForB: true,
      createBoundToA: true,
      exportDisabled: true,
      projectAReceiptVisible: false,
    })
  })

  it('ignores a late project A capability after routing to B', async () => {
    const lateCapability = deferred<any>()
    const lateLatest = deferred<any>()
    const store = makeStore()
    store.loadExportCapability.mockImplementation((projectId = store.currentProjectId) =>
      projectId === 'project-A' ? lateCapability.promise : Promise.resolve({
        available: false, reason: 'NO_CURRENT_FINAL_CHAPTERS', supportedFormats: [], currentFinalChapterCount: 0,
      }))
    store.loadLatestExportReceipt.mockImplementation((projectId = store.currentProjectId) =>
      projectId === 'project-A' ? lateLatest.promise : Promise.resolve({ status: 'NONE' as const }))
    const mountedReader = await mountReader(store)

    await mountedReader.router.push('/projects/project-B/reader')
    await settle()
    lateCapability.resolve({ available: true, reason: null, supportedFormats: ['TXT'], currentFinalChapterCount: 1 })
    await settle()

    expect(store.currentProjectId).toBe('project-B')
    expect(exportButton(mountedReader.root).disabled).toBe(true)
    expect(mountedReader.root.textContent).not.toContain('/project-A/exports/book.txt')
  })

  it('ignores a late project A latest receipt after routing to B', async () => {
    const lateLatest = deferred<any>()
    const store = makeStore()
    store.loadLatestExportReceipt.mockImplementation((projectId = store.currentProjectId) =>
      projectId === 'project-A' ? lateLatest.promise : Promise.resolve({ status: 'NONE' as const }))
    const mountedReader = await mountReader(store)

    await mountedReader.router.push('/projects/project-B/reader')
    await settle()
    lateLatest.resolve(completedReceipt('project-A'))
    await settle()

    expect({
      currentProject: store.currentProjectId,
      exportDisabled: exportButton(mountedReader.root).disabled,
      projectAReceiptVisible: mountedReader.root.textContent?.includes('/project-A/exports/book.txt'),
    }).toEqual({ currentProject: 'project-B', exportDisabled: true, projectAReceiptVisible: false })
  })

  it('queries a lost create response using the original project and request identity', async () => {
    const store = makeStore()
    const receipt = completedReceipt('project-A')
    store.createExport.mockImplementation(async (requestId: string) => {
      store.queryExportReceipt.mockResolvedValue({ ...receipt, requestId })
      throw new Error('IPC response lost')
    })
    const mountedReader = await mountReader(store)

    exportButton(mountedReader.root).click()
    await settle()

    const requestId = store.createExport.mock.calls[0]?.[0]
    expect(store.queryExportReceipt.mock.calls).toContainEqual([requestId, 'project-A'])
    expect(mountedReader.root.textContent).toContain('/project-A/exports/book.txt')
  })

  it('polls only the captured project and ignores a late poll after switching projects', async () => {
    vi.useFakeTimers()
    const lateStatus = deferred<any>()
    const store = makeStore()
    store.createExport.mockReturnValue(new Promise(() => undefined))
    store.queryExportReceipt.mockReturnValue(lateStatus.promise)
    const mountedReader = await mountReader(store)

    exportButton(mountedReader.root).click()
    await settle()
    await vi.advanceTimersByTimeAsync(150)
    expect(store.queryExportReceipt.mock.calls[0]?.[1]).toBe('project-A')

    await mountedReader.router.push('/projects/project-B/reader')
    await settle()
    lateStatus.resolve(completedReceipt('project-A'))
    await settle()

    expect(store.queryExportReceipt.mock.calls.every(([, projectId]) => projectId === 'project-A')).toBe(true)
    expect(mountedReader.root.textContent).not.toContain('/project-A/exports/book.txt')
    expect(vi.getTimerCount()).toBe(0)
  })

  it('reloads latest receipt for A when returning from B', async () => {
    const store = makeStore()
    store.loadLatestExportReceipt.mockImplementation(async (projectId = store.currentProjectId) =>
      projectId === 'project-A' ? completedReceipt('project-A') : { status: 'NONE' as const })
    const mountedReader = await mountReader(store)

    await mountedReader.router.push('/projects/project-B/reader')
    await settle()
    expect(mountedReader.root.textContent).not.toContain('/project-A/exports/book.txt')
    await mountedReader.router.push('/projects/project-A/reader')
    await settle()

    expect(store.loadLatestExportReceipt.mock.calls.some(([projectId]) => projectId === 'project-A')).toBe(true)
    expect(mountedReader.root.textContent).toContain('/project-A/exports/book.txt')
  })

  it('stops polling when the Reader route is left while export is pending', async () => {
    vi.useFakeTimers()
    const pending = deferred<any>()
    const store = makeStore()
    store.createExport.mockReturnValue(pending.promise)
    const mountedReader = await mountReader(store)
    exportButton(mountedReader.root).click()
    await settle()
    expect(vi.getTimerCount()).toBeGreaterThan(0)

    await mountedReader.router.push('/projects/project-B/other')
    await settle()
    await vi.advanceTimersByTimeAsync(500)

    expect(store.queryExportReceipt).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
    pending.resolve(completedReceipt('project-A'))
    await settle()
    expect(mountedReader.root.textContent).not.toContain('/project-A/exports/book.txt')
  })
})
