import type { RunState } from '../../application/contracts'

export function productionRunStateForJob(status: string | null | undefined, fallback: RunState = 'IDLE'): RunState {
  switch (status) {
    case 'FAILED': return 'FAILED'
    case 'WAITING_DECISION': return 'WAITING_DECISION'
    case 'PAUSED': return 'PAUSED'
    case 'RUNNING':
    case 'WAITING_MODEL': return 'ACTIVE'
    case 'BLOCKED': return 'BLOCKED'
    case 'COMPLETED': return 'COMPLETED'
    case 'CANCELLED': return 'IDLE'
    default: return fallback
  }
}
