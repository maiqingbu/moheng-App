import test from 'node:test'
import assert from 'node:assert/strict'
import { B09_BLOCKING_GATES, FROZEN_BASELINE_COMMIT, H04_BLOCKING_GATES, H04_GATE_DEFINITIONS, REQUIRED_BLOCKING_GATES, START_COMMIT, evaluateAcceptance } from '../scripts/generate-b09-h04-acceptance.mjs'

const passing = () => ({
  startCommit: START_COMMIT,
  startCommitIsAncestor: true,
  baselineCommit: FROZEN_BASELINE_COMMIT,
  baselineIsAncestor: true,
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

test('H04 gate ids retain the frozen meanings and source binding requires the start commit', () => {
  assert.equal(START_COMMIT, 'e964546712b2fe620edbdf3c7d393d771f02de9c')
  assert.deepEqual(H04_GATE_DEFINITIONS, {
    'H04-A01': 'No Fourth Harness',
    'H04-A02': 'Cross-volume Bounded Read',
    'H04-A03': 'Cross-volume Provenance',
    'H04-A04': 'H02 No Mutation',
    'H04-A05': 'replan_volume_route Capability',
    'H04-A06': 'Projection Drift Repair',
    'H04-A07': 'Canon Conflict Proposal Only',
    'H04-A08': 'Checkpoint Revision Revalidation',
    'H04-A09': 'Capability Digest Revalidation',
    'H04-A10': 'Repair Authorization One-time',
    'H04-A11': '10M Bounded Query',
    'H04-A12': 'No Unbounded Loop',
  })
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
