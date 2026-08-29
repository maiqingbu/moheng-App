import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '..')
const evidence = resolve(process.env.B07_H02_EVIDENCE_DIR ?? '/tmp/moheng-b07-h02-evidence')
mkdirSync(evidence, { recursive: true })

const beforeCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const dirtyBefore = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0

function run(name, command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8' })
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
const ui = run('ui_b07_h02', 'npx', ['vitest', 'run', 'tests/store/b07-h02-frontend.test.ts'])
const architecture = run('architecture', 'node', ['scripts/verify-b07-h02-architecture.mjs'])
const acceptanceTests = run('acceptance_generator_tests', 'node', ['--test', 'tests/b07-h02-acceptance-generator.test.mjs'])

const marker = (text, pattern) => pattern.test(text)
const gates = {
  b07_bookplan_maturity: { status: marker(b07.output, /B07_BOOKPLAN_RESULT .*signing_pass=PASS.*approved=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_volume_route: { status: marker(b07.output, /B07_ROUTE_RESULT .*volume_scope=PASS.*task_reality=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_chapter_plan_gates: { status: marker(b07.output, /B07_CHAPTER_RESULT .*current_only=PASS.*context_bound=PASS.*status=PASS/), source: 'b07_targeted.log' },
  b07_plan_stale: { status: marker(b07.output, /B07_CHAPTER_RESULT .*stale_precommit=PASS/), source: 'b07_targeted.log' },
  b07_plan_reality_separation: { status: marker(b07.output, /B07_CHAPTER_RESULT .*canon_unchanged=PASS/), source: 'b07_targeted.log' },
  b07_frontend_route: { status: ui.status === 'PASS' && marker(ui.output, /3 passed/), source: 'ui_b07_h02.log' },
  h02_registry: { status: b07.status === 'PASS' && marker(b07.output, /H02_CHECKPOINT_RESULT .*status=PASS/), source: 'b07_targeted.log' },
  h02_plan_route: { status: marker(b07.output, /B07_ROUTE_RESULT .*h02_query=PASS/), source: 'b07_targeted.log' },
  h02_constraints: { status: marker(b07.output, /H02_SESSION_RESULT .*finding=SUPPORTED.*receipt=PASS/), source: 'b07_targeted.log' },
  h02_truncation: { status: marker(b07.output, /B07_ROUTE_RESULT .*truncation=PASS/) && marker(b07.output, /H02_SESSION_RESULT .*status=PASS/), source: 'b07_targeted.log' },
  h02_read_only: { status: architecture.status === 'PASS' && marker(architecture.output, /h02_write_capability_count=0/), source: 'architecture.log' },
  b06_h01_regression: { status: b06.status === 'PASS' && marker(b06.output, /test result: ok/), source: 'b06_h01_regression.log' },
  architecture: { status: architecture.status === 'PASS', source: 'architecture.log' },
  module_boundary: { status: run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs']).status === 'PASS', source: 'module_boundary.log' },
  source_binding: { status: false, source: 'gate-results.json' },
  package_smoke: { status: process.env.B07_H02_PACKAGE_SMOKE === 'PASS', source: 'package-smoke.json' },
  cargo_fmt: { status: fmt.status === 'PASS', source: 'cargo_fmt.log' },
  cargo_check: { status: check.status === 'PASS', source: 'cargo_check.log' },
  cargo_clippy: { status: clippy.status === 'PASS', source: 'cargo_clippy.log' },
}

const afterCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const dirtyAfter = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0
gates.source_binding = { status: beforeCommit === afterCommit && !dirtyBefore && !dirtyAfter, source: 'git' }

const normalized = Object.fromEntries(Object.entries(gates).map(([name, gate]) => [name, { ...gate, status: gate.status ? 'PASS' : 'FAIL', blocking: true }]))
writeFileSync(resolve(evidence, 'gate-results.json'), `${JSON.stringify({ batch: 'B07_H02', sourceCommit: beforeCommit, sourceCommitAfter: afterCommit, workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, sourceBinding: gates.source_binding.status ? 'PASS' : 'FAIL', gates: normalized }, null, 2)}\n`)
console.log(`B07_H02 gates recorded: ${Object.values(normalized).filter(gate => gate.status === 'PASS').length}/${Object.keys(normalized).length} PASS; sourceBinding=${gates.source_binding.status ? 'PASS' : 'FAIL'}; evidence=${evidence}`)
if (Object.values(normalized).some(gate => gate.status !== 'PASS')) process.exitCode = 1
