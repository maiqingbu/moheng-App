export type ResumeConditionKind =
  | 'NEXT_MEETING'
  | 'ENTITY_PRESENT'
  | 'KNOWLEDGE_ACQUIRED'
  | 'TASK_SATISFIED'
  | 'AFTER_CHAPTER'
  | 'IN_VOLUME'

export interface ResumeConditionDraft {
  kind: ResumeConditionKind
  value: string
  entityA: string
  entityB: string
}

export const resumeConditionKinds: Array<{ value: ResumeConditionKind; label: string }> = [
  { value: 'NEXT_MEETING', label: '两人再次相遇' },
  { value: 'ENTITY_PRESENT', label: '人物或实体出现' },
  { value: 'KNOWLEDGE_ACQUIRED', label: '获得相关知识' },
  { value: 'TASK_SATISFIED', label: '任务完成' },
  { value: 'AFTER_CHAPTER', label: '某章之后' },
  { value: 'IN_VOLUME', label: '进入某一卷' },
]

export function createResumeConditionDraft(condition?: Record<string, unknown> | null): ResumeConditionDraft {
  const kind = resumeConditionKinds.some(option => option.value === condition?.kind)
    ? condition?.kind as ResumeConditionKind
    : 'ENTITY_PRESENT'
  const value = condition?.value
  if (kind === 'NEXT_MEETING' && value && typeof value === 'object') {
    const meeting = value as Record<string, unknown>
    return { kind, value: '', entityA: String(meeting.entityA ?? ''), entityB: String(meeting.entityB ?? '') }
  }
  return { kind, value: value === undefined || value === null ? '' : String(value), entityA: '', entityB: '' }
}

export function serializeResumeCondition(draft: ResumeConditionDraft): Record<string, unknown> | null {
  if (draft.kind === 'NEXT_MEETING') {
    const entityA = draft.entityA.trim()
    const entityB = draft.entityB.trim()
    if (!entityA || !entityB) return null
    return { kind: draft.kind, value: { entityA, entityB } }
  }
  const value = draft.value.trim()
  if (!value) return null
  if (draft.kind === 'AFTER_CHAPTER') {
    const chapterNo = Number(value)
    if (!Number.isSafeInteger(chapterNo) || chapterNo < 0) return null
    return { kind: draft.kind, value: chapterNo }
  }
  return { kind: draft.kind, value }
}

export function buildResponsibilityEvidence(
  quoteDraft: string,
  start?: number,
  end?: number,
  advanced = false,
): Record<string, unknown> | null {
  const quote = quoteDraft.trim()
  if (!quote) return null
  if (advanced && start !== undefined && end !== undefined) return { start, end, quote }
  return { quote }
}
