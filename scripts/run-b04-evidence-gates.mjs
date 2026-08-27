import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { evaluateGitBinding, parseMetrics } from './generate-b04-acceptance.mjs'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidence = process.env.B04_EVIDENCE_DIR ? resolve(process.env.B04_EVIDENCE_DIR) : mkdtempSync(join(tmpdir(), 'moheng-b04-evidence-'))
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
  ['b04_targeted', 'cargo test --manifest-path src-tauri/Cargo.toml --test b04_ops -- --nocapture', 'b04_targeted_test.log'],
  ['cargo_all_targets', 'cargo test --manifest-path src-tauri/Cargo.toml --all-targets -- --nocapture', 'cargo_all_targets.log'],
  ['cargo_fmt', 'cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check', 'cargo_fmt.log'],
  ['cargo_check', 'cargo check --manifest-path src-tauri/Cargo.toml --all-targets', 'cargo_check.log'],
  ['cargo_clippy', 'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings', 'cargo_clippy.log']
]
function gitSnapshot() {
  const options = { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
  return { commit: execFileSync('git', ['rev-parse', 'HEAD'], options).trim(), dirty: execFileSync('git', ['status', '--porcelain=v1', '--untracked-files=all'], options).trim().length > 0 }
}
const before = gitSnapshot(); const results = []; const logs = new Map()
for (const [name, command, logName] of commands) {
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8', env: { ...process.env, B04_EVIDENCE_DIR: evidence } })
  const content = `${result.stdout ?? ''}${result.stderr ?? ''}`; const exitCode = result.status ?? 1
  logs.set(logName, content); writeFileSync(resolve(evidence, logName), content)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, log: logName })
}
const targetedLog = logs.get('b04_targeted_test.log') ?? ''
const healthEvidenceValid = /B04_FTS_HEALTH_RESULT .*mode=metadata .*full_count_queries=0 .*full_rebuild=false .*status=PASS/.test(targetedLog)
const raceEvidenceValid = /B04_RESTORE_RACE .*new_reader_during_restore=BLOCKED .*maintenance_cleanup=READY status=PASS/.test(targetedLog)
const scaleCommand = process.env.B04_RUN_SCALE === '1'
  ? ['long_book_materialization', 'cargo test --manifest-path src-tauri/Cargo.toml --test b04_scale -- --ignored --nocapture', 'b04_scale_materialization.log']
  : null
