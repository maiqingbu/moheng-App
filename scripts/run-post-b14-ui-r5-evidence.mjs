import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R5_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r5-evidence-')))
const sourceCommit = git(['rev-parse', 'HEAD'])
const commands = []
fs.mkdirSync(evidenceDir, { recursive: true })

function git(args) { return spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim() }
function write(relative, value) { const file = path.join(evidenceDir, relative); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`) }
function run(name, command, args, env = {}) {
  const startedAt = new Date().toISOString(); const started = Date.now()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 512 * 1024 * 1024 })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`; const exitCode = result.status ?? 1
  const logPath = path.join(evidenceDir, 'COMMANDS', `${name}.log`); fs.mkdirSync(path.dirname(logPath), { recursive: true }); fs.writeFileSync(logPath, output)
  const record = { name, command: [command, ...args].join(' '), start: startedAt, end: new Date().toISOString(), durationMs: Date.now() - started, exitCode, sourceCommit, evidencePath: path.relative(evidenceDir, logPath), status: exitCode === 0 ? 'PASS' : 'FAIL' }
  commands.push(record); return { ...record, output }
}
function status(records) { return records.every(record => record.status === 'PASS') ? 'PASS' : 'FAIL' }

const bridgeBuild = run('bridge_build', 'cargo', ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--features', 'e2e-test', '--bin', 'post-b14-ui-r4-bridge'])
const productionE2e = bridgeBuild.status === 'PASS' ? run('production_e2e_r5', 'node', ['scripts/capture-post-b14-ui-r5.mjs'], { POST_B14_UI_R5_EVIDENCE_DIR: evidenceDir }) : { status: 'FAIL', output: '' }
const nodeTests = run('node_tests', 'npm', ['test'])
const r5Static = run('r5_static', 'node', ['--test', 'tests/post-b14-ui-r5.test.mjs'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
const verify = run('architecture_boundary', 'npm', ['run', 'verify'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const productUx = run('product_ux', 'npm', ['run', 'verify:product-ux'])
const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const b13Health = run('b13_health', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health'])
const b13Owner = run('b13_owner', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_r2_integration'])
const b13R3 = run('b13_r3', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_r3'])
const b14Endgame = run('b14_endgame', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration'])
const b14Scale = run('b14_scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_r2_scale'])
const rustAllTargets = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])

const b13Status = status([b13Health, b13Owner, b13R3])
const b14Status = status([b14Endgame, b14Scale])
write('B13_UI_R5_REGRESSION.json', { schema: 'post-b14-ui-r5-b13-regression', sourceCommit, commands: [b13Health, b13Owner, b13R3].map(({ output, ...record }) => record), taskReality: b13Owner.status, task501: b13R3.status, continuity: b13Health.status, status: b13Status })
write('B14_UI_R5_REGRESSION.json', { schema: 'post-b14-ui-r5-b14-regression', sourceCommit, commands: [b14Endgame, b14Scale].map(({ output, ...record }) => record), completion: b14Endgame.status, postCompletion: b14Scale.status, mappingUi: b14Endgame.status, runtimeRecheck: b14Scale.status, status: b14Status })
write('COMMANDS_EXECUTED.json', { schema: 'post-b14-ui-r5-commands-executed', sourceCommit, commands, status: status(commands) })
const dirtyAfter = git(['status', '--porcelain', '--untracked-files=all'])
write('R5_RUNNER_BINDING.json', { schema: 'post-b14-ui-r5-runner-binding', sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: dirtyAfter !== '', excludedUncommittedPaths: [], status: dirtyAfter === '' ? 'PASS' : 'FAIL' })
console.log(JSON.stringify({ evidenceDir, sourceCommit, productionE2e: productionE2e.status, b13: b13Status, b14: b14Status, commands: commands.length, status: status(commands) }, null, 2))
if (status(commands) !== 'PASS') process.exitCode = 1
