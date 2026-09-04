import { describe, expect, it, vi } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'
import type { CommandReceipt, CommandRequest } from '../../src/application/contracts'
import {
  createGroundedAssistantRetcon,
  createRetconPlan,
  selectGroundedRetconContext,
} from '../../src/ui/flows/b12RetconFlow'
import {
  containsInternalProductToken,
  evidenceDisplayLabel,
  evidenceSelector,
  productErrorMessage,
  sanitizeProductText,
  selectEvidenceByIdentity,
  selectEvidenceBySelector,
} from '../../src/ui/presentation/productLanguage'
import { sanitizeAssistantText } from '../../src/ui/presentation/assistantPresentation'

const fallback = '当前信息暂不可展示，请刷新当前作品后重试。'
const forbiddenTokens = [
  'Rust', 'TypeScript', 'JavaScript', 'Vue', 'Pinia', 'Tauri', 'IPC', 'SQLite', 'SQL', 'WebView', 'Keychain',
  'Runtime', 'Owner', 'Repository', 'Artifact', 'Harness', 'capability', 'Skill ID', 'skill:opening',
  'projectId', 'payloadHash', 'stateDigest', 'sourceRefs', 'artifactRef', 'capabilityId', 'skillId',
  'PLAN_STALE', 'B12_', 'P0', 'B07::', '/src-tauri/',
]

const receipt = (request: CommandRequest, result: Record<string, unknown> = {}): CommandReceipt => ({
  commandId: request.commandId ?? request.type,
  projectId: request.projectId,
  resultProjectId: request.projectId,
  finalStatus: 'committed',
  message: '',
  acceptedAt: '',
  receiptId: `receipt:${request.type}`,
  result,
})

function writeEvidence(name: string, value: unknown) {
  const directory = process.env.R1R1_BEHAVIOR_EVIDENCE_DIR
  if (!directory) return
  fs.mkdirSync(directory, { recursive: true })
  fs.writeFileSync(path.join(directory, name), `${JSON.stringify(value, null, 2)}\n`)
}

