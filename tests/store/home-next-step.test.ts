import { describe, expect, it } from 'vitest'
import { homeNextStep } from '../../src/ui/presentation/homeNextStep'

const project = (runState: string, phase = 'LONG_FORM', blockReason: string | null = null) => ({ runState, phase, blockReason }) as never

describe('首页的下一步', () => {
  it('Given 有待决定 When 打开首页 Then 先去处理，不管写作状态', () => {
    expect(homeNextStep(project('ACTIVE'), 2, 3)).toMatchObject({ title: '先处理 2 件待决定', path: '/decisions' })
  })
  it('Given 写作停了 When 打开首页 Then 说出原因并带去写作页', () => {
    expect(homeNextStep(project('BLOCKED', 'LONG_FORM', 'BUDGET_LIMIT'), 0, 3)).toMatchObject({ title: '写作停下来了', note: '原因：预算达到上限。', path: '/production' })
  })
  it('Given 方向还没定 When 打开首页 Then 去定方向', () => {
    expect(homeNextStep(project('IDLE', 'PITCHING'), 0, null)).toMatchObject({ path: '/directions' })
  })
  it('Given 第 3 章已入库 When 打开首页 Then 写第 4 章', () => {
    expect(homeNextStep(project('IDLE'), 0, 3)).toMatchObject({ title: '写第 4 章', path: '/production' })
    expect(homeNextStep(project('IDLE'), 0, null)).toMatchObject({ title: '写第 1 章' })
  })
})
