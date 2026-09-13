import type { ProductionCenterProjection } from '../../application/contracts'

export const PRODUCTION_RUNTIME_POLL_INTERVAL_MS = 750
const TERMINAL_JOB_STATUSES = new Set(['COMPLETED', 'FAILED', 'BLOCKED', 'CANCELLED'])

export function isProductionJobTerminal(status: string | null | undefined): boolean {
  return status != null && TERMINAL_JOB_STATUSES.has(status)
}

export interface ProductionRuntimePollerOptions {
  query: () => Promise<ProductionCenterProjection>
  onProjection: (projection: ProductionCenterProjection) => void
  onError?: (error: unknown) => void
  intervalMs?: number
}

export interface ProductionRuntimePoller {
  start: (options?: { waitForActive?: boolean }) => void
  stop: () => void
  isRunning: () => boolean
}

/**
 * Keeps a page's projection synchronized with the authoritative job while a
 * production command is in flight. It is deliberately page-scoped: leaving
 * the route stops the timer, while the backend job continues independently.
 */
export function createProductionRuntimePoller(
  options: ProductionRuntimePollerOptions,
): ProductionRuntimePoller {
  const intervalMs = options.intervalMs ?? PRODUCTION_RUNTIME_POLL_INTERVAL_MS
  let timer: ReturnType<typeof setInterval> | null = null
  let running = false
  let queryInFlight = false
  let sawActiveRuntime = false
  let waitForActive = false

  const stop = () => {
    running = false
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  const tick = async () => {
    if (!running || queryInFlight) return
    queryInFlight = true
    try {
      const projection = await options.query()
      if (!running) return
      options.onProjection(projection)
      if (projection.jobStatus === 'RUNNING' || projection.jobStatus === 'WAITING_MODEL') sawActiveRuntime = true
      if ((!waitForActive || sawActiveRuntime) && isProductionJobTerminal(projection.jobStatus)) stop()
    } catch (error) {
      if (running) options.onError?.(error)
    } finally {
      queryInFlight = false
    }
  }

  return {
    start: (options = {}) => {
      if (running) return
      running = true
      sawActiveRuntime = false
      waitForActive = options.waitForActive === true
      void tick()
      timer = setInterval(() => void tick(), Math.max(100, intervalMs))
    },
    stop,
    isRunning: () => running,
  }
}
