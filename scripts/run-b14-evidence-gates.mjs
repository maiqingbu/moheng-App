import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14Acceptance } from './generate-b14-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_EVIDENCE_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-evidence-')))
fs.mkdirSync(evidenceDir, { recursive: true })
const sourceCommit = git(['rev-parse', 'HEAD'])
const dirtyBefore = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const commands = {}

function git(args) { return spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim() }
function run(name, command, args) {
  const started = Date.now()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, env: process.env })
  const output = `${result.stdout || ''}${result.stderr || ''}`
  fs.writeFileSync(path.join(evidenceDir, `${name}.log`), output)
  const record = { name, command: [command, ...args].join(' '), durationMs: Date.now() - started, exitCode: result.status ?? 1, status: result.status === 0 ? 'PASS' : 'FAIL', sourceCommit }
  commands[name] = record
  return record
}
function gate(status, source, actual = {}) { return { status, blocking: true, source, ...actual } }

const architecture = run('b14_architecture', process.execPath, ['scripts/verify-b14-architecture.mjs'])
const deterministic = run('b14_deterministic', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame'])
const atomic = run('b14_atomic', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration'])
const contracts = run('b14_contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('b14_typecheck', 'npm', ['run', 'typecheck'])
const build = run('b14_build', 'npm', ['run', 'build'])
const nodeFull = run('b01_b13_node_vitest', 'npm', ['test'])
const architectureRegression = run('b01_b13_architecture', 'npm', ['run', 'verify'])
const rustCheck = run('b01_b13_rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustTests = run('b01_b13_rust_tests', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustFmt = run('b01_b13_rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const rustClippy = run('b01_b13_rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const dirtyAfter = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const changedSinceFrozen = git(['diff', '--name-only', '1a85e8f57c2098973d89977b02d33665cc054066..HEAD']).split('\n').filter(Boolean)
const gates = {
  b14_deterministic_gate: gate(deterministic.status, 'b14_deterministic.log'),
  b14_atomic_completion: gate(atomic.status, 'b14_atomic.log'),
  b14_contracts: gate(contracts.status, 'b14_contracts.log'),
  b14_architecture: gate(architecture.status, 'b14_architecture.log'),
  b14_ui_surface: gate(typecheck.status === 'PASS' && build.status === 'PASS', 'b14_typecheck.log + b14_build.log'),
  b14_rust_quality: gate([rustCheck, rustTests, rustFmt, rustClippy].every(item => item.status === 'PASS'), 'B01_B13 rust quality logs'),
  b01_b13_regression: gate(nodeFull.status === 'PASS' && architectureRegression.status === 'PASS' && rustTests.status === 'PASS', 'B01_B13 regression logs'),
  b14_source_binding: gate(!dirtyBefore && !dirtyAfter && sourceCommit === git(['rev-parse', 'HEAD']), 'git status --porcelain'),
  b15_boundary: gate(!changedSinceFrozen.some(file => /B15|ExportRelease|RC1/.test(file)), 'git diff against B13 frozen baseline', { nextBatchStarted: false }),
}
const report = { schema: 'b14-gate-results', batch: 'B14', sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, excludedUncommittedPaths: [], commands, gates }
report.status = Object.values(gates).every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
report.nextBatchAllowed = false
fs.writeFileSync(path.join(evidenceDir, 'gate-results.json'), `${JSON.stringify(report, null, 2)}\n`)
const acceptance = generateB14Acceptance(evidenceDir)
console.log(`B14_EVIDENCE_RESULT sourceCommit=${sourceCommit} status=${acceptance.status} evidenceDir=${evidenceDir}`)
if (acceptance.status !== 'PASS') process.exitCode = 1
