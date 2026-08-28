import assert from 'node:assert/strict'
import test from 'node:test'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b06-h01-acceptance.mjs'

function passingResults() {
  return { sourceCommit: 'a'.repeat(40), sourceCommitAfter: 'a'.repeat(40), workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, sourceBinding: { status: 'PASS' }, gates: REQUIRED_BLOCKING_GATES.filter(name => name !== 'source_binding').map(name => ({ name, status: 'PASS', exitCode: 0, blocking: true, log: `${name}.log` })) }
}
test('joint acceptance fails when any B06/H01 required gate is absent', () => {
  const results = passingResults(); results.gates = results.gates.filter(gate => gate.name !== 'b06_context_manifest')
  assert.equal(evaluateAcceptance({ gateResults: results }).status, 'FAIL')
})
for (const gateName of ['b06_current_state', 'b06_continuity', 'b06_resume_condition', 'b06_context_manifest', 'b06_state_digest', 'b06_bounded_retrieval', 'b06_frontend_projection', 'b06_compiler_performance', 'h01_project_scope', 'h01_capability_registry', 'h01_sql_negative', 'h01_diagnostics', 'h01_checkpoint_resume', 'h01_checkpoint_stale', 'h01_bounded_replan', 'h01_read_only_boundary', 'b05_regression']) {
  test(`joint acceptance fails when ${gateName} fails`, () => {
    const results = passingResults(); results.gates.find(gate => gate.name === gateName).status = 'FAIL'
    assert.equal(evaluateAcceptance({ gateResults: results }).status, 'FAIL')
  })
}
test('joint acceptance fails when required evidence is missing', () => {
  assert.equal(evaluateAcceptance({ gateResults: passingResults(), requiredEvidence: ['missing.json'] }).status, 'FAIL')
})
test('joint acceptance fails when source binding is not clean', () => {
  const results = passingResults(); results.sourceBinding.status = 'FAIL'
  assert.equal(evaluateAcceptance({ gateResults: results }).status, 'FAIL')
})
