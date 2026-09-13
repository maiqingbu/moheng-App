import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))

export const REQUIRED_BLOCKING_GATES = [
  'contracts', 'node_tests', 'frontend_vitest', 'typecheck', 'build',
  'architecture', 'module_boundary', 'm0a', 'b01_regression', 'b02_regression',
  'cargo_fmt', 'cargo_check', 'cargo_clippy', 'cargo_all_targets',
  'b03_targeted', 'sqlite_integrity', 'unknown_budget', 'settlement_consistency',
  'stable_id_regression', 'secret_provider_wiring', 'secret_scan',
  'unknown_resolution', 'budget_runtime', 'modelrun_recovery', 'lease_runtime',
  'lease_kernel_invariant', 'lease_time_boundary', 'recovery_matrix',
  'recovery_raw_reopen', 'recovery_completed_reopen', 'unknown_chain_resolution',
  'lease_holder_ownership', 'git_binding'
]

export function evaluateGitBinding({ beforeCommit, afterCommit, dirtyBefore, dirtyAfter }) {
  const pass = Boolean(beforeCommit) && beforeCommit === afterCommit
    && dirtyBefore === false && dirtyAfter === false
  return {
    status: pass ? 'PASS' : 'FAIL',
    reason: pass ? 'commit and clean-tree state remained unchanged' : 'commit changed or working tree was dirty'
  }
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
    b03TargetedRustTests: countRust(logs.targeted),
    allRustTests: countRust(logs.allRust),
    nodeTests,
    vitestTests,
    contractValid: contractCounts ? Number(contractCounts[1]) : null,
    contractInvalid: contractCounts ? Number(contractCounts[2]) : null
  }
}

function metricsEqual(actual, declared) {
  return Boolean(declared) && Object.keys(actual).every(key => actual[key] === declared[key])
}

export function evaluateAcceptance({ gateResults, actualMetrics }) {
  const byName = new Map((gateResults?.gates ?? []).map(gate => [gate.name, gate]))
  const gates = REQUIRED_BLOCKING_GATES.map(name => {
    if (name === 'git_binding') {
      return {
        name,
        status: gateResults?.gitBinding?.status ?? 'FAIL',
        exitCode: gateResults?.gitBinding?.status === 'PASS' ? 0 : 1,
        blocking: true,
        log: 'gate-results.json'
      }
    }
    return byName.get(name) ?? { name, status: 'FAIL', exitCode: 1, blocking: true, log: '' }
  })
  const binding = evaluateGitBinding({
    beforeCommit: gateResults?.sourceCommit,
    afterCommit: gateResults?.sourceCommitAfter,
    dirtyBefore: gateResults?.workingTreeDirtyBefore,
    dirtyAfter: gateResults?.workingTreeDirtyAfter
  })
  const metricConsistency = metricsEqual(actualMetrics, gateResults?.metrics)
  const allBlockingPass = gates.every(gate => gate.blocking === false || (gate.status === 'PASS' && gate.exitCode === 0))
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

export function generateAcceptance(evidenceDir = process.env.B03_EVIDENCE_DIR ?? resolve(root, 'evidence/B03')) {
  const read = name => readFileSync(resolve(evidenceDir, name), 'utf8')
  const gateResults = JSON.parse(read('gate-results.json'))
  const actualMetrics = parseMetrics({
    targeted: read('b03_targeted_test.log'),
    allRust: read('cargo_all_targets.log'),
    node: read('npm_test.log'),
    vitest: read('frontend_vitest.log'),
    contract: read('contracts_gate.log')
  })
  const result = evaluateAcceptance({ gateResults, actualMetrics })
  const evidenceFiles = new Set(readdirSync(evidenceDir))
  evidenceFiles.add('acceptance.json')
  const acceptance = {
    batch: 'B03',
    milestone: 'M0D',
    status: result.status,
    sourceCommit: gateResults.sourceCommit,
    workingTreeDirtyBefore: gateResults.workingTreeDirtyBefore,
    workingTreeDirtyAfter: gateResults.workingTreeDirtyAfter,
    gitBinding: result.gitBinding.status,
    gates: result.gates,
    metricConsistency: result.metricConsistency,
    predecessor: {
      batch: 'B02',
      status: 'FROZEN_PASS',
      baselineCommit: '5a0f27cc59f2a182e637ef713b26a827ea002ef8'
    },
    keyNumbers: actualMetrics,
    evidenceGenerator: {
      script: 'scripts/generate-b03-acceptance.mjs',
      digest: digest(fileURLToPath(import.meta.url))
    },
    gateRunner: {
      script: 'scripts/run-b03-evidence-gates.mjs',
      digest: digest(resolve(root, 'scripts/run-b03-evidence-gates.mjs'))
    },
    gateRunnerExitCode: gateResults.runnerExitCode ?? (result.status === 'PASS' ? 0 : 1),
    evidence: [...evidenceFiles].sort()
  }
  writeFileSync(resolve(evidenceDir, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const acceptance = generateAcceptance()
  console.log(`B03 acceptance generated from bound gates: ${acceptance.status}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
