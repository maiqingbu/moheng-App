import type { SupportSessionProjection } from '../../application/contracts'

export type AssistantViewStatus = 'supported' | 'insufficient' | 'conflicting' | 'inferred'

export interface AssistantSection {
  title: '当前情况' | '原因' | '依据' | '建议'
  body: string
}

export interface AssistantEvidenceItem {
  label: string
  detail: string
  source?: string
  record?: string
  currentStatus?: string
  related?: string
  recordedAt?: string
}

export interface AssistantPresentation {
  answer: string
  status: AssistantViewStatus
  sections: AssistantSection[]
  evidence: AssistantEvidenceItem[]
}

const boilerplate = [
  '作为 AI……',
  '作为AI……',
  '根据你提供的上下文，',
  '以下是详细分析：',
  '希望以上内容对你有所帮助。',
  '如有需要我可以继续。',
]

const internalStatus = /PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED/g
const internalReference = /\/?(?:[A-Za-z0-9_.-]+\/)+[A-Za-z0-9_.-]+|(?:sourceRefs|artifactRef|stateDigest|operationDigest)\s*[:=][^\s,，。；;]*/g
const jsonBlock = /\{[^{}]*\}/g

export function sanitizeAssistantText(value: unknown): string {
  let text = typeof value === 'string' ? value : ''
  for (const phrase of boilerplate) text = text.replaceAll(phrase, '')
  text = text
    .replace(/作为\s*AI[：:，,。]?/g, '')
    .replace(/根据你提供的上下文[：:，,]?/g, '')
    .replace(internalStatus, '当前规划需要重新检查')
    .replace(jsonBlock, '')
    .replace(internalReference, '')
    .replace(/\b(?:SELECT|INSERT|UPDATE|DELETE|FROM|WHERE)\b/gi, '')
    .replace(/\s{2,}/g, ' ')
    .trim()
  return text
}

function statusOf(finding: Record<string, unknown>): AssistantViewStatus {
  const status = finding.supportStatus
  if (status === 'CONFLICTING') return 'conflicting'
  if (status === 'INFERRED') return 'inferred'
  if (status === 'INSUFFICIENT') return 'insufficient'
  return 'supported'
}

function recordValue(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function firstRecords(value: unknown): Record<string, unknown>[] {
  if (Array.isArray(value)) return value.map(recordValue).filter(item => Object.keys(item).length > 0).slice(0, 3)
  const record = recordValue(value)
  return Object.keys(record).length > 0 ? [record] : []
}

function stringField(record: Record<string, unknown>, ...keys: string[]): string | undefined {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'string' && value.trim()) return value.trim()
  }
  return undefined
}

function chapterSource(record: Record<string, unknown>): string | undefined {
  const chapter = stringField(record, 'chapterId', 'finalChapterId', 'fromChapterId', 'lastTouchedChapterId')
  const number = chapter?.match(/(?:chapter:|第\s*)(\d+)/i)?.[1]
  return number ? `第 ${number} 章` : undefined
}

function statusLabel(value: string | undefined): string | undefined {
  if (!value) return undefined
  return ({
    KNOWS: '已知',
    BELIEVES: '相信',
    CONCEALS: '刻意隐瞒',
    UNKNOWN: '未知',
    OPEN: '进行中',
    SUSPENDED: '暂缓',
    RESOLVED: '已解决',
    PASS: '已通过',
    VALID: '有效',
    PLAN_STALE: '需要重新整理',
  } as Record<string, string>)[value] ?? value
}

function evidenceRecord(capability: string, record: Record<string, unknown>): string {
  const subject = stringField(record, 'character', 'knowerId', 'entityId')
  const status = statusLabel(stringField(record, 'epistemicState', 'state', 'status'))
  if (capability === 'query_knowledge') return `${subject ? '角色知识' : '知识记录'}${status ? `：${status}` : '已查询'}`
  if (capability === 'query_continuity') return `剧情线${status ? `：${status}` : '已查询'}`
  if (capability === 'query_plan_route' || capability === 'diagnose_planning_constraints') return `当前卷规划${status ? `：${status}` : '已查询'}`
  if (capability === 'query_evidence') return '关联证据记录已查询'
  if (capability === 'query_events') return `章节事件${status ? `：${status}` : '已查询'}`
  if (capability === 'query_context_manifest') return '当前上下文清单已查询'
  return '当前作品记录已查询'
}

