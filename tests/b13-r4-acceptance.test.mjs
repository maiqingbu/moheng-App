import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_R4_GATES, evaluateR4Acceptance, fixedR4Report } from '../scripts/generate-b13-r4-acceptance.mjs'

function passingGates() {
  return Object.fromEntries(REQUIRED_R4_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }]))
}

test('B13-R4 acceptance is fail-closed when any required Gate is missing', () => {
  const gates = passingGates()
  delete gates.reader_b
  const result = evaluateR4Acceptance({ gates, sourceCommit: 'final', evidenceSourceCommit: 'final', sourceCommitAfter: 'final', workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] })
  assert.equal(result.status, 'FAIL')
  assert.deepEqual(result.missingGates, ['reader_b'])
  assert.equal(result.nextBatchAllowed, false)
})

test('B13-R4 acceptance requires an exact clean source binding', () => {
  const result = evaluateR4Acceptance({ gates: passingGates(), sourceCommit: 'final', evidenceSourceCommit: 'other', sourceCommitAfter: 'final', workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] })
  assert.equal(result.status, 'FAIL')
})

test('B13-R4 fixed report exposes exact rename and package fields', () => {
  const acceptance = evaluateR4Acceptance({ gates: passingGates(), sourceCommit: 'final', evidenceSourceCommit: 'final', sourceCommitAfter: 'final', workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [], expectedRenamePaths: 20, actualRenamePaths: 20, creativeSkillContentChanges: 0, frozenPathMismatches: 0, sourceMojibakeFindings: 0, candidateZipMojibakeFindings: 0, finalZipMojibakeFindings: 0, nonNfcPaths: 0 })
  const report = fixedR4Report({ ...acceptance, gates: passingGates() })
  assert.equal(report['B13-R4-01_FROZEN_PATH_RESTORATION'], 'PASS')
  assert.equal(report.EXPECTED_RENAME_PATHS, 20)
  assert.equal(report.ACTUAL_RENAME_PATHS, 20)
  assert.equal(report.NEXT_BATCH_ALLOWED, false)
})
