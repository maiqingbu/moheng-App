import { describe, expect, it } from 'vitest'
import { presentAssistantSession, sanitizeAssistantText } from '../../src/ui/presentation/assistantPresentation'

const supportedSession = {
  finding: {
    claim: '第一卷的承诺尚未彻底解决。',
    supportStatus: 'SUPPORTED',
    sourceRefs: ['thread:promise-1', 'chapter:18'],
  },
  calls: [{ capabilityId: 'query_continuity', result: [{ chapterId: 'chapter:18', status: 'OPEN' }] }],
  evidenceLedger: [{ sourceRefs: ['chapter:18'], truncated: false }],
  receipt: { projectRevision: 7 },
} as never

describe('B11 项目助手中文展示层', () => {
  it('should present grounded facts with Chinese evidence drilldown and no raw references', () => {
    const view = presentAssistantSession(supportedSession)
    expect(view.answer).toContain('第一卷的承诺尚未彻底解决')
    expect(view.sections.map(section => section.title)).toEqual(expect.arrayContaining(['当前情况', '依据']))
    expect(view.evidence).toEqual([{ label: '已核验的项目记录', detail: '共 1 条记录，来自相关章节与项目状态。' }])
    expect(view.answer).not.toMatch(/thread:|chapter:|sourceRefs|query_continuity|\{.*\}/)
  })

  it('should say the record is insufficient instead of guessing', () => {
    const view = presentAssistantSession({
      finding: { claim: '没有任何相关记录', supportStatus: 'INSUFFICIENT', sourceRefs: [] },
      calls: [],
      evidenceLedger: [],
      receipt: { projectRevision: 7 },
    } as never)
    expect(view.answer).toBe('现有记录不足以确认这件事，目前只能确认到已查询的范围。')
    expect(view.status).toBe('insufficient')
    expect(view.evidence).toEqual([])
  })

  it('should remove internal codes, JSON, paths, and AI boilerplate from dynamic text', () => {
    const clean = sanitizeAssistantText('作为 AI，根据你提供的上下文，PLAN_STALE /src-tauri/src/commands.rs {"sourceRefs":[]}')
    expect(clean).not.toMatch(/作为 AI|根据你提供的上下文|PLAN_STALE|src-tauri|sourceRefs|\{/) 
    expect(clean).toContain('当前规划需要重新检查')
  })
})
