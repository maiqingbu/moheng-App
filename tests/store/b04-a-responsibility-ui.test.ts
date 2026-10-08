import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

describe('B4-A responsibility review surface', () => {
  it('shows new constraints with source evidence and sends versioned user actions', () => {
    const page = fs.readFileSync(new URL('../../src/ui/pages/ProductionCenter.vue', import.meta.url), 'utf8')
    const contracts = fs.readFileSync(new URL('../../src/application/contracts.ts', import.meta.url), 'utf8')

    expect(page).toContain('新增约束 {{ newHardConstraintCount }} 条')
    expect(page).toContain('原文证据：')
    expect(page).toContain('兑现条件：{{ resumeConditionLabel(item.resumeCondition) }}')
    expect(page).toContain("adjustResponsibility(item, 'DOWNGRADE')")
    expect(page).toContain("adjustResponsibility(item, 'UPGRADE', 'B')")
    expect(page).toContain("adjustResponsibility(item, 'UPGRADE', 'A')")
    expect(page).toContain("adjustResponsibility(item, 'DELETE')")
    expect(page).toContain('升级时的兑现对象')
    expect(page).toContain('typed resume condition（JSON）')
    expect(page).toContain('零起点字符范围')
    expect(page).toContain("type: 'thread_responsibility_adjust'")
    expect(contracts).toContain("| 'thread_responsibility_adjust'")
  })
})
