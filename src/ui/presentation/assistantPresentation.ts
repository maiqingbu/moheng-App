import type { SupportSessionProjection } from '../../application/contracts'

export type AssistantViewStatus = 'supported' | 'insufficient' | 'conflicting' | 'inferred'

export interface AssistantSection {
  title: '当前情况' | '原因' | '依据' | '建议'
  body: string
}

export interface AssistantEvidenceItem {
  label: string
  detail: string
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
  const ledger = session.evidenceLedger ?? []
  const sourceCount = ledger.length || (Array.isArray(finding.sourceRefs) ? finding.sourceRefs.length : 0)
  const evidence = sourceCount > 0
    ? [{ label: '已核验的项目记录', detail: `共 ${sourceCount} 条记录，来自相关章节与项目状态。` }]
    : []
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
      { title: '依据', body: evidence.length ? evidence[0].detail : '当前没有可展开的完整依据。' },
      { title: '建议', body: status === 'conflicting' ? '先处理记录冲突，再决定是否需要修改。' : '如果要改变正式故事事实，请先创建变更方案。' },
    ],
    evidence,
  }
}
