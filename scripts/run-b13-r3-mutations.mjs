import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R3_EVIDENCE_DIR || '')
const mutationRoot = path.join(evidenceDir, 'B13_R3', 'MUTATIONS')
const sourceCommit = process.env.B13_R3_SOURCE_COMMIT || git(['rev-parse', 'HEAD'])
const repairBaselineCommit = process.env.B13_R3_REPAIR_BASELINE_COMMIT || git(['rev-parse', 'e6419ce^{commit}'])

function git(args) {
  return spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }).stdout.trim()
}

function exec(command, args, extraEnv = {}) {
  return spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || '/Users/mac/Library/Caches/moheng-product-ux-cargo-target',
      ...extraEnv,
    },
    maxBuffer: 256 * 1024 * 1024,
  })
}

function combined(result) {
  return (result.stdout || '') + (result.stderr || '')
}

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex')
}

function runTarget(args) {
  const result = exec('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', ...args, '--', '--nocapture'])
  return { exitCode: result.status === null ? 1 : result.status, output: combined(result) }
}

function runProductUx() {
  const result = exec('node', ['scripts/verify-product-ux.mjs'])
  return { exitCode: result.status === null ? 1 : result.status, output: combined(result) }
}

function runTruthfulness() {
  const result = exec('node', ['scripts/verify-b13-r3-evidence-truthfulness.mjs'], {
    B13_R3_EVIDENCE_DIR: evidenceDir,
    B13_R3_SOURCE_COMMIT: sourceCommit,
    B13_R3_REPAIR_BASELINE_COMMIT: repairBaselineCommit,
    B13_R3_REQUIRE_MUTATIONS: 'false',
  })
  return { exitCode: result.status === null ? 1 : result.status, output: combined(result) }
}

function writeMutationResult(directory, id, target, description, introduced, reverted, restored, original, current) {
  const result = {
    schema: 'b13-r3-mutation-result',
    id,
    sourceCommit,
    repairBaselineCommit,
    target,
    description,
    patchApplied: true,
    introducedExitCode: introduced.exitCode,
    introducedStatus: introduced.exitCode === 0 ? 'PASS' : 'FAIL',
    revertedExitCode: reverted.exitCode,
    revertedStatus: reverted.exitCode === 0 ? 'PASS' : 'FAIL',
    reverted: restored,
    originalSha256: sha256(original),
    restoredSha256: sha256(current),
    status: introduced.exitCode !== 0 && reverted.exitCode === 0 && restored ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify(result, null, 2) + '\n')
  return result
}

function mutateText(id, target, original, mutated, command, description, baseDirectory = root) {
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const targetPath = path.join(baseDirectory, target)
  const originalPath = path.join(os.tmpdir(), 'b13-r3-' + id + '-original-' + process.pid)
  const mutatedPath = path.join(os.tmpdir(), 'b13-r3-' + id + '-mutated-' + process.pid)
  fs.writeFileSync(originalPath, original)
  fs.writeFileSync(mutatedPath, mutated)
  const diff = exec('diff', ['-u', originalPath, mutatedPath])
  fs.writeFileSync(path.join(directory, 'patch.diff'), diff.stdout || id + ': mutation changed ' + target + '\n')
  let introduced
  let restored = false
  try {
    fs.writeFileSync(targetPath, mutated)
    introduced = command()
    fs.writeFileSync(path.join(directory, 'introduced.log'), introduced.output)
  } finally {
    fs.writeFileSync(targetPath, original)
    restored = true
  }
  const reverted = command()
  fs.writeFileSync(path.join(directory, 'reverted.log'), reverted.output)
  return writeMutationResult(directory, id, target, description, introduced, reverted, restored, original, fs.readFileSync(targetPath))
}

function mutateDelete(id, target, description) {
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const targetPath = path.join(evidenceDir, target)
  const original = fs.readFileSync(targetPath)
  fs.writeFileSync(path.join(directory, 'patch.diff'), 'diff --git a/' + target + ' b/' + target + '\n deleted file: ' + target + '\n')
  let introduced
  let restored = false
  try {
    fs.unlinkSync(targetPath)
    introduced = runTruthfulness()
    fs.writeFileSync(path.join(directory, 'introduced.log'), introduced.output)
  } finally {
    fs.writeFileSync(targetPath, original)
    restored = true
  }
  const reverted = runTruthfulness()
  fs.writeFileSync(path.join(directory, 'reverted.log'), reverted.output)
  return writeMutationResult(directory, id, target, description, introduced, reverted, restored, original, fs.readFileSync(targetPath))
}

