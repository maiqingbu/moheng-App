import { describe, expect, it } from 'vitest'
import { commandMessage } from '../../src/ui/presentation/productLanguage'

describe('B4-B RV-08 GatePolicy feedback', () => {
  it('directs legacy policies to explicit default adoption before planning', () => {
    expect(commandMessage({
      status: 'failed',
      message: 'GATE_POLICY_UPGRADE_REQUIRED',
    })).toContain('采用默认规则集')
  })

  it('explains that malformed rules need correction', () => {
    expect(commandMessage({
      status: 'failed',
      message: 'GATE_POLICY_RULE_INVALID',
    })).toContain('规则格式')
  })
})
