import assert from 'node:assert/strict'
import test from 'node:test'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b07-h02-acceptance.mjs'

function passing() { return { sourceCommit: 'a'.repeat(40), sourceCommitAfter: 'a'.repeat(40), sourceBinding: 'PASS', gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS' }])) } }
test('B07/H02 acceptance passes only when every blocking gate is PASS', () => { assert.equal(evaluateAcceptance(passing()).status, 'PASS') })
for (const gate of REQUIRED_BLOCKING_GATES) test(`B07/H02 acceptance fails when ${gate} is absent or failing`, () => { const missing = passing(); delete missing.gates[gate]; assert.equal(evaluateAcceptance(missing).status, 'FAIL'); const failing = passing(); failing.gates[gate].status = 'FAIL'; assert.equal(evaluateAcceptance(failing).status, 'FAIL') })
