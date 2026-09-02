import test from 'node:test'
import assert from 'node:assert/strict'
import { B12_BLOCKING_GATES, NEGATIVE_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, START_COMMIT, evaluateAcceptance } from '../scripts/generate-b12-acceptance.mjs'

function passingReport() {
  return {
    sourceBinding: 'PASS',
    startCommit: START_COMMIT,
    startCommitIsAncestor: true,
    sourceCommit: 'final-b12-commit',
    evidenceSourceCommit: 'final-b12-commit',
    sourceCommitAfter: 'final-b12-commit',
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
  }
}

test('B12 acceptance requires all 61 positive and 28 negative gates', () => {
  assert.equal(B12_BLOCKING_GATES.length, 61)
  assert.equal(NEGATIVE_BLOCKING_GATES.length, 28)
  assert.equal(new Set(REQUIRED_BLOCKING_GATES).size, REQUIRED_BLOCKING_GATES.length)
})

test('B12 acceptance fails closed when a required gate is missing', () => {
  const report = passingReport()
  delete report.gates['B12-A48']
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.deepEqual(acceptance.missingGates, ['B12-A48'])
})

test('B12 acceptance fails closed when a gate has a non-zero exit code', () => {
  const report = passingReport()
  report.gates.b12_scale_bound = { status: 'PASS', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.failedGates.includes('b12_scale_bound'))
})

test('B12 acceptance cannot pass with dirty source binding', () => {
  const report = passingReport()
  report.workingTreeDirtyAfter = true
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.equal(acceptance.sourceBinding, 'FAIL')
})

test('B12 acceptance cannot pass with a false R2 global guard', () => {
  const report = passingReport()
  report.gates.second_authority = { status: 'FAIL', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.failedGates.includes('second_authority'))
})

