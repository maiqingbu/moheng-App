import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { evaluateGitBinding, parseMetrics } from './generate-b02-acceptance.mjs'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = resolve(root, 'evidence/B02')
mkdirSync(evidence, { recursive: true })

const commands = [
  ['b02_targeted_rust', 'cargo test --manifest-path src-tauri/Cargo.toml --test b02_commit_bundle -- --nocapture', 'b02_targeted_test.log'],
  ['cargo_all_targets', 'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture', 'cargo_all_targets.log'],
  ['node_tests', 'npm test', 'npm_test.log'],
  ['typecheck', 'npm run typecheck', 'typecheck.log'],
  ['build', 'npm run build', 'build.log'],
  ['contracts', 'npm run verify:contracts', 'contracts_gate.log'],
  ['architecture', 'npm run verify', 'architecture_gate.log'],
  ['m0a', 'npm run verify:m0a', 'm0a_gate.log'],
  ['cargo_fmt', 'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check', 'cargo_fmt.log'],
  ['cargo_check', 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets', 'cargo_check.log'],
  ['cargo_clippy', 'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings', 'cargo_clippy.log']
]

function gitSnapshot() {
  const options = { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
  return {
    commit: execFileSync('git', ['rev-parse', 'HEAD'], options).trim(),
    dirty: execFileSync('git', ['status', '--porcelain=v1'], options).trim().length > 0
  }
}

const before = gitSnapshot()
const pendingLogs = new Map()
const results = []
if (before.dirty) {
  results.push({ name: 'git_binding', command: 'git rev-parse HEAD && git status --porcelain=v1', exitCode: 1, status: 'FAIL', blocking: true, log: 'evidence/B02/gate-results.json', note: 'working tree was dirty before the first gate' })
} else {
  for (const [name, command, logName] of commands) {
    const startedAt = new Date().toISOString()
    const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8' })
    const exitCode = result.status ?? 1
    pendingLogs.set(logName, `${result.stdout ?? ''}${result.stderr ?? ''}`)
    results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, startedAt, finishedAt: new Date().toISOString(), log: `evidence/B02/${logName}` })
    if (exitCode !== 0) break
  }
  try {
    execFileSync('git', ['restore', '--source', before.commit, '--', 'evidence/B01'], { cwd: root, stdio: 'ignore' })
  } catch {
    // Keep the binding failed if the B01 evidence restore itself is unavailable.
  }
}

let after
try {
  after = gitSnapshot()
} catch {
  after = { commit: '', dirty: true }
}
const gitBinding = evaluateGitBinding({ beforeCommit: before.commit, afterCommit: after.commit, dirtyBefore: before.dirty, dirtyAfter: after.dirty })
if (!results.some(result => result.name === 'git_binding')) {
  results.push({ name: 'git_binding', command: 'git rev-parse HEAD && git status --porcelain=v1', exitCode: gitBinding.status === 'PASS' ? 0 : 1, status: gitBinding.status, blocking: true, log: 'evidence/B02/gate-results.json', note: gitBinding.reason })
}

for (const [name, source, note] of [
  ['frontend_vitest', 'node_tests', 'frontend Vitest is part of npm test'],
  ['module_boundary', 'architecture', 'module boundary is part of npm run verify'],
  ['b01_regression', 'cargo_all_targets', 'B01 regression is included in Rust all-targets'],
  ['sqlite_integrity', 'cargo_all_targets', 'SQLite quick_check/foreign_key_check are asserted by migration regression']
]) {
  const sourceResult = results.find(result => result.name === source)
  results.push({ name, command: sourceResult?.command ?? 'not run because prerequisite gate failed', exitCode: sourceResult?.exitCode ?? 1, status: sourceResult?.status ?? 'FAIL', blocking: true, log: sourceResult?.log ?? '', note })
}

const output = name => pendingLogs.get(name) ?? ''
const metrics = parseMetrics({ targeted: output('b02_targeted_test.log'), allRust: output('cargo_all_targets.log'), node: output('npm_test.log'), contract: output('contracts_gate.log') })
const finalStatus = results.every(result => result.blocking === false || (result.status === 'PASS' && result.exitCode === 0)) && gitBinding.status === 'PASS' ? 'PASS' : 'FAIL'
for (const [logName, content] of pendingLogs) writeFileSync(resolve(evidence, logName), content)
const nodeLines = output('npm_test.log').split('\n')
writeFileSync(resolve(evidence, 'acceptance_generator_tests.log'), `${nodeLines.filter(line => line.includes('ACCEPTANCE-GEN-')).join('\n')}\n`)
writeFileSync(resolve(evidence, 'git_binding_tests.log'), `${nodeLines.filter(line => line.includes('GIT-BINDING-')).join('\n')}\n`)
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ generatedAt: new Date().toISOString(), sourceCommit: before.commit, sourceCommitAfter: after.commit, workingTreeDirtyBefore: before.dirty, workingTreeDirtyAfter: after.dirty, gitBinding, finalStatus, runnerExitCode: finalStatus === 'PASS' ? 0 : 1, metrics, gates: results }, null, 2)}\n`)
console.log(`B02 gates recorded: ${results.filter(result => result.status === 'PASS').length}/${results.length} PASS; gitBinding=${gitBinding.status}`)
if (finalStatus !== 'PASS') process.exitCode = 1
