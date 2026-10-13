import { describe, expect, it } from 'vitest'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

describe('FX-14 删除冲突提示', () => {
  it('Given 回收站已有同名作品 When 删除失败 Then 显示中文原因而不是“项目忙碌”', () => {
    const message = productErrorMessage('project command: PROJECT_TRASH_TARGET_EXISTS')
    expect(message).toContain('回收站')
    expect(message).not.toContain('PROJECT_TRASH_TARGET_EXISTS')
    expect(message).not.toContain('后台任务')
  })
})
