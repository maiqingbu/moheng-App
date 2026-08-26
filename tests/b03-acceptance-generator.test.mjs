import test from 'node:test'
import assert from 'node:assert/strict'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b03-acceptance.mjs'

function passingResults() {
  return {
    sourceCommit: 'a'.repeat(40),
    sourceCommitAfter: 'a'.repeat(40),
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    gitBinding: { status: 'PASS' },
    metrics: {
      b03TargetedRustTests: 24,
      allRustTests: 117,
      nodeTests: 17,
      vitestTests: 17,
      contractValid: 6,
      contractInvalid: 24
    },
    gates: REQUIRED_BLOCKING_GATES.filter(name => name !== 'git_binding').map(name => ({
      name, status: 'PASS', exitCode: 0, blocking: true, log: `${name}.log`
    }))
  }
}

test('B03 acceptance fails when a required gate is missing', () => {
  const result = passingResults()
  result.gates = result.gates.filter(gate => gate.name !== 'budget_runtime')
  assert.equal(evaluateAcceptance({ gateResults: result, actualMetrics: result.metrics }).status, 'FAIL')
})

for (const gateName of [
  'secret_scan',
  'unknown_budget',
  'settlement_consistency',
  'stable_id_regression',
  'modelrun_recovery',
  'recovery_matrix',
  'unknown_resolution',
  'lease_kernel_invariant',
  'lease_time_boundary',
  'recovery_raw_reopen',
  'recovery_completed_reopen',
  'git_binding'
]) {
  test(`B03 acceptance fails when ${gateName} fails`, () => {
    const result = passingResults()
    if (gateName === 'git_binding') result.gitBinding.status = 'FAIL'
    else result.gates.find(gate => gate.name === gateName).status = 'FAIL'
    assert.equal(evaluateAcceptance({ gateResults: result, actualMetrics: result.metrics }).status, 'FAIL')
  })
}
