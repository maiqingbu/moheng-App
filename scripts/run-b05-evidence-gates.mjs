import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { evaluateGitBinding, parseMetrics } from './generate-b05-acceptance.mjs'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = process.env.B05_EVIDENCE_DIR ? resolve(process.env.B05_EVIDENCE_DIR) : mkdtempSync(join(tmpdir(), 'moheng-b05-evidence-'))
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
  ['b05_targeted', 'cargo test --manifest-path src-tauri/Cargo.toml --test b05_narrative -- --nocapture', 'b05_targeted_test.log'],
  ['cargo_all_targets', 'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture', 'cargo_all_targets.log'],
  ['cargo_fmt', 'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check', 'cargo_fmt.log'],
  ['cargo_check', 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets', 'cargo_check.log'],
  ['cargo_clippy', 'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings', 'cargo_clippy.log'],
]
function gitSnapshot() {
  const options = { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
  return { commit: execFileSync('git', ['rev-parse', 'HEAD'], options).trim(), dirty: execFileSync('git', ['status', '--porcelain=v1', '--untracked-files=all'], options).trim().length > 0 }
}
const before = gitSnapshot(); const results = []; const logs = new Map()
for (const [name, command, logName] of commands) {
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8', env: { ...process.env, B05_EVIDENCE_DIR: evidence } })
  const content = `${result.stdout ?? ''}${result.stderr ?? ''}`; const exitCode = result.status ?? 1
  logs.set(logName, content); writeFileSync(resolve(evidence, logName), content)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, log: logName })
}
const targeted = logs.get('b05_targeted_test.log') ?? ''
const markers = {
  migration_0007: /B05_MIGRATION_RESULT .*fresh_0007=PASS .*forbidden_authority_tables=0 status=PASS/.test(targeted) && /B05_MIGRATION_UPGRADE_RESULT .*from=6 to=7 .*status=PASS/.test(targeted),
  payload_contracts: /B05_CONTRACT_RESULT valid=PASS invalid=PASS legacy_generic=REJECTED stable_ids=PASS status=PASS/.test(targeted),
  bookplan_seed_contract: /B05_BOOKPLAN_RESULT seed=PASS approved=PASS opening_hypotheses=PASS conditional_fields=PASS target_length_mapping=PASS status=PASS/.test(targeted),
  task_schema_runtime_parity: /B05_TASK_COMMIT_RESULT metric_null=ACCEPT invalid_status=REJECT malformed_evidence=REJECT malformed_condition=REJECT metric_range=REJECT discrete=ACCEPT schema_runtime_parity=PASS status=PASS/.test(targeted),
  legacy_compatibility: /B05_LEGACY_RESULT migrations_0001_0006=UNCHANGED status=PASS/.test(targeted),
  authority_separation: /B05_REPOSITORY_RESULT typed=PASS populated=PASS bounded=PASS relevant=PASS old_promise=PASS sql_owner=repositories status=PASS/.test(targeted),
  repository_coverage: /B05_REPOSITORY_COVERAGE_RESULT relations=PASS evidence=PASS obligations=PASS foreshadows=PASS mysteries=PASS tasks_by_id=PASS recent_evidence=PASS status=PASS/.test(targeted),
  repository_relevance: /B05_REPOSITORY_RESULT .*populated=PASS .*relevant=PASS .*old_promise=PASS/.test(targeted),
  plan_canon_separation: /B05_AUTHORITY_RESULT plan_artifacts=5 canon_rows_before=[1-9]\d* canon_rows_after=[1-9]\d* canon_digest_equal=true projection_only=true rebuild_source=PASS status=PASS/.test(targeted),
  thread_promotion: /B05_THREAD_RESULT ordinary=DO_NOT_PROMOTE ambient=DO_NOT_PROMOTE trivial=DO_NOT_PROMOTE detail=DO_NOT_PROMOTE promise=PROMOTE relationship=PROMOTE conflict=PROMOTE mystery=PROMOTE foreshadow=PROMOTE future_object=PROMOTE goal=PROMOTE authority=PROMOTE reservation=PROMOTE status=PASS/.test(targeted),
  task_state_contract: /B05_TASK_RESULT .*percentage=REJECTED status=PASS/.test(targeted),
  canon_event_time: /B05_CANON_TIME_RESULT story_time=PASS event_ordinal=PASS source_ref=PASS narrative_order_separate=PASS status=PASS/.test(targeted),
  frontend_contract: /B05_UI_CONTRACT_RESULT shared_fixture=PASS rust_dto=PASS frontend_dto=PASS arc_mandatory=REJECTED status=PASS/.test(targeted),
  forbidden_authority_scan: /B05_ARCHITECTURE_RESULT docs=PASS frozen_migrations=PASS no_second_authority=PASS forbidden_authority_tables=0 legacy_planner_count=0 narrative_runtime_delta_count=0 commit_bundle_v3_count=0 status=PASS/.test(logs.get('architecture_gate.log') ?? ''),
}
for (const [name, pass] of Object.entries(markers)) results.push({ name, command: 'b05_targeted/architecture', exitCode: pass ? 0 : 1, status: pass ? 'PASS' : 'FAIL', blocking: true, log: name === 'forbidden_authority_scan' ? 'architecture_gate.log' : 'b05_targeted_test.log' })
for (const path of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) {
  try { execFileSync('git', ['restore', '--source', before.commit, '--', path], { cwd: root, stdio: 'ignore' }) } catch { results.push({ name: 'b01_frozen_evidence', command: 'git restore B01 evidence', exitCode: 1, status: 'FAIL', blocking: true, log: 'gate-results.json' }) }
}
const after = gitSnapshot(); const gitBinding = evaluateGitBinding({ beforeCommit: before.commit, afterCommit: after.commit, dirtyBefore: before.dirty, dirtyAfter: after.dirty })
results.push({ name: 'git_binding', command: 'git rev-parse HEAD && git status --porcelain=v1', exitCode: gitBinding.status === 'PASS' ? 0 : 1, status: gitBinding.status, blocking: true, log: 'gate-results.json' })
const metrics = parseMetrics({ targeted: logs.get('b05_targeted_test.log'), allRust: logs.get('cargo_all_targets.log'), node: logs.get('npm_test.log'), vitest: logs.get('frontend_vitest.log'), contract: logs.get('contracts_gate.log') })
const finalStatus = results.every(item => item.status === 'PASS' && item.exitCode === 0) && gitBinding.status === 'PASS' ? 'PASS' : 'FAIL'
writeFileSync(resolve(evidence, 'test.log'), [...logs.values()].join('\n'))
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ sourceCommit: before.commit, sourceCommitAfter: after.commit, workingTreeDirtyBefore: before.dirty, workingTreeDirtyAfter: after.dirty, gitBinding, finalStatus, runnerExitCode: finalStatus === 'PASS' ? 0 : 1, metrics, gates: results }, null, 2)}\n`)
console.log(`B05 gates recorded: ${results.filter(item => item.status === 'PASS').length}/${results.length} PASS; gitBinding=${gitBinding.status}; evidence=${evidence}`)
if (finalStatus !== 'PASS') process.exitCode = 1
