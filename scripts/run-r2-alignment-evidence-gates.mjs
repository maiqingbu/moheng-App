import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root, 'src-tauri')
const evidence = process.env.R2_ALIGNMENT_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r2-alignment.'))
const generatedB01Evidence = ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']
fs.mkdirSync(evidence, { recursive: true })

function git(args, encoding = 'utf8') { return spawnSync('git', args, { cwd: root, encoding }) }
function run(name, command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', env: process.env })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  fs.writeFileSync(path.join(evidence, `${name}.log`), output)
  return { status: result.status === 0 ? 'PASS' : 'FAIL', output }
}
function sha256(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function gate(status, source) { return { status: status ? 'PASS' : 'FAIL', source, blocking: true } }

const startCommit = 'ec1a12176100ba427395c2d62bdc1661dcabfa5e'
const beforeCommit = git(['rev-parse', 'HEAD']).stdout.trim()
const dirtyBefore = git(['status', '--porcelain']).stdout.trim().length > 0
const targeted = run('alignment_targeted', 'cargo', ['test', '--test', 'b08_h03', '--', '--nocapture'], rustRoot)
const b05 = run('b05_regression', 'cargo', ['test', '--test', 'b05_narrative'], rustRoot)
const b06 = run('b06_h01_regression', 'cargo', ['test', '--test', 'b06_h01'], rustRoot)
const b07 = run('b07_h02_regression', 'cargo', ['test', '--test', 'b07_h02', '--test', 'b07_h02_semantic_negative'], rustRoot)
const architecture = run('architecture', 'node', ['scripts/verify-b08-h03-architecture.mjs'])
const boundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const m0 = run('m0a', 'node', ['scripts/verify-m0a-evidence.mjs'])
const node = run('node', 'npm', ['test'])
const vitest = run('vitest', 'npm', ['run', 'test:store'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const cargoFmt = run('cargo_fmt', 'cargo', ['fmt', '--all', '--', '--check'], rustRoot)
const cargoCheck = run('cargo_check', 'cargo', ['check', '--all-targets'], rustRoot)
const cargoClippy = run('cargo_clippy', 'cargo', ['clippy', '--all-targets', '--all-features', '--', '-D', 'warnings'], rustRoot)
const cargoAll = run('cargo_all_targets', 'cargo', ['test', '--all-targets'], rustRoot)

if (!dirtyBefore) {
  for (const file of generatedB01Evidence) {
    if (git(['diff', '--quiet', '--', file]).status !== 0) git(['restore', '--source=HEAD', '--', file])
  }
}

const migrationFiles = git(['ls-tree', '-r', '--name-only', 'HEAD', 'src-tauri/migrations']).stdout.trim().split('\n').filter(file => /000[1-9].*\.sql$/.test(file))
const migrationHashes = Object.fromEntries(migrationFiles.map(file => [file, sha256(path.join(root, file))]))
const migrationFreeze = migrationFiles.length === 9 && !git(['diff', '--quiet', `${startCommit}..HEAD`, '--', 'src-tauri/migrations']).status
const packagePath = path.join(os.tmpdir(), `moheng-r2-alignment-${beforeCommit.slice(0, 7)}.zip`)
const archive = spawnSync('git', ['archive', '--format=zip', '-o', packagePath, 'HEAD'], { cwd: root, encoding: 'utf8' })
const zipTest = archive.status === 0 ? spawnSync('unzip', ['-t', packagePath], { encoding: 'utf8' }) : { status: 1, stdout: '' }
const zipList = archive.status === 0 ? spawnSync('unzip', ['-Z1', packagePath], { encoding: 'utf8' }) : { status: 1, stdout: '' }
const forbidden = /(^|\/)(target|node_modules|\.git)(\/|$)|\.db($|[-.])/m.test(zipList.stdout ?? '')
const requiredPaths = ['.github/workflows/ci.yml', '.gitignore'].every(file => (zipList.stdout ?? '').split('\n').includes(file))
const packageSmoke = archive.status === 0 && zipTest.status === 0 && !forbidden && requiredPaths
fs.writeFileSync(path.join(evidence, 'package_smoke.json'), `${JSON.stringify({ packagePath, packageSha256: archive.status === 0 ? sha256(packagePath) : null, forbiddenEntries: forbidden ? 1 : 0, requiredPaths, status: packageSmoke ? 'PASS' : 'FAIL' }, null, 2)}\n`)
const has = marker => targeted.status === 'PASS' && targeted.output.includes(marker)
const gates = {
  'ALIGN-A01': gate(architecture.status === 'PASS' && node.output.includes('R2 alignment keeps persistence SQL inside approved Rust owners'), 'architecture.log+node.log'),
  'ALIGN-A02': gate(architecture.status === 'PASS' && has('h03_recompile_uses_b07_owner_and_b06_stale_requery'), 'architecture.log+alignment_targeted.log'),
  'ALIGN-A03': gate(has('h03_valid_authorization_cannot_bypass_planning_task_scope_gate'), 'alignment_targeted.log'),
  'ALIGN-A04': gate(has('plan_foreshadow_actual_reveal_is_blocked'), 'alignment_targeted.log'),
  'ALIGN-A05': gate(has('canon_semantic_conflict_with_new_fact_id_is_blocked') && has('canon_semantic_difference_with_disjoint_world_time_is_not_blocked'), 'alignment_targeted.log'),
  'ALIGN-A06': gate(has('canon_same_semantic_fact_with_new_fact_id_is_not_blocked'), 'alignment_targeted.log'),
  'ALIGN-A07': gate(has('b08_final_commit_fault_rolls_back_final_chapter_reality_and_receipt'), 'alignment_targeted.log'),
  'ALIGN-A08': gate(b07.status === 'PASS' && targeted.status === 'PASS', 'b07_h02_regression.log+alignment_targeted.log'),
  migration_freeze: gate(migrationFreeze, 'migration-hashes.json'),
  architecture: gate(architecture.status === 'PASS', 'architecture.log'),
  module_boundary: gate(boundary.status === 'PASS', 'module_boundary.log'),
  b05_regression: gate(b05.status === 'PASS', 'b05_regression.log'),
  b06_h01_regression: gate(b06.status === 'PASS', 'b06_h01_regression.log'),
  b07_h02_regression: gate(b07.status === 'PASS', 'b07_h02_regression.log'),
  b08_h03_regression: gate(targeted.status === 'PASS', 'alignment_targeted.log'),
  node: gate(node.status === 'PASS', 'node.log'), vitest: gate(vitest.status === 'PASS', 'vitest.log'),
  contracts: gate(contracts.status === 'PASS', 'contracts.log'), typecheck: gate(typecheck.status === 'PASS', 'typecheck.log'), build: gate(build.status === 'PASS', 'build.log'),
  m0a: gate(m0.status === 'PASS', 'm0a.log'), cargo_all_targets: gate(cargoAll.status === 'PASS', 'cargo_all_targets.log'),
  cargo_fmt: gate(cargoFmt.status === 'PASS', 'cargo_fmt.log'), cargo_check: gate(cargoCheck.status === 'PASS', 'cargo_check.log'), cargo_clippy: gate(cargoClippy.status === 'PASS', 'cargo_clippy.log'),
  package_smoke: gate(packageSmoke, 'package_smoke.json'), source_binding: gate(false, 'git'),
}
const afterCommit = git(['rev-parse', 'HEAD']).stdout.trim()
const dirtyAfter = git(['status', '--porcelain']).stdout.trim().length > 0
const sourceBinding = beforeCommit === afterCommit && !dirtyBefore && !dirtyAfter && beforeCommit !== startCommit
gates.source_binding = gate(sourceBinding, 'git')
const report = { schema: 'post-b08-h03-r2-alignment-gates-r1', startCommit, sourceCommit: beforeCommit, evidenceSourceCommit: beforeCommit, sourceCommitAfter: afterCommit, workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, excludedUncommittedPaths: [], sourceBinding: sourceBinding ? 'PASS' : 'FAIL', migrationHashes, gates }
fs.writeFileSync(path.join(evidence, 'migration-hashes.json'), `${JSON.stringify({ migrationHashes, status: migrationFreeze ? 'PASS' : 'FAIL' }, null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'alignment-gates.json'), `${JSON.stringify(report, null, 2)}\n`)
console.log(`R2 alignment gates: ${Object.values(gates).filter(item => item.status === 'PASS').length}/${Object.keys(gates).length} PASS; sourceBinding=${report.sourceBinding}; evidence=${evidence}`)
if (Object.values(gates).some(item => item.status !== 'PASS')) process.exitCode = 1
