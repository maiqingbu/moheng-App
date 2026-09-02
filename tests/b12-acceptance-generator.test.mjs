import test from 'node:test'
import assert from 'node:assert/strict'
import { B12_BLOCKING_GATES, NEGATIVE_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, START_COMMIT, evaluateAcceptance } from '../scripts/generate-b12-acceptance.mjs'
import { packageGeneratorEnvironment } from '../scripts/b12-package-environment.mjs'

function passingReport() {
  return {
    sourceBinding: 'PASS',
    startCommit: START_COMMIT,
    startCommitIsAncestor: true,
    sourceCommit: 'final-b12-commit',
    evidenceSourceCommit: 'final-b12-commit',
    sourceCommitAfter: 'final-b12-commit',
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
  }
}

test('B12 acceptance requires all 61 positive and 28 negative gates', () => {
  assert.equal(B12_BLOCKING_GATES.length, 61)
  assert.equal(NEGATIVE_BLOCKING_GATES.length, 28)
  assert.equal(new Set(REQUIRED_BLOCKING_GATES).size, REQUIRED_BLOCKING_GATES.length)
})

test('B12 acceptance fails closed when a required gate is missing', () => {
  const report = passingReport()
  delete report.gates['B12-A48']
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.deepEqual(acceptance.missingGates, ['B12-A48'])
})

test('B12 acceptance fails closed when a gate has a non-zero exit code', () => {
  const report = passingReport()
  report.gates.b12_scale_bound = { status: 'PASS', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.failedGates.includes('b12_scale_bound'))
})

test('B12 acceptance cannot pass with dirty source binding', () => {
  const report = passingReport()
  report.workingTreeDirtyAfter = true
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.equal(acceptance.sourceBinding, 'FAIL')
})

test('B12 acceptance cannot pass with a false R2 global guard', () => {
  const report = passingReport()
  report.gates.second_authority = { status: 'FAIL', exitCode: 1 }
  const acceptance = evaluateAcceptance(report)
  assert.equal(acceptance.status, 'NOT_READY')
  assert.ok(acceptance.failedGates.includes('second_authority'))
})

test('package self-check delegates package gates to the package verifier', () => {
  const report = passingReport()
  for (const name of ['package_smoke', 'fresh_package_architecture', 'fresh_package_base_architecture', 'fresh_package_module_boundary', 'fresh_package_generator', 'fresh_package_node', 'fresh_package_language']) {
    report.gates[name] = { status: 'FAIL', exitCode: 1 }
  }
  const acceptance = evaluateAcceptance(report, { excludeSelfGate: true })
  assert.equal(acceptance.status, 'READY_FOR_ACCEPTANCE')
  assert.deepEqual(acceptance.failedGates, [])
})

test('package verifier pins generator evidence to the package root', () => {
  const packageRoot = '/tmp/b12-package-root'
  const environment = packageGeneratorEnvironment(packageRoot)
  assert.equal(environment.B12_GENERATOR_SELF_CHECK, '1')
  assert.equal(environment.B12_EVIDENCE_DIR, '/tmp/b12-package-root/evidence/B12')
})