function main() {
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R3_MUTATION_EVIDENCE_DIR_REQUIRED')
  if (git(['status', '--porcelain', '--untracked-files=all'])) throw new Error('B13_R3_MUTATION_WORKTREE_MUST_START_CLEAN')
  fs.mkdirSync(mutationRoot, { recursive: true })
  const results = []

  const volumePath = 'src-tauri/src/volume_lifecycle.rs'
  const volumeOriginal = fs.readFileSync(path.join(root, volumePath))
  const volumeText = volumeOriginal.toString('utf8')
  const completeCall = 'NarrativeRepository::task_states_by_ids_complete(connection, project_id, &task_ids)'
  if (!volumeText.includes(completeCall)) throw new Error('B13_R3_MUTATION_COMPLETE_READER_MARKER_MISSING')
  const volumeMutated = Buffer.from(volumeText.replace(completeCall, 'NarrativeRepository::task_states_by_ids(connection, project_id, &task_ids, 500)'), 'utf8')
  results.push(mutateText(
    'MUT-R3-01',
    volumePath,
    volumeOriginal,
    volumeMutated,
    () => runTarget(['--test', 'b13_r3']),
    'replace B09 planning-state complete reader with the shared bounded reader; 501 formal integration must fail',
  ))

  const uxPath = 'src/ui/pages/VolumeManager.vue'
  const uxOriginal = fs.readFileSync(path.join(root, uxPath))
  const uxText = uxOriginal.toString('utf8')
  const cleanCopy = '全书规划只保存整体战略；详细卷计划按当前卷逐步建立，不会一次生成整本书的章节规划。'
  const leakedCopy = 'BookPlan 只保存全书战略；详细 VolumePlan 按当前卷创建。'
  if (!uxText.includes(cleanCopy)) throw new Error('B13_R3_MUTATION_PRODUCT_COPY_MARKER_MISSING')
  const uxMutated = Buffer.from(uxText.replace(cleanCopy, leakedCopy), 'utf8')
  results.push(mutateText(
    'MUT-R3-02',
    uxPath,
    uxOriginal,
    uxMutated,
    runProductUx,
    'restore internal BookPlan/VolumePlan names in the PageHeader subtitle; Product UX must fail closed',
  ))

  const plansPath = 'B13_R3_CURRENT_QUERY_PLANS.json'
  const plansFile = path.join(evidenceDir, plansPath)
  const plansOriginal = fs.readFileSync(plansFile)
  const plans = JSON.parse(plansOriginal.toString('utf8'))
  const oldSql = 'SELECT COUNT(*) FROM planning_state_ledger_index WHERE project_id=?1'
  const formalRoute = plans.plans.find(plan => plan.queryName === 'formal_route_source')
  if (!formalRoute) throw new Error('B13_R3_MUTATION_QUERY_PLAN_MARKER_MISSING')
  formalRoute.sql = oldSql
  formalRoute.sqlDigest = sha256(Buffer.from(JSON.stringify({ sql: oldSql }), 'utf8'))
  const plansMutated = Buffer.from(JSON.stringify(plans, null, 2) + '\n', 'utf8')
  results.push(mutateText(
    'MUT-R3-03A',
    plansPath,
    plansOriginal,
    plansMutated,
    runTruthfulness,
    'point current performance evidence at the frozen compatibility SQL; Query Plan truthfulness must fail',
    evidenceDir,
  ))

  results.push(mutateDelete(
    'MUT-R3-03B',
    'B13_R3/PACKAGE_READERS/reader-b-list.log',
    'delete the independent Reader B raw list log; dual-reader evidence must fail closed',
  ))

  const result = {
    schema: 'b13-r3-mutation-gates',
    sourceCommit,
    repairBaselineCommit,
    mutations: results,
    status: results.length === 4 && results.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R3_MUTATION_GATES.json'), JSON.stringify(result, null, 2) + '\n')
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}

try { main() } catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
