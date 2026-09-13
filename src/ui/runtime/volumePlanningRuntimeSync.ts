import type { VolumeDetailProjection } from '../../application/contracts'

export const VOLUME_PLANNING_RUNTIME_POLL_INTERVAL_MS = 750
const TERMINAL_STATUSES = new Set(['COMPLETED', 'FAILED', 'BLOCKED', 'CANCELLED'])

export interface VolumePlanningRuntimePoller {
  start: (options?: { waitForActive?: boolean }) => void
  stop: () => void
  isRunning: () => boolean
}

export interface VolumePlanningRuntimePollerOptions {
  query: () => Promise<VolumeDetailProjection>
  onProjection: (projection: VolumeDetailProjection) => void
  onError?: (error: unknown) => void
  intervalMs?: number
}

/**
 * Synchronizes the VolumeWorkspace with the backend B03 runtime projection.
 * The timer only reads the durable projection; execution remains owned by the
 * command boundary and continues if the user leaves the page.
 */
export function createVolumePlanningRuntimePoller(
  options: VolumePlanningRuntimePollerOptions,
): VolumePlanningRuntimePoller {
  const intervalMs = options.intervalMs ?? VOLUME_PLANNING_RUNTIME_POLL_INTERVAL_MS
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
      const operation = projection.runtimeOperation
      if (operation?.active === true) sawActiveRuntime = true
      const terminal = operation != null && TERMINAL_STATUSES.has(operation.status)
      if ((!waitForActive || sawActiveRuntime) && terminal) stop()
    } catch (error) {
      if (running) options.onError?.(error)
    } finally {
      queryInFlight = false
    }
  }

  return {
    start: (startOptions = {}) => {
      if (running) return
      running = true
      sawActiveRuntime = false
      waitForActive = startOptions.waitForActive === true
      void tick()
      timer = setInterval(() => void tick(), Math.max(100, intervalMs))
    },
    stop,
    isRunning: () => running,
  }
}
