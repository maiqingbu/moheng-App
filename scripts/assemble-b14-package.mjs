import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_EVIDENCE_DIR || '')
const deliveryDir = '/Users/mac/Desktop/交付包'
const sourceCommit = git(['rev-parse', 'HEAD'])
const shortCommit = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_B14_EndgameCompletion_正式验收交付版_${shortCommit}`
const instructionCandidates = [
  '/Users/mac/Downloads/墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md',
  '/Users/mac/Desktop/墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md',
]
const freezeCandidates = [
  '/Users/mac/Downloads/墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B13_2026-09-05.md',
  '/Users/mac/Desktop/墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B13_2026-09-05.md',
]
const expectedHashes = {
  instruction: 'e67cba88e4fbe9c898e22f6215fa7a8dea6f3206bf73ba417ff3a5abd6ede6e0',
  freeze: 'd198723eae4441c9d538155b18cddd938611285ce23ae873ecde2790c9d18933',
}

function git(args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' })
  if (result.status !== 0) throw new Error(`git failed: ${args.join(' ')}`)
  return result.stdout.trim()
}
function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}
function resolveInput(candidates, expected, label) {
  const file = candidates.find(candidate => fs.existsSync(candidate))
  if (!file) throw new Error(`B14_${label}_INSTRUCTION_MISSING`)
  const actual = sha256File(file)
  if (actual !== expected) throw new Error(`B14_${label}_INSTRUCTION_HASH_MISMATCH:${actual}`)
  return { file, sha256: actual }
}
function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`)
}
function copyFile(source, target) {
  fs.mkdirSync(path.dirname(target), { recursive: true })
  fs.copyFileSync(source, target)
}
function trackedSourceFiles() {
  const result = spawnSync('git', ['-c', 'core.quotePath=false', 'ls-files', '-z'], { cwd: root })
  if (result.status !== 0) throw new Error('B14_SOURCE_INVENTORY_FAILED')
  return result.stdout.toString('utf8').split('\0').filter(Boolean).filter(file => {
    if (file.startsWith('evidence/')) return false
    if (/(^|\/)(node_modules|target|dist|\.git)(\/|$)/.test(file)) return false
    if (/\.(db|db-shm|db-wal|log|zip|7z)$/i.test(file)) return false
    return true
  })
}
function packageTree(stageRoot) {
  const entries = []
  const walk = directory => {
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      const relative = path.relative(stageRoot, file)
      if (fs.statSync(file).isDirectory()) walk(file)
      else entries.push({ path: relative, sha256: sha256File(file), bytes: fs.statSync(file).size })
    }
  }
  walk(stageRoot)
  return entries.sort((a, b) => a.path.localeCompare(b.path))
}
function zipDirectory(stageRoot, zipPath) {
  const result = spawnSync('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', stageRoot, zipPath], { encoding: 'utf8' })
  if (result.status !== 0) throw new Error(`B14_ZIP_FAILED:${result.stderr || result.stdout}`)
}

if (!evidenceDir || !fs.existsSync(path.join(evidenceDir, 'B14_ACCEPTANCE.json'))) throw new Error('B14_PACKAGE_EVIDENCE_MISSING')
const acceptance = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_ACCEPTANCE.json'), 'utf8'))
if (acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.evidenceSourceCommit !== sourceCommit || acceptance.sourceCommitAfter !== sourceCommit || acceptance.workingTreeDirtyBefore !== false || acceptance.workingTreeDirtyAfter !== false || acceptance.nextBatchAllowed !== false) throw new Error('B14_PACKAGE_ACCEPTANCE_NOT_READY')
if (git(['status', '--porcelain', '--untracked-files=all']) !== '') throw new Error('B14_PACKAGE_WORKTREE_DIRTY')

const instruction = resolveInput(instructionCandidates, expectedHashes.instruction, 'INSTRUCTION')
const freeze = resolveInput(freezeCandidates, expectedHashes.freeze, 'FREEZE')
const staging = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b14-package-${shortCommit}-`))
const stageRoot = path.join(staging, packageName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B14')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
for (const directory of [sourceRoot, evidenceRoot, frozenRoot, manifestRoot]) fs.mkdirSync(directory, { recursive: true })

for (const file of trackedSourceFiles()) copyFile(path.join(root, file), path.join(sourceRoot, file))
fs.cpSync(evidenceDir, evidenceRoot, { recursive: true })
copyFile(instruction.file, path.join(frozenRoot, 'B14_DEVELOPMENT_INSTRUCTION_R1.md'))
copyFile(freeze.file, path.join(frozenRoot, 'R2_FREEZE_THROUGH_B13.md'))
writeJson(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), { batch: 'B14', sourceCommit, instructionSha256: instruction.sha256, freezeSha256: freeze.sha256, nextBatch: 'B15', nextBatchStarted: false })
writeJson(path.join(manifestRoot, 'BASELINE.json'), { batch: 'B14', sourceCommit, sourceCommitShort: shortCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, nextBatchAllowed: false })
fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\ngit status --porcelain\n${git(['status', '--porcelain', '--untracked-files=all'])}\n`)
fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 B14 终局与完结治理正式验收包\n\n源码提交：\`${sourceCommit}\`\n\nB14 blocking acceptance：PASS\n\n本包包含确定性 Endgame Gate、EndgameContract、原子 BookExitState + BookCompletionReport 提交、DomainReceipt、正式 Tauri/UI 接线与 source-bound Evidence。B15 未启动。\n`)

const inventory = packageTree(stageRoot)
fs.writeFileSync(path.join(manifestRoot, 'FILE_SHA256SUMS.txt'), `${inventory.map(item => `${item.sha256}  ${item.path}`).join('\n')}\n`)
const forbidden = item => !item.path.startsWith('EVIDENCE/') && /(^|\/)(node_modules|target|dist|\.git)(\/|$)|\.(db|db-shm|db-wal|log|zip|7z)$/i.test(item.path)
writeJson(path.join(manifestRoot, 'PACKAGE_CONSISTENCY.json'), { schema: 'b14-package-consistency', sourceCommit, packageRoot: `${packageName}/`, entryCount: inventory.length, forbiddenMatches: inventory.filter(forbidden), status: inventory.every(item => !forbidden(item)) ? 'PASS' : 'FAIL' })
if (JSON.parse(fs.readFileSync(path.join(manifestRoot, 'PACKAGE_CONSISTENCY.json'), 'utf8')).status !== 'PASS') throw new Error('B14_PACKAGE_CONSISTENCY_FAILED')

fs.mkdirSync(deliveryDir, { recursive: true })
const zipPath = path.join(deliveryDir, `${packageName}.zip`)
zipDirectory(stageRoot, zipPath)
const zipSha256 = sha256File(zipPath)
const shaPath = path.join(deliveryDir, `${packageName}.SHA256SUMS.txt`)
fs.writeFileSync(shaPath, `${zipSha256}  ${path.basename(zipPath)}\n`)
console.log(JSON.stringify({ status: 'PASS', sourceCommit, zip: zipPath, sha256File: shaPath, zipSha256, packageRoot: `${packageName}/`, entryCount: inventory.length }, null, 2))
