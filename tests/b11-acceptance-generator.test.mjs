import test from 'node:test'
import assert from 'node:assert/strict'
import { B11_BLOCKING_GATES, NEGATIVE_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, START_COMMIT, evaluateAcceptance } from '../scripts/generate-b11-acceptance.mjs'

function passingReport() {
  return {
    sourceBinding: 'PASS',
    startCommit: START_COMMIT,
    startCommitIsAncestor: true,
    sourceCommit: 'final-b11-commit',
    evidenceSourceCommit: 'final-b11-commit',
    sourceCommitAfter: 'final-b11-commit',
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
  }
}

test('B11 acceptance requires all 51 positive and 24 negative gates', () => {
  assert.equal(B11_BLOCKING_GATES.length, 51)
  assert.equal(NEGATIVE_BLOCKING_GATES.length, 24)
  assert.equal(new Set(REQUIRED_BLOCKING_GATES).size, REQUIRED_BLOCKING_GATES.length)
})

test('B11 acceptance fails closed when a required gate is missing', () => {
  const report = passingReport()
  delete report.gates['B11-A31']
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.deepEqual(acceptance.missingGates, ['B11-A31'])
})

test('B11 acceptance cannot pass with dirty source binding', () => {
  const report = passingReport()
  report.workingTreeDirtyAfter = true
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.equal(acceptance.sourceBinding, 'FAIL')
})

