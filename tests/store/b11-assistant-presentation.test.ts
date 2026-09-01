import { describe, expect, it } from 'vitest'
import { presentAssistantSession, sanitizeAssistantText } from '../../src/ui/presentation/assistantPresentation'

const supportedSession = {
  finding: {
    claim: '第一卷的承诺尚未彻底解决。',
    supportStatus: 'SUPPORTED',
    sourceRefs: ['thread:promise-1', 'chapter:18'],
  },
  calls: [{ callId: 'call:continuity', capabilityId: 'query_continuity', result: [{ chapterId: 'chapter:18', status: 'OPEN' }] }],
  evidenceLedger: [{ callId: 'call:continuity', sourceRefs: ['chapter:18'], truncated: false, startedAt: '2026-09-02T02:00:00Z', finishedAt: '2026-09-02T02:00:00Z' }],
  receipt: { projectRevision: 7 },
} as never

describe('B11 项目助手中文展示层', () => {
  it('should present grounded facts with Chinese evidence drilldown and no raw references', () => {
    const view = presentAssistantSession(supportedSession)
    expect(view.answer).toContain('第一卷的承诺尚未彻底解决')
    expect(view.sections.map(section => section.title)).toEqual(expect.arrayContaining(['当前情况', '依据']))
    expect(view.evidence).toEqual([{
      label: '剧情线1',
      detail: '剧情线当前状态已查询',
      source: '第 18 章',
      record: '剧情线：进行中',
      currentStatus: '进行中',
      related: '相关剧情线',
      recordedAt: '2026-09-02T02:00:00Z',
    }])
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

  it('should expose grounded record details instead of only a source count', () => {
    const view = presentAssistantSession({
      finding: { claim: '角色知道这件事。', supportStatus: 'SUPPORTED', sourceRefs: ['fact:1'] },
      calls: [{ capabilityId: 'query_knowledge', callId: 'call:knowledge', result: [{ knowerId: 'character:lin', epistemicState: 'KNOWS', fromChapterId: 'chapter:7', storyTime: '夜里' }] }],
      evidenceLedger: [{ callId: 'call:knowledge', sourceRefs: ['fact:1'], startedAt: '2026-09-02T02:00:00Z', finishedAt: '2026-09-02T02:00:00Z' }],
      receipt: { projectRevision: 7 },
    } as never)
    expect(view.evidence[0]).toMatchObject({
      source: '第 7 章',
      record: '角色知识：已知',
      currentStatus: '已知',
      related: '相关角色与事实',
      recordedAt: '夜里',
    })
    expect(JSON.stringify(view.evidence)).not.toMatch(/character:|fact:|chapter:/)
  })
})
