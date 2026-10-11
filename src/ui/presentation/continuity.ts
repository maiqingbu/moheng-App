export function resumeConditionLabel(condition: Record<string, unknown> | null | undefined): string {
  if (!condition) return '未提供明确兑现条件'
  const kind = String(condition.kind ?? '')
  const value = condition.value
  if (kind === 'ENTITY_PRESENT') return `当人物或实体出现（${String(value ?? '未知')}）`
  if (kind === 'NEXT_MEETING' && value && typeof value === 'object') {
    const meeting = value as Record<string, unknown>
    return `当 ${String(meeting.entityA ?? '人物 A')} 与 ${String(meeting.entityB ?? '人物 B')} 再次相遇`
  }
  if (kind === 'KNOWLEDGE_ACQUIRED') return `当相关知识获得（${String(value ?? '未知')}）`
  if (kind === 'TASK_SATISFIED') return `当任务完成（${String(value ?? '未知')}）`
  if (kind === 'AFTER_CHAPTER') return `第 ${String(value ?? '未知')} 章之后`
  if (kind === 'IN_VOLUME') return `进入卷（${String(value ?? '未知')}）`
  return kind ? `${kind} · ${JSON.stringify(value)}` : '未提供明确兑现条件'
}

export function continuityClassLabel(value: string): string {
  return ({
    OBLIGATION: '剧情责任',
    MYSTERY: '谜团',
    FORESHADOW: '伏笔',
    PAYOFF: '兑现事项',
    CONTINUITY: '连续性事项',
    TASK: '任务',
  } as Record<string, string>)[value] ?? '剧情线'
}

export function continuityDispositionLabel(value: string): string {
  return ({
    ADVANCE: '继续推进',
    HOLD: '暂缓处理',
    RESOLVE: '已兑现',
    OBSERVE: '继续观察',
    CHANGE_REQUEST: '等待变更流程',
  } as Record<string, string>)[value] ?? value
}
