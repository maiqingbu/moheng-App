import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_B14_GATES, evaluateB14Acceptance, fixedB14Report } from '../scripts/generate-b14-acceptance.mjs'

function passingGates() { return Object.fromEntries(REQUIRED_B14_GATES.map(name => [name, { status: 'PASS' }])) }
function binding() { return { sourceCommit: 'final', evidenceSourceCommit: 'final', sourceCommitAfter: 'final', workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] } }

test('B14 acceptance is fail-closed when a blocking gate is missing', () => {
  const gates = passingGates()
  delete gates.b14_atomic_completion
  const result = evaluateB14Acceptance({ gates, ...binding() })
  assert.equal(result.status, 'FAIL')
  assert.deepEqual(result.missingGates, ['b14_atomic_completion'])
  assert.equal(result.nextBatchAllowed, false)
})

test('B14 acceptance requires a clean exact source binding', () => {
  const result = evaluateB14Acceptance({ gates: passingGates(), ...binding(), evidenceSourceCommit: 'other' })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.exactSourceBinding, false)
})

test('B14 fixed report keeps B15 locked even when all gates pass', () => {
  const result = evaluateB14Acceptance({ gates: passingGates(), ...binding() })
  const report = fixedB14Report(result)
  assert.equal(report.STATUS, 'PASS')
  assert.equal(report.NEXT_BATCH_ALLOWED, false)
})

