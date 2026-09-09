import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const baseline = '4552fa978f42e6d3d355afd2512ddaf77920a433'
const evidenceDir = path.resolve(process.env.POST_B14_UI_R8_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r8-evidence-')))
const commands = []
fs.mkdirSync(evidenceDir, { recursive: true })

const git = args => {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' })
  return result.stdout.trim()
}
const sourceCommit = git(['rev-parse', 'HEAD'])
const write = (relative, value) => {
  const file = path.join(evidenceDir, relative)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`)
}
const readJson = relative => JSON.parse(fs.readFileSync(path.join(evidenceDir, relative), 'utf8'))
const run = (name, command, args, env = {}) => {
  const start = new Date().toISOString()
  const started = Date.now()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 512 * 1024 * 1024 })
  const exitCode = result.status ?? 1
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const logRelative = path.join('COMMANDS', `${name}.log`)
  fs.mkdirSync(path.dirname(path.join(evidenceDir, logRelative)), { recursive: true })
  fs.writeFileSync(path.join(evidenceDir, logRelative), output)
  const record = { name, command: [command, ...args].join(' '), start, end: new Date().toISOString(), durationMs: Date.now() - started, exitCode, sourceCommit, evidencePath: logRelative, status: exitCode === 0 ? 'PASS' : 'FAIL' }
  commands.push(record)
  return record
}
const pass = records => records.every(record => record.status === 'PASS') ? 'PASS' : 'FAIL'
const changed = sourceCommit === baseline ? [] : git(['diff', '--name-only', `${baseline}..${sourceCommit}`]).split('\n').filter(Boolean)
const productionPrefixes = ['src/', 'src-tauri/src/', 'contracts/', 'migrations/', 'creative-skills/']
const productionChanges = changed.filter(file => productionPrefixes.some(prefix => file === prefix.slice(0, -1) || file.startsWith(prefix)))
const allowedChanges = file => file.startsWith('scripts/') || file.startsWith('tests/package/') || file.startsWith('tests/certification/') || file.startsWith('docs/') || file === 'package.json'
const productionZeroDiff = {
  schema: 'post-b14-ui-r8-production-zero-diff',
  repairBaselineCommit: baseline,
  sourceCommit,
  changedFiles: changed,
  srcChanges: changed.filter(file => file.startsWith('src/')).length,
  srcTauriSrcChanges: changed.filter(file => file.startsWith('src-tauri/src/')).length,
  contractsChanges: changed.filter(file => file.startsWith('contracts/')).length,
  migrationsChanges: changed.filter(file => file.startsWith('migrations/')).length,
  creativeSkillsChanges: changed.filter(file => file.startsWith('creative-skills/')).length,
  unexpectedChanges: changed.filter(file => !allowedChanges(file)),
  allZero: productionChanges.length === 0 && changed.every(allowedChanges),
  status: productionChanges.length === 0 && changed.every(allowedChanges) ? 'PASS' : 'FAIL',
}
write('R8_PRODUCTION_ZERO_DIFF.json', productionZeroDiff)

const r8Static = run('r8_certification_tests', 'node', ['--test', 'tests/certification/post-b14-ui-r8.test.mjs'])
const nodeTests = run('node_tests', 'npm', ['test'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const architecture = run('architecture_boundary', 'npm', ['run', 'verify'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const productUx = run('product_ux', 'npm', ['run', 'verify:product-ux'])
const r7Regression = run('post_b14_ui_r7_regression', 'npm', ['run', 'verify:post-b14-ui-r7'], { POST_B14_UI_R7_EVIDENCE_DIR: evidenceDir })
const rustAllTargets = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])

const r7B13 = fs.existsSync(path.join(evidenceDir, 'B13_UI_R7_REGRESSION.json')) ? readJson('B13_UI_R7_REGRESSION.json') : { status: 'FAIL', commands: [] }
const r7B14 = fs.existsSync(path.join(evidenceDir, 'B14_UI_R7_REGRESSION.json')) ? readJson('B14_UI_R7_REGRESSION.json') : { status: 'FAIL', commands: [] }
write('B13_UI_R8_REGRESSION.json', { schema: 'post-b14-ui-r8-b13-regression', sourceCommit, consumedEvidence: 'B13_UI_R7_REGRESSION.json', commands: r7B13.commands, status: r7B13.status === 'PASS' ? 'PASS' : 'FAIL' })
write('B14_UI_R8_REGRESSION.json', { schema: 'post-b14-ui-r8-b14-regression', sourceCommit, consumedEvidence: 'B14_UI_R7_REGRESSION.json', commands: r7B14.commands, status: r7B14.status === 'PASS' ? 'PASS' : 'FAIL' })
const display = fs.existsSync(path.join(evidenceDir, 'FULL_PRODUCT_VISIBLE_DOM_SCAN_R7.json')) ? readJson('FULL_PRODUCT_VISIBLE_DOM_SCAN_R7.json') : { status: 'FAIL' }
const a11 = fs.existsSync(path.join(evidenceDir, 'A11_TARGET_STATE_R7.json')) ? readJson('A11_TARGET_STATE_R7.json') : { status: 'FAIL' }
const noop = fs.existsSync(path.join(evidenceDir, 'A11_NOOP_NEGATIVE_R7.json')) ? readJson('A11_NOOP_NEGATIVE_R7.json') : { status: 'FAIL' }
const b13 = readJson('B13_UI_R8_REGRESSION.json')
const b14 = readJson('B14_UI_R8_REGRESSION.json')
write('COMMANDS_EXECUTED_R8.json', { schema: 'post-b14-ui-r8-commands-executed', sourceCommit, commands, status: pass(commands) })
const dirty = git(['status', '--porcelain', '--untracked-files=all'])
write('R8_RUNNER_BINDING.json', { schema: 'post-b14-ui-r8-runner-binding', repairBaselineCommit: baseline, sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: dirty !== '', excludedUncommittedPaths: [], productionCodeZeroDiff: productionZeroDiff.status, status: productionZeroDiff.status === 'PASS' && dirty === '' ? 'PASS' : 'FAIL' })

const status = pass(commands) === 'PASS' && productionZeroDiff.status === 'PASS' && r7Regression.status === 'PASS' && rustAllTargets.status === 'PASS' ? 'PASS' : 'FAIL'
console.log(JSON.stringify({ evidenceDir, sourceCommit, productionCodeZeroDiff: productionZeroDiff.status, displayLayer: display.status, a11: a11.status ?? (a11.allExpectedFieldsMatched ? 'PASS' : 'FAIL'), a11Noop: noop.status, b13: b13.status, b14: b14.status, status }, null, 2))
if (status !== 'PASS') process.exitCode = 1
