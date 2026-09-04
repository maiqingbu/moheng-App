import test from 'node:test'
import assert from 'node:assert/strict'
import { REQUIRED_BLOCKING_GATES, evaluateAcceptance } from '../scripts/generate-b13-acceptance.mjs'

const required = [
  'b13_contract_policy', 'b13_deterministic_runtime', 'b13_projection_rebuild', 'b13_backend_query',
  'b13_formal_ui', 'b13_product_ux', 'b13_scale', 'b13_evidence',
  'factor_h01_obligation', 'factor_h02_foreshadow', 'factor_h03_route', 'factor_h04_task',
  'factor_h05_context', 'factor_h06_model_cost', 'factor_h07_reader_desire', 'factor_h08_quality',
  'architecture', 'module_boundary', 'no_authority_mutation', 'no_remote_model_call', 'determinism',
  'pagination', 'scale', 'product_ux', 'capability_parity', 'b14_boundary', 'b15_boundary', 'rc1_boundary',
  'rust_tests', 'node_tests', 'vitest', 'typecheck', 'build', 'contracts', 'package_integrity', 'package_utf8',
  'secret_scan', 'source_binding',
]

test('B13 acceptance declares every blocking factor and boundary gate', () => {
  for (const name of required) assert.ok(REQUIRED_BLOCKING_GATES.includes(name), `missing ${name}`)
  assert.equal(new Set(REQUIRED_BLOCKING_GATES).size, REQUIRED_BLOCKING_GATES.length)
})

test('B13 acceptance fails closed when any required gate is missing or failing', () => {
  const report = { sourceCommit: 'a'.repeat(40), sourceBinding: 'PASS', gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])) }
  assert.equal(evaluateAcceptance(report).status, 'PASS')
  const missing = { ...report, gates: { ...report.gates } }
  delete missing.gates.factor_h08_quality
  assert.equal(evaluateAcceptance(missing).status, 'FAIL')
  const failed = { ...report, gates: { ...report.gates, factor_h01_obligation: { status: 'FAIL', exitCode: 1 } } }
  assert.equal(evaluateAcceptance(failed).status, 'FAIL')
})