describe('R1-R1 行为证据', () => {
  it('records fail-closed dynamic language and Assistant fixtures from production functions', () => {
    const productInputs = [
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
    const assistantInputs = [
      'Harness capability query_knowledge 返回 Owner Runtime 异常。',
      'capability diagnose_continuity 当前异常。',
      'Skill ID skill:opening 未加载。',
      '当前 payloadHash=abc，projectId=project:demo。',
      '当前由 Rust / TypeScript Runtime 处理。',
    ]
    const rows = productInputs.map(input => {
      const actual = sanitizeProductText(input, fallback)
      const errorActual = productErrorMessage(input, fallback)
      expect(containsInternalProductToken(input), input).toBe(true)
      expect(actual, input).toBe(fallback)
      expect(errorActual, input).toBe(fallback)
      return {
        input,
        function: 'sanitizeProductText + productErrorMessage',
        expectedBehavior: '返回调用方提供的安全中文 fallback，不泄漏内部术语',
        actualOutput: actual,
        forbiddenTokensFound: forbiddenTokens.filter(token => input.includes(token)),
        status: 'PASS',
      }
    })
    const assistantRows = assistantInputs.map(input => {
      const actual = sanitizeAssistantText(input)
      expect(actual).toMatch(/[\u4e00-\u9fff]/)
      expect(actual).not.toMatch(/Rust|TypeScript|Harness|capability|Owner|Runtime|Skill ID|skill:|payloadHash|projectId|\{|\[|SELECT/i)
      return {
        input,
        function: 'sanitizeAssistantText',
        expectedBehavior: '返回完整自然中文，不泄漏内部术语、字段、JSON 或 SQL',
        actualOutput: actual,
        forbiddenTokensFound: forbiddenTokens.filter(token => input.includes(token)),
        status: 'PASS',
      }
    })
    const positives = [
      '第三章中主角已经知道这件事，但第五章仍按未知状态行动。',
      '当前卷的冲突集中在人物动机与时间线衔接。',
    ].map(input => {
      expect(sanitizeProductText(input, '备用')).toBe(input)
      expect(sanitizeAssistantText(input)).toBe(input)
      return { input, productOutput: input, assistantOutput: input, status: 'PASS' }
    })
    writeEvidence('R1R1_DYNAMIC_LANGUAGE_FIXTURES.json', {
      schema: 'r1r1-dynamic-language-fixtures',
      product: rows,
      assistant: assistantRows,
      positive: positives,
      status: 'PASS',
    })
  })

  it('records raw Evidence command preservation and stable display selection', async () => {
    const rawEvidence = {
      artifactId: 'artifact:evidence:1',
      version: 3,
      payloadHash: 'hash-raw-1',
      label: '正式章节依据 · {"chapterId":"chapter:p:1","level":"CHAPTER"}',
      supportsFactId: 'fact:1',
      relationship: 'FACT_SOURCE' as const,
    }
    const secondEvidence = { ...rawEvidence, version: 4, payloadHash: 'hash-raw-2', label: '正式章节依据 · 当前版本' }
    const context = {
      facts: [{ factId: 'fact:1', label: '第一卷核心事实', current: true }],
      evidence: [rawEvidence, secondEvidence],
    }
    const calls: CommandRequest[] = []
    const execute = vi.fn(async (_key: string, request: CommandRequest) => {
      calls.push(request)
      if (request.type === 'impact_preflight') return receipt(request, { impactDigest: 'i'.repeat(64), status: 'PREFLIGHTED' })
      if (request.type === 'retcon_plan_create') return receipt(request, { retconPlanId: 'plan:1', planDigest: 'p'.repeat(64), approvedScopeClass: 'LOCAL', riskClass: 'LOW' })
      return receipt(request)
    })
    const selected = selectGroundedRetconContext(context, '事实改成新内容')
    const draft = await createGroundedAssistantRetcon({ execute, queryRetconContext: vi.fn(async () => context) }, 'p1', '事实改成新内容')
    await createRetconPlan(execute, draft)
    const proposalEvidence = calls.find(request => request.type === 'change_request_create')?.payload?.requestedChange?.evidenceRef
    const planEvidence = calls.find(request => request.type === 'retcon_plan_create')?.payload?.evidenceRefs?.[0]
    const selectors = context.evidence.map(evidenceSelector)
    const displayLabels = context.evidence.map((item, index) => evidenceDisplayLabel(item, index))
    expect(selected.evidence).toEqual(rawEvidence)
    expect(proposalEvidence).toEqual(rawEvidence)
    expect(planEvidence).toEqual(rawEvidence)
    expect(displayLabels.join('')).not.toMatch(/[{}[\]]|chapterId|level/)
    expect(new Set(displayLabels).size).toBe(2)
    expect(selectEvidenceBySelector(context.evidence, selectors[0])?.payloadHash).toBe('hash-raw-1')
    expect(selectEvidenceBySelector(context.evidence, selectors[1])?.payloadHash).toBe('hash-raw-2')
    expect(selectEvidenceByIdentity(context.evidence, { artifactId: 'artifact:evidence:1', version: 4, payloadHash: 'hash-raw-2' })).toEqual(secondEvidence)
    expect(selectEvidenceByIdentity(context.evidence, { artifactId: 'artifact:evidence:1', version: 4, payloadHash: 'hash-raw-1' })).toBeUndefined()
    writeEvidence('R1R1_B12_EVIDENCE_SEPARATION.json', {
      schema: 'r1r1-b12-evidence-separation',
      rawInputEvidence: rawEvidence,
      uiDisplayLabel: displayLabels[0],
      capturedCommandPayloadEvidence: proposalEvidence,
      capturedPlanPayloadEvidence: planEvidence,
      identitySelector: selectors[0],
      rawEqualsPayload: JSON.stringify(proposalEvidence) === JSON.stringify(rawEvidence),
      displayDoesNotLeakRawJson: !/[{}[\]]|chapterId|level/.test(displayLabels[0]),
      selectionUsesDisplayLabelAsIdentity: false,
      duplicateDisplayLabelsAreDistinct: new Set(displayLabels).size === 2,
      selectedPayloadHashes: selectors.map(selector => selectEvidenceBySelector(context.evidence, selector)?.payloadHash),
      compositeIdentityRestore: { artifactId: secondEvidence.artifactId, version: secondEvidence.version, payloadHash: secondEvidence.payloadHash },
      status: 'PASS',
    })
  })
})
