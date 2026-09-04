import { describe, expect, it } from 'vitest'
import {
  commandMessage,
  containsInternalProductToken,
  evidenceDisplayLabel,
  evidenceLabel,
  evidenceSelector,
  productErrorMessage,
  receiptSummary,
  selectEvidenceByIdentity,
  selectEvidenceBySelector,
  sanitizeProductText,
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
    expect(productErrorMessage('连接服务失败，请检查 Endpoint')).toContain('模型服务连接失败')
    expect(productErrorMessage('PROTOCOL / projectId=secret payloadHash=abc')).not.toMatch(/projectId|payloadHash|PROTOCOL/)
    expect(commandMessage({ status: 'failed', message: 'EVIDENCE_GAP' })).toContain('现有信息不足，暂时无法给出可靠结论')
  })

  it('blocks every dynamic internal engineering token while preserving a caller fallback', () => {
    const fallback = '当前信息暂不可展示，请刷新后重试。'
    const inputs = [
      '当前由 Rust 处理，请稍候。',
      '当前 TypeScript 模块异常，请重试。',
      'JavaScript 页面状态异常，请刷新。',
      'Vue 页面状态异常，请刷新。',
      'SQLite 查询失败，请重试。',
      'WebView 当前异常。',
      'IPC 当前异常。',
      'Harness capability query_continuity 当前异常。',
      'capability query_continuity 当前异常。',
      'capability query_knowledge 返回异常。',
      'capability diagnose_continuity 当前异常。',
      'capabilityId=query_continuity 当前异常。',
      '预计只影响第 3 章与开篇评估，不改变已提交 Canon。',
      'Skill ID skill:opening 当前异常。',
      'Runtime Owner 当前不可用。',
      '当前 projectId 为 project:demo。',
      '当前 payloadHash 为 abc123。',
      'stateDigest=abc sourceRefs=def artifactRef=ghi capabilityId=x skillId=y',
      'SELECT * FROM artifacts',
      '/src-tauri/src/commands.rs',
      'B07::PlanningOwner 当前不可用。',
      'P0 需要处理。',
      '["raw", "json"]',
    ]
    for (const input of inputs) {
      expect(containsInternalProductToken(input), input).toBe(true)
      expect(sanitizeProductText(input, fallback), input).toBe(fallback)
      expect(productErrorMessage(input, fallback), input).toBe(fallback)
    }
  })

  it('keeps provider connection errors actionable without opening a generic engineering bypass', () => {
    expect(productErrorMessage('连接服务失败，请检查 Endpoint')).toContain('模型服务连接失败')
    expect(productErrorMessage('API KEY transport Runtime exception')).toContain('模型服务连接失败')
    expect(productErrorMessage('TypeScript provider adapter failed')).toBe('操作未完成，请刷新当前作品后重试。')
  })

  it('keeps natural Chinese dynamic text intact', () => {
    expect(sanitizeProductText('第三章中主角已经知道这件事，但第五章仍按未知状态行动。', '备用')).toBe('第三章中主角已经知道这件事，但第五章仍按未知状态行动。')
    expect(sanitizeProductText('当前卷的冲突集中在人物动机与时间线衔接。', '备用')).toBe('当前卷的冲突集中在人物动机与时间线衔接。')
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

  it('does not expose JSON or internal identifiers in evidence labels', () => {
    expect(evidenceLabel('正式章节依据 · {"chapterId":"chapter:18","artifactId":"artifact:1"}')).toBe('这条事实的原始依据')
    expect(evidenceLabel('第一卷第18章正文依据')).toBe('第一卷第18章正文依据')
  })

  it('keeps evidence identity and raw values separate from duplicate display labels', () => {
    const evidence = [
      { artifactId: 'artifact:evidence:1', version: 3, payloadHash: 'hash-raw-1', label: '正式章节依据 · {"chapterId":"chapter:p:1","level":"CHAPTER"}' },
      { artifactId: 'artifact:evidence:2', version: 4, payloadHash: 'hash-raw-2', label: '正式章节依据 · {"chapterId":"chapter:p:2","level":"CHAPTER"}' },
    ]
    const selectors = evidence.map(evidenceSelector)
    const labels = evidence.map((item, index) => evidenceDisplayLabel(item, index))
    expect(new Set(selectors).size).toBe(2)
    expect(new Set(labels).size).toBe(2)
    expect(labels.join('')).not.toMatch(/[{}[\]]|chapterId|level/)
    expect(selectEvidenceBySelector(evidence, selectors[0])?.payloadHash).toBe('hash-raw-1')
    expect(selectEvidenceBySelector(evidence, selectors[1])?.payloadHash).toBe('hash-raw-2')
  })

  it('restores Evidence by artifact, version and optional payload hash', () => {
    const evidence = [
      { artifactId: 'artifact:A', version: 3, payloadHash: 'hash-X', label: '旧版本依据' },
      { artifactId: 'artifact:A', version: 4, payloadHash: 'hash-Y', label: '当前版本依据' },
    ]
    expect(selectEvidenceByIdentity(evidence, { artifactId: 'artifact:A', version: 4, payloadHash: 'hash-Y' })).toEqual(evidence[1])
    expect(selectEvidenceByIdentity(evidence, { artifactId: 'artifact:A', version: 4, payloadHash: 'hash-X' })).toBeUndefined()
    expect(selectEvidenceByIdentity(evidence, { artifactId: 'artifact:A', version: 3 })).toEqual(evidence[0])
  })

  it('removes internal codes, fields, paths and model self-talk from assistant output', () => {
    const clean = sanitizeAssistantText('作为 AI，根据你提供的上下文，PLAN_STALE owner=B07::PlanningOwner projectId=p1 payloadHash=abc {"sourceRefs":[]}')
    expect(clean).toContain('当前规划已与最新内容不一致')
    expect(clean).not.toMatch(/作为 AI|根据你提供的上下文|PLAN_STALE|owner|B07::|projectId|payloadHash|sourceRefs|\{/)
  })

  it('blocks dynamic Assistant engineering terms instead of returning a damaged sentence', () => {
    const fallbackInputs = [
      'Harness capability query_knowledge 返回 Owner Runtime 异常。',
      'capability diagnose_continuity 当前异常。',
      'Skill ID skill:opening 未加载。',
      '当前 payloadHash=abc，projectId=project:demo。',
      '当前由 Rust / TypeScript Runtime 处理。',
    ]
    for (const input of fallbackInputs) {
      const clean = sanitizeAssistantText(input)
      expect(clean, input).toMatch(/[\u4e00-\u9fff]/)
      expect(clean, input).not.toMatch(/Harness|capability|Owner|Runtime|Skill ID|skill:|payloadHash|projectId|Rust|TypeScript|\{|\[|SELECT/i)
      expect(clean, input).not.toMatch(/当前由\s*\/\s*处理|返回\s*\s*异常/)
    }
  })

  it('keeps natural Assistant Chinese positive fixtures intact', () => {
    expect(sanitizeAssistantText('第三章中主角已经知道这件事，但第五章仍按未知状态行动。')).toBe('第三章中主角已经知道这件事，但第五章仍按未知状态行动。')
    expect(sanitizeAssistantText('当前卷的冲突集中在人物动机与时间线衔接。')).toBe('当前卷的冲突集中在人物动机与时间线衔接。')
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
