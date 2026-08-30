import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b07-h02-acceptance.mjs'

const root = new URL('../', import.meta.url)
const read = relative => readFileSync(new URL(relative, root), 'utf8')
const passing = () => ({
  sourceCommit: 'a'.repeat(40),
  sourceCommitAfter: 'a'.repeat(40),
  sourceBinding: 'PASS',
  gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS' }])),
})

const injections = [
  ['golden_three_signing', 'b07_signing_authority'],
  ['caller_reservation_omission', 'b07_chapter_plan_gates'],
  ['unmet_task_dependency', 'b07_chapter_plan_gates'],
  ['model_output_mismatch', 'b07_chapter_plan_gates'],
  ['h02_missing_provenance', 'h02_diagnostic_provenance'],
  ['scale_missing', 'b07_h02_scale'],
]

for (const [name, gate] of injections) {
  test(`semantic injection ${name} fails its blocking gate`, () => {
    const injected = passing()
    injected.gates[gate].status = 'FAIL'
    assert.equal(evaluateAcceptance(injected).status, 'FAIL')
  })
}

test('semantic injection suite is tied to production authority guards', () => {
  const commands = read('src-tauri/src/commands.rs')
  const repository = read('src-tauri/src/repositories/narrative.rs')
  const workspace = read('src/ui/pages/VolumeWorkspace.vue')
  const manager = read('src/ui/pages/VolumeManager.vue')
  assert.match(repository, /chapter_no IN \(1,2,3\)/)
  assert.match(repository, /canon_evidence e/)
  assert.match(commands, /signing_receipt_passes/)
  assert.match(commands, /planning_model_run_binds_plan/)
  assert.match(commands, /validate_reservation_actions/)
  assert.match(commands, /validate_task_reality/)
  assert.match(commands, /continuity_threads/)
  assert.match(workspace, /currentChapterPlan/)
  assert.match(workspace, /futureReservations/)
  assert.doesNotMatch(manager, /volume_plan_build|volume_production_start|volume_production_pause|volume_close/)
})
