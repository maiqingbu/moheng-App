import { describe, expect, it } from 'vitest'
import type { EndgameMappingWorkflowProjection } from '../../src/application/contracts'
import { scopedMappingCandidates } from '../../src/application/endgameMapping'

const workflow: EndgameMappingWorkflowProjection = {
  mysteries: [],
  reservations: [
    { key: 'reservation:A', id: 'reservation:A', label: '约束 A' },
    { key: 'reservation:B', id: 'reservation:B', label: '约束 B' },
  ],
  hardPayoffs: [
    { key: 'payoff:P1', id: 'P1', label: '回报 P1' },
    { key: 'payoff:P2', id: 'P2', label: '回报 P2' },
  ],
  responsibilityCandidates: [],
  fulfillmentCandidates: [
    { ref: 'final:unrelated', label: '全局无关章节' },
  ],
  reservationCandidates: {
    'reservation:A': [{ ref: 'final:A', label: '证据 A' }],
    'reservation:B': [{ ref: 'final:B', label: '证据 B' }],
  },
  hardPayoffCandidates: {
    'payoff:P1': [{ ref: 'final:P1', label: '证据 P1' }],
    'payoff:P2': [{ ref: 'final:P2', label: '证据 P2' }],
  },
}

describe('B14-R5 Completion mapping projection', () => {
  it('keeps reservation and payoff candidates scoped to their responsibility', () => {
    expect(scopedMappingCandidates(workflow, 'reservation', 'reservation:A')).toEqual([{ ref: 'final:A', label: '证据 A' }])
    expect(scopedMappingCandidates(workflow, 'reservation', 'reservation:B')).toEqual([{ ref: 'final:B', label: '证据 B' }])
    expect(scopedMappingCandidates(workflow, 'payoff', 'P1')).toEqual([{ ref: 'final:P1', label: '证据 P1' }])
    expect(scopedMappingCandidates(workflow, 'payoff', 'P2')).toEqual([{ ref: 'final:P2', label: '证据 P2' }])
  })

  it('returns an empty list without falling back to the global candidate union', () => {
    expect(scopedMappingCandidates(workflow, 'reservation', 'reservation:missing')).toEqual([])
    expect(scopedMappingCandidates(workflow, 'payoff', 'missing')).toEqual([])
  })

  it('keeps a 2000-chapter global candidate set out of scoped dropdowns', () => {
    const large = {
      ...workflow,
      fulfillmentCandidates: Array.from({ length: 2000 }, (_, index) => ({ ref: `final:${index}`, label: `章节 ${index}` })),
      reservationCandidates: { 'reservation:A': [{ ref: 'final:A', label: '证据 A' }] },
    }
    expect(scopedMappingCandidates(large, 'reservation', 'reservation:A')).toHaveLength(1)
    expect(scopedMappingCandidates(large, 'reservation', 'reservation:B')).toHaveLength(0)
  })
})
