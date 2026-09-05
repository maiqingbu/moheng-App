import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_R1_GATES, evaluateR1Acceptance } from '../scripts/generate-b13-r1-acceptance.mjs'

const commit = 'a'.repeat(40)

function passingReport() {
  return {
    sourceCommit: commit,
    evidenceSourceCommit: commit,
    sourceCommitAfter: commit,
    repairBaselineCommit: 'b'.repeat(40),
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates: Object.fromEntries(REQUIRED_R1_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
  }
}

test('B13 R1 acceptance requires every five blocker and evidence gate', () => {
  for (const gate of [
    'b13_r1_01_chapter_identity',
    'b13_r1_02_plan_stale_semantics',
    'b13_r1_03_global_truth_scale',
    'b13_r1_04_product_identity',
    'b13_r1_05_evidence_truthfulness',
  ]) assert.ok(REQUIRED_R1_GATES.includes(gate))
  assert.equal(new Set(REQUIRED_R1_GATES).size, REQUIRED_R1_GATES.length)
})

test('B13 R1 acceptance fails closed for missing gates or dirty source binding', () => {
  const report = passingReport()
  assert.equal(evaluateR1Acceptance(report).status, 'PASS')

  const missing = { ...report, gates: { ...report.gates } }
  delete missing.gates.b13_r1_04_product_identity
  assert.equal(evaluateR1Acceptance(missing).status, 'FAIL')

  const dirty = { ...report, workingTreeDirtyAfter: true }
  assert.equal(evaluateR1Acceptance(dirty).status, 'FAIL')
})
