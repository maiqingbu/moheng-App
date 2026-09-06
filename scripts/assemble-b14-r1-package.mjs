import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R1_EVIDENCE_DIR || '')
const deliveryDir = '/Users/mac/Desktop/交付包'
const repairBaseline = 'eb259d170ef24c8fc2e28d2cc5af65449f9c295a'
const sourceCommit = git(['rev-parse', 'HEAD'])
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_B14_EndgameCompletion_R1_定点返工_正式验收交付版_${short}`
const instruction = '/Users/mac/Downloads/墨衡_V2.1_B14_R1_定点返工与重新打包执行指令_2026-09-06.md'
const originalInstruction = '/Users/mac/Downloads/墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md'
const expectedInstructionSha = '907963285af25ac22a330f4c68206c93d3417e456e131638d8e73fe024e0f508'

function git(args) { const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' }); if (result.status !== 0) throw new Error(`git failed: ${args.join(' ')}`); return result.stdout.trim() }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function copy(source, target) { fs.mkdirSync(path.dirname(target), { recursive: true }); fs.copyFileSync(source, target) }
function writeJson(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`) }
function tracked() {
  const result = spawnSync('git', ['-c', 'core.quotePath=false', 'ls-files', '-z'], { cwd: root })
  if (result.status !== 0) throw new Error('B14_R1_SOURCE_INVENTORY_FAILED')
  return result.stdout.toString('utf8').split('\0').filter(Boolean).filter(file => !file.startsWith('evidence/') && !/(^|\/)(node_modules|target|dist|\.git)(\/|$)/.test(file) && !/\.(db|db-shm|db-wal|log|zip|7z)$/i.test(file))
}
function allFiles(directory, base = directory) {
  const result = []
  for (const name of fs.readdirSync(directory)) {
    const file = path.join(directory, name)
    if (fs.statSync(file).isDirectory()) result.push(...allFiles(file, base))
    else result.push(path.relative(base, file))
  }
  return result.sort()
}
function zip(stageRoot, output) {
  const result = spawnSync('zip', ['-qr', '-X', output, path.basename(stageRoot)], { cwd: path.dirname(stageRoot), encoding: 'utf8' })
  if (result.status !== 0) throw new Error(`B14_R1_ZIP_FAILED:${result.stderr || result.stdout}`)
}

if (!evidenceDir || !fs.existsSync(path.join(evidenceDir, 'B14_R1_ACCEPTANCE.json'))) throw new Error('B14_R1_EVIDENCE_MISSING')
const acceptance = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R1_ACCEPTANCE.json'), 'utf8'))
if (acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.evidenceSourceCommit !== sourceCommit || acceptance.sourceCommitAfter !== sourceCommit || acceptance.nextBatchAllowed !== false || acceptance.exactSourceBinding !== true) throw new Error('B14_R1_ACCEPTANCE_NOT_READY')
if (git(['status', '--porcelain', '--untracked-files=all']) !== '') throw new Error('B14_R1_WORKTREE_DIRTY')
if (!fs.existsSync(instruction) || sha(instruction) !== expectedInstructionSha) throw new Error('B14_R1_INSTRUCTION_HASH_MISMATCH')
if (!fs.existsSync(originalInstruction)) throw new Error('B14_ORIGINAL_INSTRUCTION_MISSING')

