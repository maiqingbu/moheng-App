import test from 'node:test'
import assert from 'node:assert/strict'
import { evaluateAcceptance, REQUIRED_BLOCKING_GATES } from '../scripts/generate-b04-acceptance.mjs'

function passingResults() {
  const metrics = { b04TargetedRustTests: 10, allRustTests: 140, nodeTests: 25, vitestTests: 17, contractValid: 6, contractInvalid: 24 }
  return {
    sourceCommit: 'a'.repeat(40), sourceCommitAfter: 'a'.repeat(40), workingTreeDirtyBefore: false, workingTreeDirtyAfter: false,
    gitBinding: { status: 'PASS' }, metrics,
    gates: REQUIRED_BLOCKING_GATES.filter(name => name !== 'git_binding').map(name => ({ name, status: 'PASS', exitCode: 0, blocking: true, log: `${name}.log` }))
  }
}

test('B04 acceptance fails when a required gate is missing', () => {
  const results = passingResults()
  results.gates = results.gates.filter(gate => gate.name !== 'fts5')
  assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
})

test('B04 acceptance requires the release-scale materialization gate', () => {
  const results = passingResults()
  results.gates = results.gates.filter(gate => gate.name !== 'long_book_materialization')
  assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
})

for (const gateName of ['deep_integrity', 'performance', 'git_binding']) {
  test(`B04 acceptance fails when ${gateName} fails`, () => {
    const results = passingResults()
    if (gateName === 'git_binding') results.gitBinding.status = 'FAIL'
    else results.gates.find(gate => gate.name === gateName).status = 'FAIL'
    assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
  })
}

for (const gateName of ['long_book_scale_certification', 'fts_incremental_scale', 'scale_performance', 'scale_backup_restore', 'restore_read_pool', 'deep_integrity_scale', 'vacuum_governance', 'fts_upgrade_backfill', 'fts_real_text_scale', 'single_chapter_query_perf', 'restore_maintenance_race', 'fts_metadata_health', 'single_chapter_production_path']) {
  test(`B04 acceptance fails when ${gateName} fails`, () => {
    const results = passingResults()
    results.gates.find(gate => gate.name === gateName).status = 'FAIL'
    assert.equal(evaluateAcceptance({ gateResults: results, actualMetrics: results.metrics }).status, 'FAIL')
  })
}