const reuseScale = process.env.B04_REUSE_SCALE_EVIDENCE === '1' && existsSync(resolve(evidence, 'b04_scale_materialization.log'))
if (scaleCommand && !reuseScale) {
  const [name, command, logName] = scaleCommand
  const result = spawnSync(command, { cwd: root, shell: true, encoding: 'utf8', env: { ...process.env, B04_EVIDENCE_DIR: evidence } })
  const content = `${result.stdout ?? ''}${result.stderr ?? ''}`; const exitCode = result.status ?? 1
  logs.set(logName, content); writeFileSync(resolve(evidence, logName), content)
  results.push({ name, command, exitCode, status: exitCode === 0 ? 'PASS' : 'FAIL', blocking: true, log: logName })
  const scaleLog = logs.get(logName) ?? ''
  const scalePass = exitCode === 0 && /B04_SCALE_CERTIFICATION_RESULT .*integrity=PASS .*fts_incremental=PASS .*performance=PASS .*backup_restore=PASS .*restore_read_pool=PASS .*vacuum=PASS .*single_chapter_path=PASS status=PASS/.test(scaleLog)
  results.push({ name: 'long_book_scale_certification', command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  for (const gateName of ['fts_incremental_scale', 'scale_performance', 'scale_backup_restore', 'restore_read_pool', 'deep_integrity_scale', 'vacuum_governance']) {
    results.push({ name: gateName, command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  }
  for (const gateName of ['fts_real_text_scale', 'single_chapter_query_perf', 'single_chapter_production_path']) {
    results.push({ name: gateName, command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  }
  results.push({ name: 'restore_maintenance_race', command: 'b04_targeted', exitCode: raceEvidenceValid ? 0 : 1, status: raceEvidenceValid ? 'PASS' : 'FAIL', blocking: true, log: 'b04_targeted_test.log' })
} else if (scaleCommand && reuseScale) {
  const [name, command, logName] = scaleCommand
  const scaleLog = readFileSync(resolve(evidence, logName), 'utf8')
  logs.set(logName, scaleLog)
  const scalePass = /B04_SCALE_CERTIFICATION_RESULT .*integrity=PASS .*fts_incremental=PASS .*performance=PASS .*backup_restore=PASS .*restore_read_pool=PASS .*vacuum=PASS .*single_chapter_path=PASS status=PASS/.test(scaleLog)
  results.push({ name, command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  results.push({ name: 'long_book_scale_certification', command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  for (const gateName of ['fts_incremental_scale', 'scale_performance', 'scale_backup_restore', 'restore_read_pool', 'deep_integrity_scale', 'vacuum_governance', 'fts_real_text_scale', 'single_chapter_query_perf', 'single_chapter_production_path']) {
    results.push({ name: gateName, command, exitCode: scalePass ? 0 : 1, status: scalePass ? 'PASS' : 'FAIL', blocking: true, log: logName })
  }
} else {
  results.push({ name: 'long_book_materialization', command: 'B04_RUN_SCALE=1 npm run verify:b04', exitCode: 1, status: 'FAIL', blocking: true, log: 'b04_scale_materialization.log' })
  for (const gateName of ['long_book_scale_certification', 'fts_incremental_scale', 'scale_performance', 'scale_backup_restore', 'restore_read_pool', 'deep_integrity_scale', 'vacuum_governance']) {
    results.push({ name: gateName, command: 'B04_RUN_SCALE=1 npm run verify:b04', exitCode: 1, status: 'FAIL', blocking: true, log: 'b04_scale_materialization.log' })
  }
  for (const gateName of ['fts_real_text_scale', 'single_chapter_query_perf', 'single_chapter_production_path']) {
    results.push({ name: gateName, command: 'B04_RUN_SCALE=1 npm run verify:b04', exitCode: 1, status: 'FAIL', blocking: true, log: 'b04_scale_materialization.log' })
  }
  results.push({ name: 'restore_maintenance_race', command: 'b04_targeted', exitCode: 1, status: 'FAIL', blocking: true, log: 'b04_targeted_test.log' })
}
const requiredOperations = {
  project_overview: 2000, volume_list: 300, chapter_list: 300, canon_current: 250,
  adjacency: 500, normal_commit: 1000, recovery_readable: 10000
}
const operationMarkers = [...targetedLog.matchAll(/B04_PERF_OPERATION operation=([a-z_]+) samples=(\d+) p50_ms=([0-9.]+) p95_ms=([0-9.]+)/g)]
const performanceEvidenceValid = Object.entries(requiredOperations).every(([operation, threshold]) => {
  const marker = operationMarkers.find(item => item[1] === operation)
  return marker && Number(marker[2]) > 0 && Number(marker[4]) <= threshold
})
const restoreEvidenceValid = /B04_RESTORE_WORKFLOW_RESULT .*maintenance=PASS .*checksum=PASS .*quick_check=PASS .*foreign_key_check=PASS .*deep_integrity=PASS .*receipt=PASS/.test(targetedLog)
const upgradeEvidenceValid = /B04_FTS_UPGRADE_RESULT .*rebuild_triggered=true .*second_open_full_rebuild=false .*status=PASS/.test(targetedLog)
const targetedGate = results.find(item => item.name === 'b04_targeted')
if (targetedGate && (!performanceEvidenceValid || !restoreEvidenceValid || !upgradeEvidenceValid || !healthEvidenceValid || !raceEvidenceValid)) {
  targetedGate.status = 'FAIL'; targetedGate.exitCode = 1
}
for (const path of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) {
  try { execFileSync('git', ['restore', '--source', before.commit, '--', path], { cwd: root, stdio: 'ignore' }) } catch { results.push({ name: 'b01_frozen_evidence', status: 'FAIL', exitCode: 1, blocking: true, log: 'gate-results.json' }) }
}
const aliases = [
  ['b01_regression', 'cargo_all_targets'], ['b02_regression', 'cargo_all_targets'], ['b03_regression', 'cargo_all_targets'],
  ['sqlite_integrity', 'cargo_all_targets'], ['fts5', 'b04_targeted'], ['sqlite_runtime_profile', 'b04_targeted'],
  ['wal_vacuum', 'b04_targeted'], ['backup_restore', 'b04_targeted'], ['restore_workflow', 'b04_targeted'], ['deep_integrity', 'b04_targeted'],
  ['long_book_fixture', 'b04_targeted'], ['fts_upgrade_backfill', 'b04_targeted'], ['fts_metadata_health', 'b04_targeted'], ['performance', 'b04_targeted']
]
for (const [name, source] of aliases) { const gate = results.find(item => item.name === source); results.push({ name, command: gate.command, exitCode: gate.exitCode, status: gate.status, blocking: true, log: gate.log }) }
const after = gitSnapshot(); const gitBinding = evaluateGitBinding({ beforeCommit: before.commit, afterCommit: after.commit, dirtyBefore: before.dirty, dirtyAfter: after.dirty })
results.push({ name: 'git_binding', command: 'git rev-parse HEAD && git status --porcelain=v1', exitCode: gitBinding.status === 'PASS' ? 0 : 1, status: gitBinding.status, blocking: true, log: 'gate-results.json' })
const metrics = parseMetrics({ targeted: logs.get('b04_targeted_test.log') ?? '', allRust: logs.get('cargo_all_targets.log') ?? '', node: logs.get('npm_test.log') ?? '', vitest: logs.get('frontend_vitest.log') ?? '', contract: logs.get('contracts_gate.log') ?? '' })
const finalStatus = results.every(item => item.status === 'PASS' && item.exitCode === 0) && gitBinding.status === 'PASS' ? 'PASS' : 'FAIL'
writeFileSync(resolve(evidence, 'test.log'), [...logs.values()].join('\n'))
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ sourceCommit: before.commit, sourceCommitAfter: after.commit, workingTreeDirtyBefore: before.dirty, workingTreeDirtyAfter: after.dirty, gitBinding, finalStatus, runnerExitCode: finalStatus === 'PASS' ? 0 : 1, metrics, gates: results }, null, 2)}\n`)
console.log(`B04 gates recorded: ${results.filter(item => item.status === 'PASS').length}/${results.length} PASS; gitBinding=${gitBinding.status}; evidence=${evidence}`)
if (finalStatus !== 'PASS') process.exitCode = 1
