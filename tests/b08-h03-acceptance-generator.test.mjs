import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_BLOCKING_GATES, evaluateAcceptance } from '../scripts/generate-b08-h03-acceptance.mjs'

const passing = () => ({
  sourceCommit:'final',sourceCommitAfter:'final',sourceBinding:'PASS',
  gates:Object.fromEntries(REQUIRED_BLOCKING_GATES.map(id => [id,{status:'PASS'}])),
})

test('B08/H03 acceptance requires the full frozen matrix', () => {
  assert.equal(REQUIRED_BLOCKING_GATES.filter(id => id.startsWith('B08-F')).length,12)
  assert.equal(REQUIRED_BLOCKING_GATES.filter(id => id.startsWith('B08-I')).length,9)
  assert.equal(REQUIRED_BLOCKING_GATES.filter(id => id.startsWith('H03-A')).length,11)
  assert.equal(REQUIRED_BLOCKING_GATES.filter(id => id.startsWith('H03-I')).length,4)
  assert.equal(evaluateAcceptance(passing()).status,'PASS')
})

test('every missing functional or release gate fails closed', () => {
  for (const id of REQUIRED_BLOCKING_GATES) {
    const report = passing()
    delete report.gates[id]
    const result = evaluateAcceptance(report)
    assert.equal(result.status,'FAIL',id)
    assert.ok(result.missingBlockingGates.includes(id),id)
  }
})

test('dirty or mismatched source binding cannot pass', () => {
  const report = passing()
  report.sourceBinding='FAIL'
  assert.equal(evaluateAcceptance(report).status,'FAIL')
})
