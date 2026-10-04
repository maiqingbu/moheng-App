import { describe, expect, it } from 'vitest'
import type { HealthIssue } from '../../src/application/contracts'
import { healthIssueLocation, healthLevelLabel } from '../../src/ui/presentation/healthLanguage'

const issue = (location: string): HealthIssue => ({
  issueId: 'health:r1:issue',
  factorCode: 'ROUTE_DRIFT',
  severity: 'attention',
  title: '路线需要核对',
  detail: '请回到卷工作区核对。',
  location,
  recommendedDestination: '卷工作区',
  sourceRefs: [
    { kind: 'volume-plan', ref: 'volume:b13:r1:route' },
    { kind: 'obligation', ref: 'obligation:b13:r1:hidden' },
    { kind: 'artifact', ref: 'artifact:b13:r1:route' },
  ],
})

describe('B13 R1 health presentation', () => {
  it('shows distinct labels for unassessed, incomplete, unavailable, and assessed states', () => {
    expect(healthLevelLabel('not_evaluated')).toBe('尚未评估')
    expect(healthLevelLabel('insufficient_data')).toBe('数据仍不足')
    expect(healthLevelLabel('source_unavailable')).toBe('检查暂不可用')
    expect(healthLevelLabel('healthy')).toBe('长篇状态稳定')
    expect(healthLevelLabel('attention')).toBe('总体稳定，有关注项')
    expect(healthLevelLabel('risk')).toBe('需要优先处理')
  })

  it('renders backend volume display metadata while keeping opaque refs internal', () => {
    const visible = healthIssueLocation(issue('第 3 卷 · 暗潮'))

    expect(visible).toBe('第 3 卷 · 暗潮')
    expect(visible).not.toMatch(/volume:|obligation:|artifact:/)
  })

  it('fails closed when a malformed projection leaks an opaque location', () => {
    expect(healthIssueLocation(issue('第 volume:b13:r1:route 卷'))).toBe('相关位置')
  })
})
