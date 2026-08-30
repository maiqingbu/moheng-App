import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '..')
const evidence = resolve(process.env.B07_H02_EVIDENCE_DIR ?? '/tmp/moheng-b07-h02-evidence')
mkdirSync(evidence, { recursive: true })

const beforeCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const dirtyBefore = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0
const generatedRegressionArtifacts = ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']

function run(name, command, args, cwd = root) {
  const env = name === 'b06_h01_acceptance' ? { ...process.env, B06_H01_REUSE_FROZEN_SCALE: '1' } : process.env
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', env })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  writeFileSync(resolve(evidence, `${name}.log`), output)
  return { name, status: result.status === 0 ? 'PASS' : 'FAIL', exitCode: result.status ?? 1, output }
}

const b07 = run('b07_targeted', 'cargo', ['test', '--test', 'b07_h02', '--', '--nocapture'], resolve(root, 'src-tauri'))
const b06 = run('b06_h01_regression', 'cargo', ['test', '--test', 'b06_h01', '--', '--nocapture'], resolve(root, 'src-tauri'))
const fmt = run('cargo_fmt', 'cargo', ['fmt', '--all', '--', '--check'], resolve(root, 'src-tauri'))
const check = run('cargo_check', 'cargo', ['check', '--all-targets'], resolve(root, 'src-tauri'))
const clippy = run('cargo_clippy', 'cargo', ['clippy', '--all-targets', '--all-features', '--', '-D', 'warnings'], resolve(root, 'src-tauri'))
const frontend = run('frontend', 'npm', ['run', 'typecheck'])
const nodeFull = run('node_full', 'npm', ['test'])
const build = run('frontend_build', 'npm', ['run', 'build'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const vitest = run('vitest_full', 'npm', ['run', 'test:store'])
const ui = run('ui_b07_h02', 'npx', ['vitest', 'run', 'tests/store/b07-h02-frontend.test.ts'])
const b05 = run('b05_regression', 'npm', ['run', 'verify:b05'])
const b06Acceptance = run('b06_h01_acceptance', 'npm', ['run', 'verify:b06-h01'])
const b06Architecture = run('b06_h01_architecture', 'node', ['scripts/verify-b06-h01-architecture.mjs'])
const m0a = run('m0a', 'node', ['scripts/verify-m0a-evidence.mjs'])
const cargoAll = run('cargo_all_targets', 'cargo', ['test', '--all-targets'], resolve(root, 'src-tauri'))
const architecture = run('architecture', 'node', ['scripts/verify-b07-h02-architecture.mjs'])
const acceptanceTests = run('acceptance_generator_tests', 'node', ['--test', 'tests/b07-h02-acceptance-generator.test.mjs'])
const semanticInjections = run('semantic_injections', 'node', ['--test', 'tests/b07-h02-semantic-injection.test.mjs'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const nonScaleReady = [b07, b06, fmt, check, clippy, frontend, nodeFull, build, contracts, vitest, ui, b05, b06Acceptance, b06Architecture, m0a, architecture, acceptanceTests, semanticInjections, moduleBoundary].every(result => result.status === 'PASS')
const scale = nonScaleReady ? run('b07_h02_scale', 'cargo', ['test', '--test', 'b07_scale', '--', '--ignored', '--nocapture'], resolve(root, 'src-tauri')) : { name: 'b07_h02_scale', status: 'FAIL', exitCode: 1, output: 'SKIPPED: non-scale gates are not PASS' }
if (!nonScaleReady) writeFileSync(resolve(evidence, 'b07_h02_scale.log'), scale.output)

// B01's live-schema evidence test intentionally regenerates two frozen files.
// Restore only these explicit generated outputs so a clean release run remains
// source-bound; no source or unrelated user file is touched.
if (!dirtyBefore) {
  for (const path of generatedRegressionArtifacts) {
    const changed = spawnSync('git', ['diff', '--quiet', '--', path], { cwd: root }).status !== 0
    if (changed) {
      const baseline = spawnSync('git', ['show', `${beforeCommit}:${path}`], { cwd: root, encoding: 'buffer' })
      if (baseline.status === 0) writeFileSync(resolve(root, path), baseline.stdout)
    }
  }
}

const marker = (text, pattern) => pattern.test(text)
const gates = {
  b07_bookplan_maturity: { status: marker(b07.output, /B07_BOOKPLAN_RESULT .*signing_pass=PASS.*approved=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_volume_route: { status: marker(b07.output, /B07_ROUTE_RESULT .*volume_scope=PASS.*task_reality=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_chapter_plan_gates: { status: marker(b07.output, /B07_CHAPTER_RESULT .*current_only=PASS.*context_bound=PASS.*reservation_gate=PASS.*task_dependency=PASS.*continuity_authority=PASS.*model_output_bound=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_plan_stale: { status: marker(b07.output, /B07_CHAPTER_RESULT .*stale_precommit=PASS/), source: 'b07_targeted.log' },
  b07_plan_reality_separation: { status: marker(b07.output, /B07_CHAPTER_RESULT .*canon_unchanged=PASS/), source: 'b07_targeted.log' },
  b07_frontend_route: { status: ui.status === 'PASS' && marker(ui.output, /3 passed/), source: 'ui_b07_h02.log' },
  h02_registry: { status: b07.status === 'PASS' && marker(b07.output, /H02_CHECKPOINT_RESULT .*status=PASS/), source: 'b07_targeted.log' },
  h02_plan_route: { status: marker(b07.output, /B07_ROUTE_RESULT .*h02_query=PASS/), source: 'b07_targeted.log' },
  h02_constraints: { status: marker(b07.output, /H02_SESSION_RESULT .*finding=SUPPORTED.*receipt=PASS/), source: 'b07_targeted.log' },
  h02_diagnostic_provenance: { status: marker(b07.output, /H02_DIAGNOSTIC_RESULT .*stale_owner=PASS.*reservation_provenance=PASS.*thread_provenance=PASS.*status=PASS/), source: 'b07_targeted.log' },
  semantic_injections: { status: semanticInjections.status === 'PASS' && /semantic injection suite is tied to production authority guards/.test(semanticInjections.output), source: 'semantic_injections.log' },
  h02_question_fixtures: { status: marker(b07.output, /H02_QUESTIONS_RESULT .*q1=PASS.*q2=PASS.*q3=PASS.*q4=PASS.*q5=PASS.*q6=PASS.*q7=PASS.*real_support=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_signing_authority: { status: marker(b07.output, /B07_SIGN_RESULT .*no_receipt=PASS.*wrong_verdict=PASS.*fake_artifact=PASS.*actual_pass=PASS.*opening_bound=PASS/), source: 'b07_targeted.log' },
  b07_volume_intent_contract: { status: marker(b07.output, /B07_INTENT_RESULT .*malformed_rejected=PASS.*valid_commit=PASS/), source: 'b07_targeted.log' },
  h02_truncation: { status: marker(b07.output, /B07_ROUTE_RESULT .*truncation=PASS/) && marker(b07.output, /H02_SESSION_RESULT .*status=PASS/), source: 'b07_targeted.log' },
  h02_read_only: { status: architecture.status === 'PASS' && marker(architecture.output, /h02_write_capability_count=0/), source: 'architecture.log' },
  b06_h01_regression: { status: b06.status === 'PASS' && marker(b06.output, /test result: ok/), source: 'b06_h01_regression.log' },
  architecture: { status: architecture.status === 'PASS', source: 'architecture.log' },
  module_boundary: { status: moduleBoundary.status === 'PASS', source: 'module_boundary.log' },
  source_binding: { status: false, source: 'gate-results.json' },
  package_smoke: { status: process.env.B07_H02_PACKAGE_SMOKE === 'PASS', source: 'package-smoke.json' },
  cargo_fmt: { status: fmt.status === 'PASS', source: 'cargo_fmt.log' },
  cargo_check: { status: check.status === 'PASS', source: 'cargo_check.log' },
  cargo_clippy: { status: clippy.status === 'PASS', source: 'cargo_clippy.log' },
  node_full: { status: nodeFull.status === 'PASS', source: 'node_full.log' },
  frontend_build: { status: build.status === 'PASS', source: 'frontend_build.log' },
  contracts: { status: contracts.status === 'PASS', source: 'contracts.log' },
  vitest_full: { status: vitest.status === 'PASS', source: 'vitest_full.log' },
  b05_regression: { status: b05.status === 'PASS', source: 'b05_regression.log' },
  b06_h01_acceptance: { status: b06Acceptance.status === 'PASS', source: 'b06_h01_acceptance.log' },
  b06_h01_architecture: { status: b06Architecture.status === 'PASS', source: 'b06_h01_architecture.log' },
  m0a: { status: m0a.status === 'PASS', source: 'm0a.log' },
  cargo_all_targets: { status: cargoAll.status === 'PASS', source: 'cargo_all_targets.log' },
  b07_h02_scale: { status: scale.status === 'PASS', source: 'b07_h02_scale.log' },
}

const afterCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const dirtyAfter = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0
gates.source_binding = { status: beforeCommit === afterCommit && !dirtyBefore && !dirtyAfter, source: 'git' }

const normalized = Object.fromEntries(Object.entries(gates).map(([name, gate]) => [name, { ...gate, status: gate.status ? 'PASS' : 'FAIL', blocking: true }]))
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ batch: 'B07_H02', sourceCommit: beforeCommit, sourceCommitAfter: afterCommit, workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, sourceBinding: gates.source_binding.status ? 'PASS' : 'FAIL', gates: normalized }, null, 2)}\n`)
console.log(`B07_H02 gates recorded: ${Object.values(normalized).filter(gate => gate.status === 'PASS').length}/${Object.keys(normalized).length} PASS; sourceBinding=${gates.source_binding.status ? 'PASS' : 'FAIL'}; evidence=${evidence}`)
if (Object.values(normalized).some(gate => gate.status !== 'PASS')) process.exitCode = 1
