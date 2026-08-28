import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))

export const REQUIRED_BLOCKING_GATES = [
  'contracts', 'node_tests', 'frontend_vitest', 'typecheck', 'build', 'architecture',
  'module_boundary', 'm0a', 'b05_targeted', 'cargo_fmt', 'cargo_check', 'cargo_clippy',
  'cargo_all_targets', 'migration_0007', 'payload_contracts', 'legacy_compatibility',
  'authority_separation', 'thread_promotion', 'task_state_contract', 'canon_event_time',
  'frontend_contract', 'forbidden_authority_scan', 'scale_evidence_binding', 'git_binding',
]

export function evaluateGitBinding({ beforeCommit, afterCommit, dirtyBefore, dirtyAfter }) {
  return { status: beforeCommit && beforeCommit === afterCommit && dirtyBefore === false && dirtyAfter === false ? 'PASS' : 'FAIL' }
}

export function parseMetrics(logs) {
  const countRust = text => [...(text ?? '').matchAll(/test result: ok[.;] (\d+) passed/g)].reduce((sum, match) => sum + Number(match[1]), 0)
  const contract = logs.contract?.match(/(\d+) valid accepted, (\d+) invalid rejected/)
  return {
    b05TargetedRustTests: countRust(logs.targeted),
    allRustTests: countRust(logs.allRust),
    nodeTests: Number(logs.node?.match(/ℹ tests (\d+)/)?.[1] ?? 0),
    vitestTests: Number(logs.vitest?.match(/Tests\s+(\d+)\s+passed/)?.[1] ?? 0),
    contractValid: contract ? Number(contract[1]) : null,
    contractInvalid: contract ? Number(contract[2]) : null,
  }
}

export function evaluateAcceptance({ gateResults, actualMetrics }) {
  const byName = new Map((gateResults?.gates ?? []).map(gate => [gate.name, gate]))
  const gates = REQUIRED_BLOCKING_GATES.map(name => name === 'git_binding'
    ? { name, status: gateResults?.gitBinding?.status ?? 'FAIL', exitCode: gateResults?.gitBinding?.status === 'PASS' ? 0 : 1, blocking: true, log: 'gate-results.json' }
    : byName.get(name) ?? { name, status: 'FAIL', exitCode: 1, blocking: true, log: '' })
  const binding = evaluateGitBinding(gateResults ?? {})
  const metricConsistency = Boolean(gateResults?.metrics) && Object.keys(actualMetrics).every(key => actualMetrics[key] === gateResults.metrics[key])
  return { status: gates.every(gate => gate.status === 'PASS' && gate.exitCode === 0) && binding.status === 'PASS' && metricConsistency ? 'PASS' : 'FAIL', gates, gitBinding: binding, metricConsistency: metricConsistency ? 'PASS' : 'FAIL' }
}

function digest(path) { return createHash('sha256').update(readFileSync(path)).digest('hex') }

export function generateAcceptance(evidenceDir = process.env.B05_EVIDENCE_DIR ?? resolve(root, 'evidence/B05')) {
  const read = name => readFileSync(resolve(evidenceDir, name), 'utf8')
  const gateResults = JSON.parse(read('gate-results.json'))
  const actualMetrics = parseMetrics({ targeted: read('b05_targeted_test.log'), allRust: read('cargo_all_targets.log'), node: read('npm_test.log'), vitest: read('frontend_vitest.log'), contract: read('contracts_gate.log') })
  const result = evaluateAcceptance({ gateResults, actualMetrics })
  const files = new Set(readdirSync(evidenceDir)); files.add('acceptance.json')
  const acceptance = {
    batch: 'B05', milestone: 'M1-R1-foundation', status: result.status,
    sourceCommit: gateResults.sourceCommit,
    workingTreeDirtyBefore: gateResults.workingTreeDirtyBefore,
    workingTreeDirtyAfter: gateResults.workingTreeDirtyAfter,
    gitBinding: result.gitBinding.status,
    metricConsistency: result.metricConsistency,
    gates: result.gates,
    keyNumbers: actualMetrics,
    predecessor: { batch: 'B04', status: 'FROZEN_PASS', baselineCommit: '77f39aedc9bdb90070715f0399c6e6e1ea5eda10' },
    evidenceGenerator: { script: 'scripts/generate-b05-acceptance.mjs', digest: digest(fileURLToPath(import.meta.url)) },
    gateRunner: { script: 'scripts/run-b05-evidence-gates.mjs', digest: digest(resolve(root, 'scripts/run-b05-evidence-gates.mjs')) },
    evidence: [...files].sort(),
  }
  writeFileSync(resolve(evidenceDir, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const acceptance = generateAcceptance()
  console.log(`B05 acceptance generated from bound gates: ${acceptance.status}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
