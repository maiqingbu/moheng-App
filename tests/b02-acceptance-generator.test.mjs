import assert from 'node:assert/strict'
import test from 'node:test'
import {
  REQUIRED_BLOCKING_GATES,
  evaluateAcceptance,
  evaluateGitBinding
} from '../scripts/generate-b02-acceptance.mjs'

const metrics = {
  b02TargetedRustTests: 20,
  allRustTests: 110,
  nodeTests: 9,
  vitestTests: 17,
  contractValid: 6,
  contractInvalid: 24
}

function gateResults(overrides = {}) {
  const gates = REQUIRED_BLOCKING_GATES
    .filter(name => name !== 'git_binding')
    .map(name => ({ name, status: 'PASS', exitCode: 0, blocking: true, log: `evidence/B02/${name}.log` }))
  return {
    sourceCommit: 'a'.repeat(40),
    sourceCommitAfter: 'a'.repeat(40),
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    gitBinding: { status: 'PASS' },
    metrics: { ...metrics },
    gates: gates.map(gate => ({ ...gate, ...(overrides[gate.name] ?? {}) }))
  }
}

test('ACCEPTANCE-GEN-01: all blocking gates PASS produces PASS', () => {
  const result = evaluateAcceptance({ gateResults: gateResults(), actualMetrics: metrics })
  assert.equal(result.status, 'PASS')
})

test('ACCEPTANCE-GEN-02: cargo clippy FAIL produces FAIL', () => {
  const result = evaluateAcceptance({
    gateResults: gateResults({ cargo_clippy: { status: 'FAIL', exitCode: 1 } }),
    actualMetrics: metrics
  })
  assert.equal(result.status, 'FAIL')
})

test('ACCEPTANCE-GEN-03: build FAIL produces FAIL', () => {
  const result = evaluateAcceptance({
    gateResults: gateResults({ build: { status: 'FAIL', exitCode: 1 } }),
    actualMetrics: metrics
  })
  assert.equal(result.status, 'FAIL')
})

test('ACCEPTANCE-GEN-04: missing blocking gate produces FAIL', () => {
  const results = gateResults()
  results.gates = results.gates.filter(gate => gate.name !== 'cargo_clippy')
  const result = evaluateAcceptance({ gateResults: results, actualMetrics: metrics })
  assert.equal(result.status, 'FAIL')
})

test('ACCEPTANCE-GEN-05: mismatched test count produces FAIL', () => {
  const declared = { ...metrics, allRustTests: 109 }
  const result = evaluateAcceptance({ gateResults: gateResults(), actualMetrics: declared })
  assert.equal(result.status, 'FAIL')
})

test('GIT-BINDING-01: unchanged clean commit produces PASS', () => {
  assert.equal(evaluateGitBinding({
    beforeCommit: 'a',
    afterCommit: 'a',
    dirtyBefore: false,
    dirtyAfter: false
  }).status, 'PASS')
})

test('GIT-BINDING-02: changed commit produces FAIL', () => {
  assert.equal(evaluateGitBinding({
    beforeCommit: 'a',
    afterCommit: 'b',
    dirtyBefore: false,
    dirtyAfter: false
  }).status, 'FAIL')
})

test('GIT-BINDING-03: dirty after gates produces FAIL', () => {
  assert.equal(evaluateGitBinding({
    beforeCommit: 'a',
    afterCommit: 'a',
    dirtyBefore: false,
    dirtyAfter: true
  }).status, 'FAIL')
})
