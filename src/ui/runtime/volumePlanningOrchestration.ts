export interface VolumePlanningPhaseControls {
  startIntent: (options?: { waitForActive?: boolean }) => void
  stopIntent: () => void
  startPlan: (options?: { waitForActive?: boolean }) => void
  stopPlan: () => void
  runIntent: () => Promise<unknown>
  runPlan: () => Promise<unknown>
}

/**
 * Runs the two B03 mutations as one user action while keeping independent
 * runtime observers. The plan observer starts before the second command so a
 * terminal Intent projection can never hide the Plan transition.
 */
export async function executeVolumePlanningSequence(
  controls: VolumePlanningPhaseControls,
): Promise<{ intent: unknown; plan: unknown }> {
  controls.startIntent({ waitForActive: true })
  let intent: unknown
  try {
    intent = await controls.runIntent()
  } finally {
    controls.stopIntent()
  }

  controls.startPlan({ waitForActive: true })
  try {
    const plan = await controls.runPlan()
    return { intent, plan }
  } finally {
    controls.stopPlan()
  }
}
