import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = process.env.B06_H01_EVIDENCE_DIR ? resolve(process.env.B06_H01_EVIDENCE_DIR) : mkdtempSync(join(tmpdir(), 'moheng-b06-h01-evidence-'))
mkdirSync(evidence, { recursive: true })
const commands = [
  ['contracts', 'npm run verify:contracts', 'contracts_gate.log'],
  ['node_tests', 'npm test', 'npm_test.log'],
  ['frontend_vitest', 'npm run test:store', 'frontend_vitest.log'],
  ['typecheck', 'npm run typecheck', 'typecheck.log'],
  ['build', 'npm run build', 'build.log'],
  ['architecture', 'node scripts/verify-architecture.mjs', 'architecture_gate.log'],
  ['b06_h01_architecture', 'node scripts/verify-b06-h01-architecture.mjs', 'b06_h01_architecture.log'],
  ['module_boundary', 'npm run verify:boundaries', 'module_boundary.log'],
  ['m0a', 'npm run verify:m0a', 'm0a_gate.log'],
  ['b05_regression', 'cargo test --manifest-path src-tauri/Cargo.toml --test b05_narrative -- --nocapture', 'b05_regression.log'],
  ['b06_h01_targeted', 'cargo test --manifest-path src-tauri/Cargo.toml --test b06_h01 -- --nocapture', 'b06_h01_targeted.log'],
  ['cargo_all_targets', 'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture', 'cargo_all_targets.log'],
  ['cargo_fmt', 'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check', 'cargo_fmt.log'],
  ['cargo_check', 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets', 'cargo_check.log'],
  ['cargo_clippy', 'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings', 'cargo_clippy.log'],
]
if (process.env.B06_H01_RUN_SCALE === '1') commands.splice(11, 0, ['b06_h01_scale', 'cargo test --manifest-path src-tauri/Cargo.toml --test b06_scale -- --ignored --nocapture', 'b06_h01_scale.log'])
const snapshot = () => ({ commit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(), dirty: execFileSync('git', ['status', '--porcelain=v1', '--untracked-files=all'], { cwd: root, encoding: 'utf8' }).trim().length > 0 })
const before = snapshot(); const results = []; const logs = new Map()
for (const [name, command, file] of commands) {
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8', env: { ...process.env, B06_H01_EVIDENCE_DIR: evidence } })
  const content = `${result.stdout ?? ''}${result.stderr ?? ''}`; const exitCode = result.status ?? 1
  logs.set(file, content); writeFileSync(resolve(evidence, file), content)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, log: file })
}
const targeted = logs.get('b06_h01_targeted.log') ?? ''
const architecture = logs.get('b06_h01_architecture.log') ?? ''
const marker = (pattern, log = targeted) => pattern.test(log)
const markers = {
  b06_current_state: marker(/B06_CURRENT_STATE_RESULT .*populated=PASS .*bounded=PASS .*provenance=PASS .*status=PASS/),
  b06_narrative_time: marker(/B06_CURRENT_STATE_RESULT .*status=PASS/),
  b06_continuity: marker(/B06_CONTINUITY_RESULT .*open=PASS .*suspended=PASS .*resolved=PASS .*abandoned=PASS .*status=PASS/),
  b06_resume_condition: marker(/B06_RESUME_RESULT .*unknown=REJECT .*malformed=REJECT status=PASS/),
  b06_cross_volume_promise: marker(/B06_CONTINUITY_RESULT .*cross_volume=PASS .*exact_evidence=PASS/),
  b06_volume_progress: marker(/B06_CONTEXT_RESULT .*progress=PASS/),
  b06_context_manifest: marker(/B06_CONTEXT_RESULT .*manifest=PASS .*bounded_hard_context=PASS/),
  b06_state_digest: marker(/B06_CONTEXT_RESULT .*digest_deterministic=PASS .*unrelated_revision_valid=PASS .*related_change_stale=PASS/),
  b06_bounded_retrieval: marker(/B06_CURRENT_STATE_RESULT .*bounded=PASS/),
  b06_frontend_projection: marker(/B06_H01_INTEGRATION_RESULT .*tauri=PASS .*repository=PASS .*context=PASS/),
  b06_deterministic_digest: marker(/B06_CONTEXT_RESULT .*digest_deterministic=PASS/),
  b06_compiler_performance: Boolean(process.env.B06_H01_RUN_SCALE === '1') && /B06_SCALE_RESULT .*compiler_p95_ms=([0-9.]+) .*hard_context_bounded=true status=PASS/.test(logs.get('b06_h01_scale.log') ?? ''),
  h01_project_scope: marker(/H01_RESULT .*scope=PASS/),
  h01_capability_registry: marker(/H01_RESULT .*registry=PASS/),
  h01_support_planner: marker(/H01_RESULT .*max_steps=7/),
  h01_executor: marker(/H01_RESULT .*executor=PASS/),
  h01_sql_negative: marker(/H01_RESULT .*sql_negative=PASS/),
  h01_read_only_boundary: marker(/H01_RESULT .*write_capability=0/),
  h01_diagnostics: marker(/H01_RESULT .*diagnostics=PASS/),
  h01_evidence_ledger: marker(/H01_RESULT .*evidence_ledger=PASS/),
  h01_checkpoint_resume: marker(/H01_RESULT .*checkpoint_safe=PASS/),
  h01_checkpoint_stale: marker(/H01_RESULT .*checkpoint_stale=PASS/),
  h01_bounded_replan: marker(/H01_RESULT .*bounded_replan=PASS/),
  h01_session_receipt: marker(/B06_H01_INTEGRATION_RESULT .*support_receipt=PASS .*durable_checkpoint=PASS/),
  h01_bounded_query: marker(/H01_RESULT .*bounded_query=PASS/),
  migration_0008: marker(/H01_MIGRATION_RESULT .*support_checkpoint=PASS .*additive=PASS .*narrative_authority=UNCHANGED status=PASS/),
  b05_regression: results.find(item => item.name === 'b05_regression')?.status === 'PASS',
  architecture_boundary: /B06_H01_ARCHITECTURE_RESULT .*legacyPlannerCount=0 .*narrativeRuntimeDeltaCount=0 .*commitBundleV3Count=0 .*secondAuthorityCount=0 .*tsAuthoritativeCompilerCount=0 .*narrativeRawSqlCount=0 .*harnessRawSqlCount=0 .*h01WriteCapabilityCount=0 .*skillRuntimeIntegrationCount=0 .*b07SymbolCount=0 .*b08SymbolCount=0 status=PASS/.test(architecture),
}
for (const [name, pass] of Object.entries(markers)) results.push({ name, command: 'b06_h01_targeted/architecture', exitCode: pass ? 0 : 1, status: pass ? 'PASS' : 'FAIL', blocking: true, log: name === 'architecture_boundary' ? 'b06_h01_architecture.log' : 'b06_h01_targeted.log' })
const after = snapshot()
const sourceBinding = { status: before.commit === after.commit && !before.dirty && !after.dirty ? 'PASS' : 'FAIL', beforeCommit: before.commit, afterCommit: after.commit, dirtyBefore: before.dirty, dirtyAfter: after.dirty }
results.push({ name: 'source_binding', command: 'git rev-parse HEAD && git status --porcelain=v1', exitCode: sourceBinding.status === 'PASS' ? 0 : 1, status: sourceBinding.status, blocking: true, log: 'gate-results.json' })
const finalStatus = results.every(item => item.status === 'PASS' && item.exitCode === 0) ? 'PASS' : 'FAIL'
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ batch: 'B06_H01', sourceCommit: before.commit, sourceCommitAfter: after.commit, workingTreeDirtyBefore: before.dirty, workingTreeDirtyAfter: after.dirty, sourceBinding, finalStatus, runnerExitCode: finalStatus === 'PASS' ? 0 : 1, gates: results }, null, 2)}\n`)
console.log(`B06_H01 gates recorded: ${results.filter(item => item.status === 'PASS').length}/${results.length} PASS; sourceBinding=${sourceBinding.status}; evidence=${evidence}`)
if (finalStatus !== 'PASS') process.exitCode = 1
