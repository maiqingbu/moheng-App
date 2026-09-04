import type { HealthIssue, LongFormHealthProjectionR1 } from '../../application/contracts'

type Factor = LongFormHealthProjectionR1['factors'][number]

const FACTOR_LABELS: Record<Factor['factorCode'], string> = {
  OBLIGATION_OVERDUE: '剧情义务',
  FORESHADOW_AGING: '伏笔节奏',
  ROUTE_DRIFT: '规划路线',
  TASK_BLOCKAGE: '任务推进',
  CONTEXT_GROWTH: '上下文规模',
  MODEL_COST_DRIFT: '模型与成本',
  READER_DESIRE: '读者期待',
  QUALITY_DRIFT: '质量趋势',
}

const SEVERITY_LABELS: Record<HealthIssue['severity'], string> = {
  healthy: '稳定',
  attention: '需要关注',
  risk: '优先处理',
}

export function factorLabel(code: Factor['factorCode']): string {
  return FACTOR_LABELS[code] ?? '长期状态'
}

export function severityLabel(severity: HealthIssue['severity']): string {
  return SEVERITY_LABELS[severity] ?? '需要关注'
}

export function statusLabel(status: Factor['status']): string {
  if (status === 'INSUFFICIENT_DATA') return '数据积累中'
  if (status === 'SOURCE_UNAVAILABLE') return '暂时无法完成这一项检查'
  return '已完成检查'
}

export function truthClassLabel(truthClass: Factor['truthClass']): string {
  return truthClass === 'SOFT_ADVISORY' ? '创作建议' : '正式状态检查'
}

export function destinationLabel(destination: string): string {
  if (destination.includes('创作')) return '创作中心'
  if (destination.includes('历史')) return '版本与变更记录'
  return '卷工作区'
}

export function healthLevelLabel(level: LongFormHealthProjectionR1['level']): string {
  if (level === 'risk') return '需要优先处理'
  if (level === 'attention') return '总体稳定，有关注项'
  return '状态稳定'
}

export function issueKey(issue: HealthIssue): string {
  return `${issue.factorCode}:${issue.issueId}`
}
