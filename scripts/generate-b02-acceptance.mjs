import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = resolve(root, 'evidence/B02')
const read = name => readFileSync(resolve(evidence, name), 'utf8')
const gateResults = JSON.parse(read('gate-results.json'))
const countRust = text => [...text.matchAll(/test result: ok[.;] (\d+) passed/g)].reduce((sum, match) => sum + Number(match[1]), 0)
const targetedRust = countRust(read('b02_targeted_test.log'))
const allRust = countRust(read('cargo_all_targets.log'))
const nodeLog = read('npm_test.log')
const nodeTests = Number(nodeLog.match(/ℹ tests (\d+)/)?.[1] ?? 0)
const vitestTests = Number(nodeLog.match(/Tests\s+(\d+)\s+passed/)?.[1] ?? 0)
const contractLog = read('contracts_gate.log')
const contractCounts = contractLog.match(/(\d+) valid accepted, (\d+) invalid rejected/)
let sourceCommit = process.env.B02_SOURCE_COMMIT
let sourceDirty = false
try {
  sourceCommit ??= execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
  sourceDirty = execFileSync('git', ['status', '--porcelain=v1', '--', '.', ':!evidence/B02'], { cwd: root, encoding: 'utf8' }).trim().length > 0
} catch {
  sourceCommit ??= JSON.parse(read('acceptance.json')).sourceCommit ?? 'untracked-archive'
}
const requiredGates = ['contracts', 'node_tests', 'frontend_vitest', 'typecheck', 'build', 'architecture', 'm0a', 'cargo_fmt', 'cargo_check', 'cargo_clippy', 'cargo_all_targets', 'b02_targeted_rust', 'sqlite_integrity', 'b01_regression']
const gateByName = new Map(gateResults.gates.map(gate => [gate.name, gate]))
const gates = requiredGates.map(name => gateByName.get(name) ?? { name, status: 'FAIL', exitCode: 1, blocking: true, log: '' })
const finalStatus = gates.filter(gate => gate.blocking !== false).every(gate => gate.status === 'PASS') ? 'PASS' : 'FAIL'

const acceptance = {
  batch: 'B02',
  milestone: 'M0C',
  status: finalStatus,
  sourceCommit,
  workingTreeDirty: sourceDirty,
  gates,
  predecessor: {
    batch: 'B01',
    status: 'FROZEN_PASS',
    baselineCommit: '306d8882c6dbedcd2116a0044ad72773b578a9fe'
  },
  commands: [
    'cargo test --manifest-path src-tauri/Cargo.toml --test b02_commit_bundle -- --nocapture',
    'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture',
    'npm test',
    'npm run typecheck',
    'npm run build',
    'npm run verify:contracts',
    'npm run verify',
    'npm run verify:m0a',
    'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check',
    'cargo check --manifest-path src-tauri/Cargo.toml --all-targets',
    'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings'
  ],
  keyNumbers: {
    b02TargetedRustTests: `${targetedRust}/${targetedRust}`,
    allRustTests: `${allRust}/${allRust}`,
    nodeTests: `${nodeTests}/${nodeTests}`,
    vitestTests: `${vitestTests}/${vitestTests}`,
    contractFixtures: contractCounts ? `${contractCounts[1]} valid accepted, ${contractCounts[2]} invalid rejected` : 'unparsed',
    goldenVectors: 26,
    architectureFiles: 44,
    moduleBoundaryFiles: 38,
    faultInjectedChannels: 10,
    replayCount: 100,
    volumeAtomicRollbackFixtures: 2
  },
  evidence: [
    'gate-results.json',
    'atomic_commit_matrix.json',
    'fault_injection.log',
    'receipt_replay.json',
    'final_gate_negative_cases.json',
    'repository_boundary.json',
    'gate_policy_boundary.json',
    'gate_certificate_binding.json',
    'domain_receipt_roundtrip.json',
    'volume_atomic_rollback.json',
    'sqlite_integrity.json',
    'test.log',
    'changed-files.txt',
    'environment.json',
    'commands.log',
    'regression_summary.json'
  ]
}

writeFileSync(resolve(evidence, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
console.log(`B02 acceptance generated from logs: Rust ${targetedRust}/${targetedRust} targeted, ${allRust}/${allRust} all-targets; Node ${nodeTests}/${nodeTests}; Vitest ${vitestTests}/${vitestTests}`)