function evidenceDetail(capability: string, record: Record<string, unknown>): string {
  const claim = sanitizeAssistantText(stringField(record, 'claim', 'description', 'detail', 'title'))
  if (claim) return claim
  if (capability === 'query_continuity') return '剧情线当前状态已查询'
  if (capability === 'query_knowledge') return '角色知识状态已查询'
  if (capability === 'query_plan_route' || capability === 'diagnose_planning_constraints') return '当前卷规划已查询'
  if (capability === 'query_evidence') return '章节证据已查询'
  if (capability === 'query_events') return '章节事件已查询'
  return '已从当前作品记录中核对到这条依据。'
}

function buildEvidence(session: SupportSessionProjection): AssistantEvidenceItem[] {
  const calls = session.calls ?? []
  const ledger = session.evidenceLedger ?? []
  const rows: AssistantEvidenceItem[] = []
  for (const entry of ledger) {
    const call = calls.find(item => item.callId === entry.callId)
    const records = firstRecords(call?.result)
    records.forEach((record, index) => {
      const capability = typeof call?.capabilityId === 'string' ? call.capabilityId : 'project-record'
      const source = chapterSource(record) ?? (capability.includes('plan') ? '当前卷规划' : '当前作品记录')
      const recordedAt = stringField(record, 'recordedAt', 'storyTime', 'fromStoryTime', 'createdAt')
        ?? stringField(entry, 'finishedAt', 'startedAt')
      const rawStatus = stringField(record, 'epistemicState', 'state', 'status')
      rows.push({
        label: `${capability === 'query_knowledge' ? '角色知识' : capability === 'query_continuity' ? '剧情线' : capability === 'query_evidence' ? '章节证据' : capability === 'query_events' ? '章节事件' : capability.includes('plan') ? '卷级规划' : '作品记录'}${rows.length + index + 1}`,
        detail: evidenceDetail(capability, record),
        source,
        record: evidenceRecord(capability, record),
        currentStatus: statusLabel(rawStatus),
        related: topicLabel(capability),
        recordedAt,
      })
    })
  }
  return rows
}

function topicLabel(capability: string): string {
  if (capability === 'query_knowledge') return '相关角色与事实'
  if (capability === 'query_continuity') return '相关剧情线'
  if (capability === 'query_plan_route' || capability === 'diagnose_planning_constraints') return '当前卷安排'
  if (capability === 'query_evidence' || capability === 'query_events') return '相关章节'
  return '当前作品'
}

export function presentAssistantSession(session: SupportSessionProjection): AssistantPresentation {
  const finding = session.finding ?? {}
  const status = statusOf(finding)
  if (status === 'insufficient') {
    return {
      answer: '现有记录不足以确认这件事，目前只能确认到已查询的范围。',
      status,
      sections: [
        { title: '当前情况', body: '现有记录不足以确认这件事。' },
        { title: '依据', body: '已查询范围内没有足够的完整证据。' },
        { title: '建议', body: '可以补充相关章节或扩大查询范围，再重新确认。' },
      ],
      evidence: [],
    }
  }

  const claim = sanitizeAssistantText(finding.claim) || '项目记录已完成查询。'
  const evidence = buildEvidence(session)
  const evidenceSummary = evidence.length ? `${evidence.length} 条可展开记录` : '当前没有可展开的完整依据。'
  const answer = status === 'conflicting'
    ? '现有记录之间存在不一致，暂时不能把这件事当作确定事实。'
    : `项目记录显示：${claim}`
  const reason = status === 'conflicting'
    ? '不同记录给出了不一致的结果，系统保留疑点。'
    : '回答只采用了当前项目中已查询到的记录。'
  return {
    answer,
    status,
    sections: [
      { title: '当前情况', body: answer },
      { title: '原因', body: reason },
      { title: '依据', body: evidence.length ? evidenceSummary : '当前没有可展开的完整依据。' },
      { title: '建议', body: status === 'conflicting' ? '先处理记录冲突，再决定是否需要修改。' : '如果要改变正式故事事实，请先创建变更方案。' },
    ],
    evidence,
  }
}
