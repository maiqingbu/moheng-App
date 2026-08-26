import { spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

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

const results = []
for (const [name, command, logName] of commands) {
  const startedAt = new Date().toISOString()
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8' })
  const exitCode = result.status ?? 1
  writeFileSync(resolve(evidence, logName), `${result.stdout ?? ''}${result.stderr ?? ''}`)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, startedAt, finishedAt: new Date().toISOString(), log: `evidence/B02/${logName}` })
  if (exitCode !== 0) break
}

for (const [name, source, note] of [
  ['frontend_vitest', 'node_tests', 'frontend Vitest is part of npm test'],
  ['b01_regression', 'cargo_all_targets', 'B01 regression is included in Rust all-targets'],
  ['sqlite_integrity', 'cargo_all_targets', 'SQLite quick_check/foreign_key_check are asserted by migration regression']
]) {
  const sourceResult = results.find(result => result.name === source)
  results.push({ name, command: sourceResult?.command ?? 'not run because prerequisite gate failed', exitCode: sourceResult?.exitCode ?? 1, status: sourceResult?.status ?? 'FAIL', blocking: true, log: sourceResult?.log ?? '', note })
}

writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ generatedAt: new Date().toISOString(), gates: results }, null, 2)}\n`)
console.log(`B02 gates recorded: ${results.filter(result => result.status === 'PASS').length}/${results.length} PASS`)
if (results.some(result => result.blocking !== false && result.status !== 'PASS')) process.exitCode = 1
