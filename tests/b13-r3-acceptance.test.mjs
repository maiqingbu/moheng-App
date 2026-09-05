import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_R3_GATES, evaluateR3Acceptance, fixedR3Report } from '../scripts/generate-b13-r3-acceptance.mjs'

function passingReport(overrides = {}) {
  const gates = Object.fromEntries(REQUIRED_R3_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }]))
  return {
    repairBaselineShort: 'e6419ce',
    repairBaselineCommit: 'e6419ce2bd980637e30f76c3a57e1e6486b184be',
    postB12FrozenCommit: 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617',
    sourceCommit: 'f'.repeat(40),
    evidenceSourceCommit: 'f'.repeat(40),
    sourceCommitAfter: 'f'.repeat(40),
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates,
    ...overrides,
  }
}

test('B13-R3 acceptance is blocking and fail-closed for every required gate', () => {
  const report = passingReport()
  const acceptance = evaluateR3Acceptance(report)
  assert.equal(acceptance.status, 'PASS')
  assert.equal(acceptance.nextBatchAllowed, false)
  assert.deepEqual(acceptance.missingGates, [])
  assert.deepEqual(acceptance.failedGates, [])
  const fixed = fixedR3Report(acceptance)
  assert.equal(fixed.NEXT_BATCH_ALLOWED, false)
  assert.equal(fixed.DELIVERY_STATUS, 'READY_FOR_FORMAL_ACCEPTANCE')
})

test('B13-R3 acceptance fails when the dual-reader gate is absent', () => {
  const report = passingReport()
  delete report.gates.reader_b
  const acceptance = evaluateR3Acceptance(report)
  assert.equal(acceptance.status, 'FAIL')
  assert.deepEqual(acceptance.missingGates, ['reader_b'])
  assert.equal(acceptance.nextBatchAllowed, false)
})