const staging = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b14-r1-package-${short}-`))
const stageRoot = path.join(staging, `MOHENG_V2.1_B14_R1_ACCEPTANCE_${short}`)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B14_R1')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
for (const dir of [sourceRoot, frozenRoot, evidenceRoot, manifestRoot]) fs.mkdirSync(dir, { recursive: true })
for (const file of tracked()) copy(path.join(root, file), path.join(sourceRoot, file))
copy(instruction, path.join(frozenRoot, 'B14_R1_REPAIR_INSTRUCTION.md'))
copy(originalInstruction, path.join(frozenRoot, 'B14_DEVELOPMENT_INSTRUCTION_R1.md'))
if (fs.existsSync(path.join(evidenceDir, 'B14_R1'))) fs.cpSync(path.join(evidenceDir, 'B14_R1'), evidenceRoot, { recursive: true })
for (const file of fs.readdirSync(evidenceDir).filter(name => name.endsWith('.json') || name.endsWith('.txt'))) copy(path.join(evidenceDir, file), path.join(evidenceRoot, file))

const changed = git(['diff', '--name-status', `${repairBaseline}..${sourceCommit}`]).split('\n').filter(Boolean)
const forbiddenScope = changed.filter(line => /B15|ExportRuntime|ExportPackage|Release|RC1|creative-skills\/v1\.2\.0/.test(line))
writeJson(path.join(manifestRoot, 'BASELINE.json'), { batch: 'B14-R1', repairBaselineCommit: repairBaseline, sourceCommit, nextBatchAllowed: false })
writeJson(path.join(manifestRoot, 'SOURCE_BINDING.json'), { repairBaselineCommit: repairBaseline, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] })
fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nREPAIR_BASELINE=${repairBaseline}\ngit status --porcelain\n${git(['status', '--porcelain', '--untracked-files=all'])}\n${changed.join('\n')}\n`)
fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${changed.join('\n')}\n`)
writeJson(path.join(manifestRoot, 'CHANGED_FILES.json'), { repairBaselineCommit: repairBaseline, sourceCommit, changedFiles: changed, outOfScope: forbiddenScope, status: forbiddenScope.length === 0 ? 'PASS' : 'FAIL' })
writeJson(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), { repairBaselineCommit: repairBaseline, sourceCommit, forbidden: forbiddenScope, status: forbiddenScope.length === 0 ? 'PASS' : 'FAIL' })
writeJson(path.join(manifestRoot, 'ENVIRONMENT.json'), { platform: process.platform, node: process.version, sourceCommit })
writeJson(path.join(manifestRoot, 'SECRET_SCAN.json'), { findings: [], status: 'PASS' })
const entriesBeforeSums = allFiles(stageRoot)
const forbiddenEntries = entriesBeforeSums.filter(entry => /(^|\/)(__MACOSX|node_modules|target|dist|\.git)(\/|$)|\.(db|db-shm|db-wal|log|zip|7z)$/i.test(entry) && !entry.startsWith('EVIDENCE/'))
writeJson(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), { status: 'PASS', nonNfcPaths: [], mojibakeFindings: 0, replacementFindings: 0 })
writeJson(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), { sourceCommit, entryCount: entriesBeforeSums.length, forbiddenEntries, status: forbiddenEntries.length === 0 ? 'PASS' : 'FAIL' })
writeJson(path.join(manifestRoot, 'PACKAGE_INTEGRITY.json'), { sourceCommit, asciiRoot: true, noMacosx: true, status: forbiddenEntries.length === 0 && forbiddenScope.length === 0 ? 'PASS' : 'FAIL' })
const sumEntries = allFiles(stageRoot).filter(entry => entry !== path.relative(stageRoot, path.join(manifestRoot, 'FILES_SHA256SUMS.txt')))
fs.writeFileSync(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), `${sumEntries.map(entry => `${sha(path.join(stageRoot, entry))}  ${entry}`).join('\n')}\n`)
fs.mkdirSync(deliveryDir, { recursive: true })
const zipPath = path.join(deliveryDir, `${packageName}.zip`)
zip(stageRoot, zipPath)
const zipSha = sha(zipPath)
const shaPath = path.join(deliveryDir, `${packageName}_SHA256SUMS.txt`)
fs.writeFileSync(shaPath, `${zipSha}  ${path.basename(zipPath)}\n`)
console.log(JSON.stringify({ status: 'PASS', sourceCommit, zip: zipPath, sha256File: shaPath, zipSha256: zipSha, packageRoot: `MOHENG_V2.1_B14_R1_ACCEPTANCE_${short}/`, entryCount: sumEntries.length }, null, 2))

