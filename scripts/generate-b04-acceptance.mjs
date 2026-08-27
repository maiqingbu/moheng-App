import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))

export const REQUIRED_BLOCKING_GATES = [
  'contracts', 'node_tests', 'frontend_vitest', 'typecheck', 'build',
  'architecture', 'module_boundary', 'm0a', 'b01_regression', 'b02_regression',
  'b03_regression', 'cargo_fmt', 'cargo_check', 'cargo_clippy', 'cargo_all_targets',
  'b04_targeted', 'sqlite_integrity', 'fts5', 'sqlite_runtime_profile', 'wal_vacuum',
  'backup_restore', 'deep_integrity', 'long_book_fixture', 'performance', 'git_binding'
]

export function evaluateGitBinding({ beforeCommit, afterCommit, dirtyBefore, dirtyAfter }) {
  const pass = Boolean(beforeCommit) && beforeCommit === afterCommit
    && dirtyBefore === false && dirtyAfter === false
  return { status: pass ? 'PASS' : 'FAIL' }
}

export function countRust(text = '') {
  return [...text.matchAll(/test result: ok[.;] (\d+) passed/g)]
    .reduce((sum, match) => sum + Number(match[1]), 0)
}

export function parseMetrics(logs) {
  const nodeTests = Number(logs.node?.match(/ℹ tests (\d+)/)?.[1] ?? 0)
  const vitestTests = Number(logs.vitest?.match(/Tests\s+(\d+)\s+passed/)?.[1] ?? logs.node?.match(/Tests\s+(\d+)\s+passed/)?.[1] ?? 0)
  const contractCounts = logs.contract?.match(/(\d+) valid accepted, (\d+) invalid rejected/)
  return {
    b04TargetedRustTests: countRust(logs.targeted),
    allRustTests: countRust(logs.allRust),
    nodeTests,
    vitestTests,
    contractValid: contractCounts ? Number(contractCounts[1]) : null,
    contractInvalid: contractCounts ? Number(contractCounts[2]) : null
  }
}

export function evaluateAcceptance({ gateResults, actualMetrics }) {
  const byName = new Map((gateResults?.gates ?? []).map(gate => [gate.name, gate]))
  const gates = REQUIRED_BLOCKING_GATES.map(name => name === 'git_binding'
    ? { name, status: gateResults?.gitBinding?.status ?? 'FAIL', exitCode: gateResults?.gitBinding?.status === 'PASS' ? 0 : 1, blocking: true, log: 'gate-results.json' }
    : byName.get(name) ?? { name, status: 'FAIL', exitCode: 1, blocking: true, log: '' })
  const binding = evaluateGitBinding({
    beforeCommit: gateResults?.sourceCommit,
    afterCommit: gateResults?.sourceCommitAfter,
    dirtyBefore: gateResults?.workingTreeDirtyBefore,
    dirtyAfter: gateResults?.workingTreeDirtyAfter
  })
  const metricConsistency = Boolean(gateResults?.metrics)
    && Object.keys(actualMetrics).every(key => actualMetrics[key] === gateResults.metrics[key])
  const allBlockingPass = gates.every(gate => gate.status === 'PASS' && gate.exitCode === 0)
  return {
    status: allBlockingPass && binding.status === 'PASS' && metricConsistency ? 'PASS' : 'FAIL',
    gates,
    gitBinding: binding,
    metricConsistency: metricConsistency ? 'PASS' : 'FAIL'
  }
}

function digest(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function generateAcceptance(evidenceDir = process.env.B04_EVIDENCE_DIR ?? resolve(root, 'evidence/B04')) {
  const read = name => readFileSync(resolve(evidenceDir, name), 'utf8')
  const gateResults = JSON.parse(read('gate-results.json'))
  const actualMetrics = parseMetrics({
    targeted: read('b04_targeted_test.log'),
    allRust: read('cargo_all_targets.log'),
    node: read('npm_test.log'),
    vitest: read('frontend_vitest.log'),
    contract: read('contracts_gate.log')
  })
  const result = evaluateAcceptance({ gateResults, actualMetrics })
  const files = new Set(readdirSync(evidenceDir))
  files.add('acceptance.json')
  const acceptance = {
    batch: 'B04', milestone: 'M0E', status: result.status,
    sourceCommit: gateResults.sourceCommit,
    workingTreeDirtyBefore: gateResults.workingTreeDirtyBefore,
    workingTreeDirtyAfter: gateResults.workingTreeDirtyAfter,
    gitBinding: result.gitBinding.status,
    gates: result.gates,
    metricConsistency: result.metricConsistency,
    predecessor: { batch: 'B03', status: 'FROZEN_PASS', baselineCommit: 'd39e35ca3a4a9998e69fdc4c22cadaf5e281e494' },
    keyNumbers: actualMetrics,
    evidenceGenerator: { script: 'scripts/generate-b04-acceptance.mjs', digest: digest(fileURLToPath(import.meta.url)) },
    gateRunner: { script: 'scripts/run-b04-evidence-gates.mjs', digest: digest(resolve(root, 'scripts/run-b04-evidence-gates.mjs')) },
    evidence: [...files].sort()
  }
  writeFileSync(resolve(evidenceDir, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const acceptance = generateAcceptance()
  console.log(`B04 acceptance generated from bound gates: ${acceptance.status}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
