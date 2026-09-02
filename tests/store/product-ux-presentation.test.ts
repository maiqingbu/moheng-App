import { describe, expect, it } from 'vitest'
import {
  commandMessage,
  productErrorMessage,
  receiptSummary,
  scopeLabel,
  statusLabel,
} from '../../src/ui/presentation/productLanguage'
import { presentAssistantSession, sanitizeAssistantText } from '../../src/ui/presentation/assistantPresentation'

describe('Product UX 展示语义映射', () => {
  it('maps internal runtime states to actionable Chinese without changing the protocol value', () => {
    expect(statusLabel('PLAN_STALE')).toBe('当前规划已与最新内容不一致，需要重新确认或更新')
    expect(statusLabel('AUTHORIZATION_STALE')).toBe('作品状态已经变化，请重新确认修复方案')
    expect(statusLabel('EVIDENCE_GAP')).toBe('现有信息不足，暂时无法给出可靠结论')
    expect(statusLabel('BLOCKED')).toBe('暂时无法继续')
    expect(scopeLabel('GLOBAL')).toBe('全书范围')
    expect(scopeLabel('WIDE')).toBe('较大范围')
    expect(scopeLabel('LOCAL')).toBe('局部范围')
  })

  it('turns protocol and provider errors into actionable user messages', () => {
    expect(productErrorMessage('PLAN_STALE')).toContain('重新确认')
    expect(productErrorMessage('PROTOCOL / PROJECT_BINDING_ERROR')).toContain('作品状态')
    expect(productErrorMessage('AUTHORIZATION_STALE')).toContain('重新确认修复方案')
    expect(productErrorMessage('连接服务失败，请检查 Endpoint')).toContain('连接服务失败')
    expect(productErrorMessage('PROTOCOL / projectId=secret payloadHash=abc')).not.toMatch(/projectId|payloadHash|PROTOCOL/)
    expect(commandMessage({ status: 'failed', message: 'EVIDENCE_GAP' })).toBe('现有信息不足，暂时无法给出可靠结论')
  })

  it('presents receipts as Chinese outcome summaries while preserving failure meaning', () => {
    expect(receiptSummary({ finalStatus: 'committed', message: 'done', result: { status: 'VERIFIED' } })).toEqual({
      status: 'success',
      title: '已完成并核对',
      detail: '操作已经完成，结果已重新核对。',
    })
    expect(receiptSummary({ finalStatus: 'failed', message: 'AUTHORIZATION_STALE' })).toEqual({
      status: 'failed',
      title: '未完成',
      detail: '作品状态已经变化，请重新确认修复方案。',
    })
  })

  it('removes internal codes, fields, paths and model self-talk from assistant output', () => {
    const clean = sanitizeAssistantText('作为 AI，根据你提供的上下文，PLAN_STALE owner=B07::PlanningOwner projectId=p1 payloadHash=abc {"sourceRefs":[]}')
    expect(clean).toContain('当前规划已与最新内容不一致')
    expect(clean).not.toMatch(/作为 AI|根据你提供的上下文|PLAN_STALE|owner|B07::|projectId|payloadHash|sourceRefs|\{/)
  })

  it('keeps assistant evidence and diagnosis in author language', () => {
    const view = presentAssistantSession({
      finding: { claim: '当前规划为 PLAN_STALE，owner=B07::PlanningOwner。', supportStatus: 'SUPPORTED', sourceRefs: ['artifact:1'] },
      calls: [{ capabilityId: 'query_plan_route', callId: 'call:1', result: [{ chapterId: 'chapter:18', status: 'BLOCKED', payloadHash: 'x' }] }],
      evidenceLedger: [{ callId: 'call:1', sourceRefs: ['artifact:1'], startedAt: '2026-09-02T02:00:00Z', finishedAt: '2026-09-02T02:00:00Z' }],
      receipt: { projectRevision: 7 },
    } as never)
    expect(JSON.stringify(view)).not.toMatch(/PLAN_STALE|B07::|payloadHash|artifact:|query_plan_route/)
    expect(JSON.stringify(view)).toContain('需要重新确认或更新')
  })
})
