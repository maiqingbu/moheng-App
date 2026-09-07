import type { EndgameMappingCandidate, EndgameMappingWorkflowProjection } from './contracts'

export type ScopedMappingKind = 'reservation' | 'payoff'

function responsibilityRef(kind: ScopedMappingKind, id: string) {
  const prefix = kind === 'reservation' ? 'reservation:' : 'payoff:'
  return id.startsWith(prefix) ? id : `${prefix}${id}`
}

export function scopedMappingCandidates(
  workflow: Pick<EndgameMappingWorkflowProjection, 'reservationCandidates' | 'hardPayoffCandidates'>,
  kind: ScopedMappingKind,
  id: string,
): EndgameMappingCandidate[] {
  const ref = responsibilityRef(kind, id)
  return kind === 'reservation'
    ? workflow.reservationCandidates[ref] ?? []
    : workflow.hardPayoffCandidates[ref] ?? []
}
