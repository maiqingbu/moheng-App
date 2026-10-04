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
  if (level === 'source_unavailable') return '检查暂不可用'
  if (level === 'insufficient_data') return '数据仍不足'
  if (level === 'not_evaluated') return '尚未评估'
  return '长篇状态稳定'
}

export function issueKey(issue: HealthIssue): string {
  return `${issue.factorCode}:${issue.issueId}`
}

/**
 * Health locations are supplied by the backend projection's display metadata.
 * An invalid projection must fail closed instead of leaking an opaque source
 * reference into the product surface.
 */
export function healthIssueLocation(issue: HealthIssue): string {
  const location = issue.location.trim()
  if (!location) return '相关位置'
  if (/(?:^|[\s·])(?:chapter|volume|artifact|obligation|project)(?:[_:-]|\b)/i.test(location)) {
    return '相关位置'
  }
  return location
}

/** Browser-only manual walkthrough fixtures. They transform an already valid
 * health projection at the presentation boundary; production queries never
 * receive this mode because it is enabled only by the explicit demo build.
 */
export function healthEvidenceProjection(
  base: LongFormHealthProjectionR1,
  mode: string,
  cursor: string | null = null,
): LongFormHealthProjectionR1 {
  if (!mode) return base
  const projection = JSON.parse(JSON.stringify(base)) as LongFormHealthProjectionR1
  const factor = (code: Factor['factorCode']) => projection.factors.find(item => item.factorCode === code)
  const setFactor = (code: Factor['factorCode'], severity: Factor['severity'], status: Factor['status'], summary: string, detail: string) => {
    const target = factor(code)
    if (!target) return
    target.severity = severity
    target.status = status
    target.summary = summary
    target.detail = detail
  }
  const issue = (index: number, factorCode: Factor['factorCode'], severity: HealthIssue['severity'], location: string): HealthIssue => ({
    issueId: `health:evidence:${index}`,
    factorCode,
    severity,
    title: '需要关注的事项',
    detail: '请回到对应工作区核对当前安排。',
    location,
    recommendedDestination: factorCode === 'QUALITY_DRIFT' ? '创作中心' : '卷工作区',
    sourceRefs: [],
  })

  if (mode === 'm01') {
    for (const item of projection.factors) {
      item.status = 'EVALUATED'
      item.severity = 'healthy'
    }
    projection.level = 'healthy'
    projection.score = 100
    projection.issues = { items: [], nextCursor: null, total: 0 }
  } else if (mode === 'm03') {
    setFactor('OBLIGATION_OVERDUE', 'risk', 'EVALUATED', '剧情义务需要优先处理', '已有高优先级剧情义务到达处理节点。')
    projection.level = 'risk'
    projection.counters.overdueObligations = Math.max(1, projection.counters.overdueObligations)
    projection.issues = { items: [issue(1, 'OBLIGATION_OVERDUE', 'risk', '第 9 章附近')], nextCursor: null, total: 1 }
  } else if (mode === 'm04') {
    setFactor('QUALITY_DRIFT', 'attention', 'EVALUATED', '质量趋势需要关注', '近期质量样本出现变化，仅作创作建议参考。')
    projection.level = 'attention'
    projection.issues = { items: [issue(1, 'QUALITY_DRIFT', 'attention', '近期章节')], nextCursor: null, total: 1 }
  } else if (mode === 'm05') {
    setFactor('READER_DESIRE', 'attention', 'EVALUATED', '有读者期待到了回应节点', '有一项读者期待需要在卷工作区核对回应安排。')
    projection.level = 'attention'
    projection.counters.overdueReaderDesires = Math.max(1, projection.counters.overdueReaderDesires)
    projection.issues = { items: [issue(1, 'READER_DESIRE', 'attention', '第 9 章附近')], nextCursor: null, total: 1 }
  } else if (mode === 'm06') {
    setFactor('CONTEXT_GROWTH', 'attention', 'INSUFFICIENT_DATA', '上下文样本仍在积累', '完成更多相同能力与模型的正式运行后再判断趋势。')
    projection.level = 'attention'
    projection.issues = { items: [], nextCursor: null, total: 0 }
  } else if (mode === 'm07') {
    for (const code of ['OBLIGATION_OVERDUE', 'FORESHADOW_AGING', 'ROUTE_DRIFT', 'TASK_BLOCKAGE'] as const) {
      setFactor(code, 'healthy', 'EVALUATED', '数据仍在积累', '建立正式记录后，这项检查会继续更新。')
    }
    for (const code of ['CONTEXT_GROWTH', 'MODEL_COST_DRIFT', 'QUALITY_DRIFT'] as const) {
      setFactor(code, 'attention', 'INSUFFICIENT_DATA', '数据仍在积累', '新建作品尚未产生足够正式记录。')
    }
    projection.level = 'attention'
    projection.issues = { items: [], nextCursor: null, total: 0 }
  } else if (mode === 'm08') {
    setFactor('OBLIGATION_OVERDUE', 'attention', 'SOURCE_UNAVAILABLE', '部分来源需要核对', '关联章节记录暂时无法读取，未将其误判为逾期。')
    projection.level = 'attention'
    projection.issues = { items: [issue(1, 'OBLIGATION_OVERDUE', 'attention', '相关章节')], nextCursor: null, total: 1 }
  } else if (mode === 'm09' || mode === 'm10') {
    const all = Array.from({ length: 60 }, (_, index) => issue(
      index + 1,
      (['OBLIGATION_OVERDUE', 'FORESHADOW_AGING', 'READER_DESIRE'] as const)[index % 3],
      index % 3 === 0 ? 'risk' : 'attention',
      `第 ${(index % 20) + 1} 章附近`,
    ))
    const start = Number.parseInt(cursor ?? '0', 10) || 0
    projection.issues = { items: all.slice(start, start + 30), nextCursor: start + 30 < all.length ? String(start + 30) : null, total: all.length }
    projection.level = 'risk'
  } else if (mode === 'm02' || mode === 'm11' || mode === 'm12') {
    // Keep the real demo projection and exercise the normal page presentation.
  }
  return projection
}
