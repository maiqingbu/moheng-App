import assert from 'node:assert/strict'
import test from 'node:test'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b05-acceptance.mjs'

function passingResults() {
  const metrics = { b05TargetedRustTests: 7, allRustTests: 171, nodeTests: 54, vitestTests: 19, contractValid: 15, contractInvalid: 31 }
  return { sourceCommit: 'a'.repeat(40), sourceCommitAfter: 'a'.repeat(40), workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, gitBinding: { status: 'PASS' }, metrics, gates: REQUIRED_BLOCKING_GATES.filter(name => name !== 'git_binding').map(name => ({ name, status: 'PASS', exitCode: 0, blocking: true, log: `${name}.log` })) }
}

test('B05 acceptance fails closed when a required gate is missing', () => {
  const results = passingResults()
  results.gates = results.gates.filter(gate => gate.name !== 'migration_0007')
  assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
})

for (const gateName of ['payload_contracts', 'bookplan_seed_contract', 'task_schema_runtime_parity', 'legacy_compatibility', 'authority_separation', 'repository_coverage', 'repository_relevance', 'plan_canon_separation', 'thread_promotion', 'task_state_contract', 'canon_event_time', 'frontend_contract', 'forbidden_authority_scan']) {
  test(`B05 acceptance fails when ${gateName} fails`, () => {
    const results = passingResults()
    results.gates.find(gate => gate.name === gateName).status = 'FAIL'
    assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
  })
}
