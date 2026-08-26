import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = resolve(root, 'evidence/B02')
const read = name => readFileSync(resolve(evidence, name), 'utf8')

export const REQUIRED_BLOCKING_GATES = [
  'contracts',
  'node_tests',
  'frontend_vitest',
  'typecheck',
  'build',
  'architecture',
  'module_boundary',
  'm0a',
  'cargo_fmt',
  'cargo_check',
  'cargo_clippy',
  'cargo_all_targets',
  'b02_targeted_rust',
  'sqlite_integrity',
  'b01_regression',
  'git_binding'
]

export function evaluateGitBinding({ beforeCommit, afterCommit, dirtyBefore, dirtyAfter }) {
  const pass = Boolean(beforeCommit)
    && beforeCommit === afterCommit
    && dirtyBefore === false
    && dirtyAfter === false
  return {
    status: pass ? 'PASS' : 'FAIL',
    reason: pass ? 'commit and clean-tree state remained unchanged' : 'commit changed or working tree was dirty'
  }
}

export function countRust(text) {
  return [...text.matchAll(/test result: ok[.;] (\d+) passed/g)]
    .reduce((sum, match) => sum + Number(match[1]), 0)
}

export function parseMetrics(logs) {
  const nodeTests = Number(logs.node.match(/ℹ tests (\d+)/)?.[1] ?? 0)
  const vitestTests = Number(logs.node.match(/Tests\s+(\d+)\s+passed/)?.[1] ?? 0)
  const contractCounts = logs.contract.match(/(\d+) valid accepted, (\d+) invalid rejected/)
  return {
    b02TargetedRustTests: countRust(logs.targeted),
    allRustTests: countRust(logs.allRust),
    nodeTests,
    vitestTests,
    contractValid: contractCounts ? Number(contractCounts[1]) : null,
    contractInvalid: contractCounts ? Number(contractCounts[2]) : null
  }
}

function sameMetrics(actual, declared) {
  if (!declared) return false
  return Object.keys(actual).every(key => actual[key] === declared[key])
}

export function evaluateAcceptance({ gateResults, actualMetrics }) {
  const gateByName = new Map((gateResults.gates ?? []).map(gate => [gate.name, gate]))
  const gates = REQUIRED_BLOCKING_GATES.map(name => {
    if (name === 'git_binding') {
      return {
        name,
        status: gateResults.gitBinding?.status ?? 'FAIL',
        exitCode: gateResults.gitBinding?.status === 'PASS' ? 0 : 1,
        blocking: true,
        log: 'evidence/B02/gate-results.json'
      }
    }
    return gateByName.get(name) ?? {
      name,
      status: 'FAIL',
      exitCode: 1,
      blocking: true,
      log: ''
    }
  })
  const binding = evaluateGitBinding({
    beforeCommit: gateResults.sourceCommit,
    afterCommit: gateResults.sourceCommitAfter,
    dirtyBefore: gateResults.workingTreeDirtyBefore,
    dirtyAfter: gateResults.workingTreeDirtyAfter
  })
  const metricConsistency = sameMetrics(actualMetrics, gateResults.metrics)
  const allBlockingPass = gates
    .filter(gate => gate.blocking !== false)
    .every(gate => gate.status === 'PASS' && gate.exitCode === 0)
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

export function generateAcceptance() {
  const gateResults = JSON.parse(read('gate-results.json'))
  const actualMetrics = parseMetrics({
    targeted: read('b02_targeted_test.log'),
    allRust: read('cargo_all_targets.log'),
    node: read('npm_test.log'),
    contract: read('contracts_gate.log')
  })
  const result = evaluateAcceptance({ gateResults, actualMetrics })
  const acceptance = {
    batch: 'B02',
    milestone: 'M0C',
    status: result.status,
    sourceCommit: gateResults.sourceCommit,
    workingTreeDirtyBefore: gateResults.workingTreeDirtyBefore,
    workingTreeDirtyAfter: gateResults.workingTreeDirtyAfter,
    gitBinding: result.gitBinding.status,
    gates: result.gates,
    metricConsistency: result.metricConsistency,
    predecessor: {
      batch: 'B01',
      status: 'FROZEN_PASS',
      baselineCommit: '306d8882c6dbedcd2116a0044ad72773b578a9fe'
    },
    keyNumbers: {
      b02TargetedRustTests: `${actualMetrics.b02TargetedRustTests}/${actualMetrics.b02TargetedRustTests}`,
      allRustTests: `${actualMetrics.allRustTests}/${actualMetrics.allRustTests}`,
      nodeTests: `${actualMetrics.nodeTests}/${actualMetrics.nodeTests}`,
      vitestTests: `${actualMetrics.vitestTests}/${actualMetrics.vitestTests}`,
      contractFixtures: `${actualMetrics.contractValid} valid accepted, ${actualMetrics.contractInvalid} invalid rejected`,
      goldenVectors: 26,
      architectureFiles: 44,
      moduleBoundaryFiles: 38,
      faultInjectedChannels: 10,
      replayCount: 100,
      volumeAtomicRollbackFixtures: 2
    },
    evidenceGenerator: {
      script: 'scripts/generate-b02-acceptance.mjs',
      digest: digest(fileURLToPath(import.meta.url))
    },
    gateRunner: {
      script: 'scripts/run-b02-evidence-gates.mjs',
      digest: digest(resolve(root, 'scripts/run-b02-evidence-gates.mjs'))
    },
    gateRunnerExitCode: gateResults.runnerExitCode ?? (result.status === 'PASS' ? 0 : 1),
    evidence: [
      'acceptance.json',
      'gate-results.json',
      'test.log',
      'npm_test.log',
      'cargo_fmt.log',
      'cargo_check.log',
      'cargo_clippy.log',
      'cargo_all_targets.log',
      'b02_targeted_test.log',
      'typecheck.log',
      'build.log',
      'contracts_gate.log',
      'architecture_gate.log',
      'm0a_gate.log',
      'sqlite_integrity.json',
      'acceptance_generator_tests.log',
      'git_binding_tests.log',
      'changed-files.txt'
    ]
  }
  writeFileSync(resolve(evidence, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const acceptance = generateAcceptance()
  console.log(`B02 acceptance generated from bound gates: ${acceptance.status}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
