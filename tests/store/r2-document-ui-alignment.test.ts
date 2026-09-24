/* @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, reactive } from 'vue'
import ProductionCenter from '../../src/ui/pages/ProductionCenter.vue'
import VolumeWorkspace from '../../src/ui/pages/VolumeWorkspace.vue'
import Completion from '../../src/ui/pages/Completion.vue'
import StorySetup from '../../src/ui/pages/StorySetup.vue'

const harness = vi.hoisted(() => ({ store: null as any, port: null as any }))
vi.mock('vue-router', () => ({ useRoute: () => ({ params: { id: 'p', volumeId: 'v' }, query: {} }), useRouter: () => ({ push: vi.fn() }) }))
vi.mock('../../src/ui/stores/projectStore', () => ({ useProjectStore: () => harness.store }))
vi.mock('../../src/app/providers/runtimePort', () => ({ getFrontendPort: () => harness.port }))
const mounted: Array<ReturnType<typeof createApp>> = []
async function render(component: any) {
  const element = document.createElement('div')
  document.body.appendChild(element)
  const app = createApp(component)
  mounted.push(app)
  app.mount(element)
  for (let i = 0; i < 8; i++) await Promise.resolve()
  await nextTick()
  return element
}
function production(overrides: Record<string, unknown> = {}) {
  const projection = {
    timeline: [], tasks: [], modelRuns: [], activeWork: { detail: '' },
    signing: { status: 'NOT_STARTED', verdict: '' },
    chapterPlan: { ref: { artifactId: 'plan', version: 1 }, chapterGoal: '进入山城', activeTaskRefs: [], continuityActions: [] },
    stale: { status: 'VALID_OR_REQUIRES_RELEVANT_RECHECK', reason: '' },
    jobStatus: null, reconciliation: 'REPLAN_VOLUME_ROUTE', ...overrides,
  }
  harness.store = reactive({
    project: { id: 'p', runState: 'IDLE', blockReason: null, committedChapters: 0 }, production: projection,
    advancedMode: false, commandState: () => ({ status: 'idle', message: '' }),
    loadProduction: vi.fn(async () => harness.store.production), runCommand: vi.fn(),
  })
}
function volume(overrides: Record<string, unknown> = {}) {
  harness.store = reactive({
    currentProjectId: 'p', advancedMode: false,
    volumeDetail: { volume: { id: 'v', number: 1, title: '山城', goal: '找到线索', status: 'READY' }, chapters: { items: [] } },
    planningRoute: { volumeId: 'v', tasks: [{ taskId: 't', class: 'REQUIRED', objective: '找到钥匙' }], taskReality: [], planningPressure: {}, routeVersion: 1, truncated: false, horizonPreview: [], ...overrides },
    volumeLifecycle: null, commandState: () => ({ status: 'idle', message: '' }), runCommand: vi.fn(),
    loadVolumeDetail: vi.fn(async () => harness.store.volumeDetail),
    loadPlanningRoute: vi.fn(async () => harness.store.planningRoute), loadVolumeLifecycle: vi.fn(async () => null),
  })
}

afterEach(() => { mounted.splice(0).forEach(app => app.unmount()); document.body.innerHTML = '' })

describe('R2 文档修订对应的前端行为', () => {
  it('Given 计划仍需相关状态复核 When 展示 Then 不宣称可以使用', async () => {
    production()
    const page = await render(ProductionCenter)
    expect(page.textContent).toContain('等待相关状态复核')
    expect(page.textContent).not.toContain('规划状态：可以使用')
  })
  it('Given 签约已失效 When 展示 Then 明确要求重新评估', async () => {
    production({ signing: { status: 'STALE', verdict: '' }, bookPlan: { status: 'APPROVED' } })
    const page = await render(ProductionCenter)
    expect(page.textContent).toContain('签约依据已失效')
    expect(page.textContent).not.toContain('正式总纲已完成')
  })
  it('Given 评估失败 When 展示 Then 不显示等待三章完成', async () => {
    production({ signing: { status: 'NOT_STARTED', verdict: '', evaluation: { status: 'FAILED', steps: [] } } })
    expect((await render(ProductionCenter)).textContent).toContain('评估未完成')
  })
  it('Given 无长篇计划 When 展示 Then 不显示开篇创建说明', async () => {
    production({ chapterPlan: null, nextChapter: null })
    const page = await render(ProductionCenter)
    expect(page.textContent).toContain('当前没有可执行章节计划')
    expect(page.textContent).not.toContain('再建立第一章规划')
  })
  it('Given 后端返回卷路线重规划 When 展示 Then 保留具体后续动作', async () => {
    production()
    expect((await render(ProductionCenter)).textContent).toContain('需要调整本卷路线')
  })
  it('Given HARD 上下文超限 When 展示 Then 阻止新生产并显示后端原因', async () => {
    production({
      nextChapter: { id: 'c', number: 4, volumeId: 'v', executionMode: 'LONG_FORM' },
      validation: { canExecute: false, context: { status: 'OVER_BUDGET', reason: '当前必需证据超出预算' } },
    })
    const page = await render(ProductionCenter)
    expect(page.textContent).toContain('当前必需证据超出预算')
    expect(Array.from(page.querySelectorAll('button')).some(button => button.textContent?.includes('开始第'))).toBe(false)
  })
  it('Given 缺少任务实际状态 When 展示 Then 不补造未开始状态', async () => {
    volume()
    const page = await render(VolumeWorkspace)
    expect(page.textContent).toContain('实际状态尚未提供')
    expect(page.textContent).toContain('找到钥匙')
  })
  it('Given 卷末预览为空 When 展示 Then 允许空预览并说明暂定属性', async () => {
    volume()
    const page = await render(VolumeWorkspace)
    expect(page.textContent).toContain('暂无后续预览')
    expect(page.textContent).toContain('卷末或书末允许为空')
  })
  it('Given 卷已关闭 When 展示 Then 不能准备新的章节', async () => {
    volume()
    harness.store.volumeDetail.volume.status = 'CLOSED'
    const page = await render(VolumeWorkspace)
    expect(page.querySelector('[data-action="prepare"]')).toBeNull()
  })
  it('Given 完结状态需要复核但各检查均 READY When 确认 Then 仍不能提交', async () => {
    harness.port = {
      queryCompletionStatus: vi.fn(async () => ({ status: 'NEEDS_RECHECK', blockerCount: 0, checks: [{ kind: 'OBLIGATION', status: 'READY' }], completed: false, projectRevision: 1 })),
    }
    const page = await render(Completion)
    const boxes = page.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')
    const last = boxes[boxes.length - 1]
    last.checked = true
    last.dispatchEvent(new Event('change'))
    await nextTick()
    const submit = Array.from(page.querySelectorAll('button')).find(button => button.textContent === '提交全书完结')!
    expect(submit.disabled).toBe(true)
  })
})

describe('R2 完结操作回执与刷新', () => {
  const ready = { status: 'READY', blockerCount: 0, checks: [], completed: false, projectRevision: 1 }
  async function submitCompletion(page: HTMLElement) {
    const boxes = page.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')
    boxes[boxes.length - 1].checked = true
    boxes[boxes.length - 1].dispatchEvent(new Event('change'))
    await nextTick()
    Array.from(page.querySelectorAll('button')).find(button => button.textContent === '提交全书完结')!.click()
    for (let i = 0; i < 12; i++) await Promise.resolve()
    await nextTick()
  }
  it('Given blocked 回执 When 提交结束 Then 不显示已保存成功', async () => {
    harness.port = {
      queryCompletionStatus: vi.fn(async () => ready),
      executeCommand: vi.fn(async (request: any) => ({ commandId: request.commandId, projectId: 'p', finalStatus: 'blocked', message: '还有未完成的正式责任' })),
    }
    const page = await render(Completion)
    await submitCompletion(page)
    expect(page.textContent).toContain('还有未完成的正式责任')
    expect(page.textContent).not.toContain('当前版本的完结状态与确认报告已经保存')
  })
  it('Given committed 回执 When 状态查询失败 Then 仅允许重新查询而不重复提交', async () => {
    harness.port = {
      queryCompletionStatus: vi.fn().mockResolvedValueOnce(ready).mockRejectedValueOnce(new Error('unavailable')),
      executeCommand: vi.fn(async (request: any) => ({ commandId: request.commandId, projectId: 'p', finalStatus: 'committed', receiptId: 'r' })),
    }
    const page = await render(Completion)
    await submitCompletion(page)
    expect(page.textContent).toContain('结果待确认')
    expect(harness.port.executeCommand).toHaveBeenCalledTimes(1)
    harness.port.queryCompletionStatus.mockResolvedValue({ ...ready, completed: true })
    Array.from(page.querySelectorAll('button')).find(button => button.textContent === '再次检查')!.click()
    for (let i = 0; i < 12; i++) await Promise.resolve()
    await nextTick()
    expect(page.textContent).toContain('当前版本已完成完结确认')
    expect(harness.port.executeCommand).toHaveBeenCalledTimes(1)
  })
  it('Given 未执行的规划阶段 When 展示时间线 Then 不声称已经完成', async () => {
    production({ timeline: [{ id: 'plan', order: 2, title: 'PLAN', status: 'pending' }] })
    const page = await render(ProductionCenter)
    expect(page.textContent).not.toContain('已建立本章目标')
    expect(page.textContent).toContain('等待执行')
  })
})


describe('R2 故事核心的用户边界', () => {
  it('Given 既有世界规则 When 保存故事核心 Then 不被前端固定模板覆盖', async () => {
    harness.store = reactive({
      currentProjectId: 'p', storySetup: { status: 'SETUP_SAVED', storyCore: { protagonist: '林青', readerPromise: '破解谜题', worldRules: ['不能复活'], forbiddenReveals: ['幕后身份'], style: '克制', expandableDirections: ['离开山城'] } },
      loadStorySetup: vi.fn(async () => harness.store.storySetup),
      commandState: () => ({ status: 'idle', message: '' }),
      runCommand: vi.fn(async () => ({ finalStatus: 'committed' })),
    })
    const page = await render(StorySetup)
    Array.from(page.querySelectorAll('button')).find(button => button.textContent === '保存故事核心并继续')!.click()
    await Promise.resolve()
    expect(harness.store.runCommand).toHaveBeenCalledWith('story-setup-save', expect.objectContaining({ payload: { storyCore: expect.objectContaining({ worldRules: ['不能复活'], forbiddenReveals: ['幕后身份'], style: '克制', expandableDirections: ['离开山城'] }) } }))
  })
})
