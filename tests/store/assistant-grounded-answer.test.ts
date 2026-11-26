import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assistantHeading, presentAssistantSession } from '../../src/ui/presentation/assistantPresentation'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

const session = (finding: Record<string, unknown>, answer: unknown) => ({
  plan: { steps: [{ stepId: 'step:1', capabilityId: 'query_knowledge' }, { stepId: 'step:2', capabilityId: 'query_evidence' }] },
  calls: [],
  evidenceLedger: [],
  receipt: {},
  finding,
  answer,
}) as never

describe('助手按查到的记录回答', () => {
  it('Given 模型写出带依据的回答 When 展示 Then 显示回答本身和查了哪几步，而不是原问题', () => {
    const view = presentAssistantSession(session(
      { supportStatus: 'SUPPORTED', claim: '柳青堂还不知道沈无悔的真名。第 3 章他只看见了半枚玉符。' },
      { planner: 'MODEL', sentences: [{ text: '柳青堂还不知道沈无悔的真名。', sources: ['fact:2'], inferred: false }, { text: '第 3 章他只看见了半枚玉符。', sources: ['event:7'], inferred: false }] },
    ))
    expect(view.answer).toBe('柳青堂还不知道沈无悔的真名。第 3 章他只看见了半枚玉符。')
    expect(view.steps).toEqual(['查人物知道什么', '查正文原句'])
  })

  it('Given 查不到 When 展示 Then 直说查不到，标题也不说已核对', () => {
    const view = presentAssistantSession(session(
      { supportStatus: 'INSUFFICIENT', claim: '现有作品记录里查不到能回答这个问题的内容。' },
      { planner: 'MODEL', sentences: [] },
    ))
    expect(view.answer).toBe('现有作品记录里查不到能回答这个问题的内容。')
    expect(assistantHeading(view.status)).toBe('记录里查不到')
    expect(assistantHeading('supported')).toBe('有依据的回答')
    expect(assistantHeading('inferred')).toBe('有依据的推断')
    expect(assistantHeading('conflicting')).toBe('记录之间不一致')
  })

  it('Given 助手面板 When 检查 Then 标题按支持程度写，并能展开查询步骤', () => {
    const dock = source('src/ui/components/ProjectAssistantDock.vue')
    expect(dock).toContain('assistantHeading(presentation.status)')
    expect(dock).toContain('presentation.steps')
    expect(dock).not.toContain("'已完成当前记录核对'")
  })
})
