import test from 'node:test'
import assert from 'node:assert/strict'
import { B09_BLOCKING_GATES, H04_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, evaluateAcceptance } from '../scripts/generate-b09-h04-acceptance.mjs'

const passing = () => ({
  sourceCommit: 'final',
  evidenceSourceCommit: 'final',
  sourceCommitAfter: 'final',
  sourceBinding: 'PASS',
  workingTreeDirtyBefore: false,
  workingTreeDirtyAfter: false,
  excludedUncommittedPaths: [],
  gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS' }])),
})

test('B09/H04 acceptance contains the complete functional and global matrix', () => {
  assert.equal(B09_BLOCKING_GATES.length, 22)
  assert.equal(H04_BLOCKING_GATES.length, 12)
  assert.equal(evaluateAcceptance(passing()).status, 'READY_FOR_ACCEPTANCE')
})

test('any missing gate fails closed and cannot claim readiness', () => {
  for (const name of REQUIRED_BLOCKING_GATES) {
    const report = passing()
    delete report.gates[name]
    const result = evaluateAcceptance(report)
    assert.equal(result.status, 'NOT_READY', name)
    assert.ok(result.missingGates.includes(name), name)
  }
})

test('dirty or mismatched source binding cannot claim readiness', () => {
  const dirty = passing()
  dirty.workingTreeDirtyAfter = true
  assert.equal(evaluateAcceptance(dirty).status, 'NOT_READY')
  const mismatched = passing()
  mismatched.evidenceSourceCommit = 'other'
  assert.equal(evaluateAcceptance(mismatched).status, 'NOT_READY')
})
