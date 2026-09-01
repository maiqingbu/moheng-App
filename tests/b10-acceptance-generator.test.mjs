import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  B10_BLOCKING_GATES,
  B10_ENGINEERING_GATES,
  NEGATIVE_BLOCKING_GATES,
  R2_GLOBAL_BLOCKING_GATES,
  REQUIRED_BLOCKING_GATES,
  START_COMMIT,
  evaluateAcceptance,
} from '../scripts/generate-b10-acceptance.mjs'

const passing = () => ({
  startCommit: START_COMMIT,
  sourceCommit: 'final',
  evidenceSourceCommit: 'final',
  sourceCommitAfter: 'final',
  startCommitIsAncestor: true,
  workingTreeDirtyBefore: false,
  workingTreeDirtyAfter: false,
  excludedUncommittedPaths: [],
  sourceBinding: 'PASS',
  gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
})

test('B10 acceptance contains the full functional, negative, global and release matrix', () => {
  assert.equal(B10_BLOCKING_GATES.length, 34)
  assert.equal(NEGATIVE_BLOCKING_GATES.length, 16)
  assert.equal(R2_GLOBAL_BLOCKING_GATES.length, 15)
  assert.equal(evaluateAcceptance(passing()).status, 'READY_FOR_ACCEPTANCE')
})

test('B10 acceptance runner uses an exact assertion mapping for every negative gate', () => {
  const root = fileURLToPath(new URL('..', import.meta.url))
  const runner = readFileSync(resolve(root, 'scripts/run-b10-evidence-gates.mjs'), 'utf8')
  assert.ok(B10_ENGINEERING_GATES.includes('b10_negative_matrix_exact_mapping'))
  assert.match(runner, /B10_NEGATIVE_MATRIX\.json/)
  assert.match(runner, /function exactTestPass\(/)
  assert.match(runner, /const negativeCases = \[/)
  assert.doesNotMatch(runner, /const negativePass = \{/)
})

test('every missing B10 gate fails closed', () => {
  for (const name of REQUIRED_BLOCKING_GATES) {
    const report = passing()
    delete report.gates[name]
    const result = evaluateAcceptance(report)
    assert.equal(result.status, 'NOT_READY', name)
    assert.ok(result.missingGates.includes(name), name)
  }
})

test('source binding and dirty state cannot be bypassed by green test status', () => {
  const dirty = passing()
  dirty.workingTreeDirtyAfter = true
  assert.equal(evaluateAcceptance(dirty).status, 'NOT_READY')
  const wrongStart = passing()
  wrongStart.startCommit = 'ec1a12176100ba427395c2d62bdc1661dcabfa5e'
  assert.equal(evaluateAcceptance(wrongStart).status, 'NOT_READY')
  const mismatched = passing()
  mismatched.evidenceSourceCommit = 'other'
  assert.equal(evaluateAcceptance(mismatched).status, 'NOT_READY')
})
