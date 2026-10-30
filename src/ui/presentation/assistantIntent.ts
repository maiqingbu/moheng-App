// FX-21: how the project assistant understood a question (backend intent).
import type { AssistantIntentResolution } from '../../application/contracts'

const intentLabels: Record<string, string> = {
  PLANNING_CONSTRAINTS: '规划与任务约束',
  KNOWLEDGE: '人物知情情况',
  CONTINUITY: '承诺与伏笔',
  PROJECT_OVERVIEW: '作品整体状态',
  RETCON: '修订已有事实',
  CHANGE: '一般修改',
  PROPOSAL_ONLY: '需要走修改提案',
  RECOMPILE_CHAPTER_PLAN: '重新生成本章计划',
  REPLAN_VOLUME_ROUTE: '重新规划本卷路线',
  REBUILD_FTS: '重建搜索索引',
  REBUILD_PROJECTION: '刷新页面显示数据',
  NO_ACTION: '暂无可执行的修复',
}

export function assistantIntentLabel(resolution: Pick<AssistantIntentResolution, 'intent' | 'source'> | null | undefined): string {
  const label = resolution ? intentLabels[resolution.intent] : undefined
  if (!resolution || !label) return ''
  return `理解为：${label}（${resolution.source === 'MODEL' ? '模型理解' : '关键词匹配'}）`
}

export function changeTypeForIntent(resolution: Pick<AssistantIntentResolution, 'intent' | 'source'>): 'RETCON' | 'CHANGE' {
  return resolution.intent === 'RETCON' ? 'RETCON' : 'CHANGE'
}
