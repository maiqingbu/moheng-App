import test from 'node:test'
import assert from 'node:assert/strict'
import { B11_BLOCKING_GATES, NEGATIVE_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, START_COMMIT, evaluateAcceptance } from '../scripts/generate-b11-acceptance.mjs'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

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

test('R06-T1 base architecture FAIL blocks B11 acceptance', () => {
  const report = passingReport()
  report.gates.base_architecture = { status: 'FAIL', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.failedGates.includes('base_architecture'))
})

test('R06-T2 missing base architecture evidence blocks B11 acceptance', () => {
  const report = passingReport()
  delete report.gates.base_architecture
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.missingGates.includes('base_architecture'))
})

test('R06-T3 B11 architecture PASS cannot mask base architecture FAIL', () => {
  const report = passingReport()
  report.gates.architecture = { status: 'PASS', exitCode: 0 }
  report.gates.base_architecture = { status: 'FAIL', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
})

test('B11 regression evidence includes the blocking base architecture result', () => {
  const runner = fs.readFileSync(path.join(root, 'scripts/run-b11-evidence-gates.mjs'), 'utf8')
  assert.match(runner, /baseArchitecture/)
  assert.match(runner, /B11_REGRESSION\.json/)
  assert.match(runner, /baseArchitecture[^\n]*status/)
})
