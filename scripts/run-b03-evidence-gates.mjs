import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { evaluateGitBinding, parseMetrics } from './generate-b03-acceptance.mjs'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = process.env.B03_EVIDENCE_DIR
  ? resolve(process.env.B03_EVIDENCE_DIR)
  : mkdtempSync(join(tmpdir(), 'moheng-b03-evidence-'))
mkdirSync(evidence, { recursive: true })

const commands = [
  ['contracts', 'npm run verify:contracts', 'contracts_gate.log'],
  ['node_tests', 'npm test', 'npm_test.log'],
  ['frontend_vitest', 'npm run test:store', 'frontend_vitest.log'],
  ['typecheck', 'npm run typecheck', 'typecheck.log'],
  ['build', 'npm run build', 'build.log'],
  ['architecture', 'node scripts/verify-architecture.mjs', 'architecture_gate.log'],
  ['module_boundary', 'npm run verify:boundaries', 'module_boundary.log'],
  ['m0a', 'npm run verify:m0a', 'm0a_gate.log'],
  ['b03_targeted', 'cargo test --manifest-path src-tauri/Cargo.toml --test b03_runtime -- --nocapture', 'b03_targeted_test.log'],
  ['cargo_all_targets', 'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture', 'cargo_all_targets.log'],
  ['cargo_fmt', 'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check', 'cargo_fmt.log'],
  ['cargo_check', 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets', 'cargo_check.log'],
  ['cargo_clippy', 'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings', 'cargo_clippy.log'],
  ['secret_scan', 'node scripts/scan-b03-secrets.mjs', 'secret_scan.log'],
  ['acceptance_generator', 'node --test tests/b03-acceptance-generator.test.mjs', 'acceptance_generator_tests.log']
]

function gitSnapshot() {
  const options = { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
  return {
    commit: execFileSync('git', ['rev-parse', 'HEAD'], options).trim(),
    dirty: execFileSync('git', ['status', '--porcelain=v1', '--untracked-files=all'], options).trim().length > 0
  }
}

const before = gitSnapshot()
const results = []
const logs = new Map()
for (const [name, command, logName] of commands) {
  const startedAt = new Date().toISOString()
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8' })
  const exitCode = result.status ?? 1
  const content = `${result.stdout ?? ''}${result.stderr ?? ''}`
  logs.set(logName, content)
  writeFileSync(resolve(evidence, logName), content)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: name !== 'acceptance_generator', startedAt, finishedAt: new Date().toISOString(), log: logName })
}

// The frozen B01 evidence test materializes a fresh database snapshot while
// checking B01. Revert only those two known generated files so B03's clean-tree
// binding does not turn a harmless regression fixture refresh into a mutation.
for (const path of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) {
  try {
    execFileSync('git', ['restore', '--source', before.commit, '--', path], { cwd: root, stdio: 'ignore' })
  } catch {
    results.push({ name: 'b01_frozen_evidence', command: `git restore --source ${before.commit} -- ${path}`, exitCode: 1, status: 'FAIL', blocking: true, log: 'gate-results.json' })
  }
}

const after = gitSnapshot()
const gitBinding = evaluateGitBinding({
  beforeCommit: before.commit,
  afterCommit: after.commit,
  dirtyBefore: before.dirty,
  dirtyAfter: after.dirty
})
results.push({
  name: 'git_binding',
  command: 'git rev-parse HEAD && git status --porcelain=v1',
  exitCode: gitBinding.status === 'PASS' ? 0 : 1,
  status: gitBinding.status,
  blocking: true,
  log: 'gate-results.json',
  note: gitBinding.reason
})

const targeted = logs.get('b03_targeted_test.log') ?? ''
const allRust = logs.get('cargo_all_targets.log') ?? ''
const node = logs.get('npm_test.log') ?? ''
const vitest = logs.get('frontend_vitest.log') ?? ''
const contract = logs.get('contracts_gate.log') ?? ''
const metrics = parseMetrics({ targeted, allRust, node, vitest, contract })
const finalStatus = results.every(result => !result.blocking || (result.status === 'PASS' && result.exitCode === 0)) && gitBinding.status === 'PASS' ? 'PASS' : 'FAIL'
writeFileSync(resolve(evidence, 'test.log'), [...logs.values()].join('\n'))
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({
  generatedAt: new Date().toISOString(),
  sourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: before.dirty,
  workingTreeDirtyAfter: after.dirty,
  gitBinding,
  finalStatus,
  runnerExitCode: finalStatus === 'PASS' ? 0 : 1,
  metrics,
  gates: results
}, null, 2)}\n`)
console.log(`B03 gates recorded: ${results.filter(result => result.status === 'PASS').length}/${results.length} PASS; gitBinding=${gitBinding.status}; evidence=${evidence}`)
if (finalStatus !== 'PASS') process.exitCode = 1
