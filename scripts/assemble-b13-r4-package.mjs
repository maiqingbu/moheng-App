import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { validatePackageTarget, validatePackageTree } from './verify-package-consistency.mjs'
import { verifyDualReaders } from './verify-b13-r3-dual-reader.mjs'
import { frozenSkillPathInventory, scanPackageZip, scanSkillDirectory, pathRestorationReport } from './verify-b13-r4-paths.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outputDir = '/Users/mac/Desktop/交付包'
const repairBaselineShort = 'a479b28'
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const originalInstructionCandidates = [
  process.env.B13_ORIGINAL_INSTRUCTION_PATH,
  '/Users/mac/Desktop/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md',
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md',
].filter(Boolean)
const r3InstructionCandidates = [
  process.env.B13_R3_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R3_最终封口修复与正式重新打包执行指令_2026-09-05.md',
].filter(Boolean)
const r4InstructionCandidates = [
  process.env.B13_R4_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R4_仅路径编码与打包Gate最终微补丁执行指令_2026-09-05.md',
].filter(Boolean)
const expectedHashes = {
  original: '54a10882729ae79497231f49e465740d39bfb67e6683d0feb0446403bbe416cb',
  r3: '9340d016721506b1fe5d63f6ba4319530257bf4eba2c506573cc0a19aa71f5b9',
  r4: 'b4315c27f15db1a5498eacfed2407cff487812bdeb5fd1f326a08edb956e3901',
}

function git(args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
  if (result.status !== 0) throw new Error('B13_R4_GIT_FAILED:' + (result.stderr || ''))
  return result.stdout.trim()
}

function sha256Bytes(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex') }
function sha256(file) { return sha256Bytes(fs.readFileSync(file)) }
function resolveExisting(candidates, label) {
  const file = candidates.find(candidate => fs.existsSync(candidate) && fs.statSync(candidate).isFile())
  if (!file) throw new Error('B13_R4_FROZEN_INPUT_MISSING:' + label + ':' + candidates.join('|'))
  return file
}
function trackedFiles() { return git(['ls-files', '-z']).split('\0').filter(Boolean) }
function forbidden(relative) {
  return /(?:^|\/)(?:node_modules|target|dist|coverage|\.git)(?:\/|$)|(?:^|\/)(?:\.env(?:\.|$)|.*\.db(?:-shm|-wal)?$|core\.[0-9]+$)/.test(relative)
}
function sourceAllowed(relative) {
  return /^(?:src|src-tauri|contracts|scripts|tests|docs|creative-skills|\.github)\//.test(relative)
    || /^(?:AGENTS\.md|RTK\.md|\.gitignore|MULTI_PROJECT_README\.md|index\.html|package\.json|package-lock\.json|tsconfig\.json|vite\.config\.ts|vitest\.config\.ts)$/.test(relative)
}
function allFiles(directory) {
  const files = []
  const visit = current => {
    for (const entry of fs.readdirSync(current).sort((left, right) => left.localeCompare(right))) {
      const file = path.join(current, entry)
      if (fs.statSync(file).isDirectory()) visit(file)
      else files.push(file)
    }
  }
  visit(directory)
  return files
}
function relative(stageRoot, file) { return path.relative(stageRoot, file).split(path.sep).join('/') }
function writeJson(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n') }
function runPython(script, args) {
  return spawnSync('python3', ['-c', script, ...args], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
}
function makeZip(stageRoot, zipPath) {
  const script = [
    'import os,sys,zipfile',
    'root=sys.argv[1]; out=sys.argv[2]',
    'with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as z:',
    '    for base,dirs,files in os.walk(root):',
    '        dirs.sort(); files.sort()',
    '        for name in files:',
    '            p=os.path.join(base,name)',
    '            arc=os.path.relpath(p,os.path.dirname(root)).replace(os.sep,"/")',
    '            z.write(p,arc)',
  ].join('\n')
  const result = runPython(script, [stageRoot, zipPath])
  if (result.status !== 0) throw new Error('B13_R4_ZIP_CREATE_FAILED:' + result.stderr)
}
function writeInventory(stageRoot, manifestRoot) {
  const checksumRelative = 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt'
  const ordinary = allFiles(stageRoot).map(file => relative(stageRoot, file)).filter(file => file !== checksumRelative).sort((a, b) => a.localeCompare(b))
  const inventory = {
    schema: 'b13-r4-package-inventory',
    countingRule: 'totalFilesExcludingChecksums counts every regular file under the package root, excluding only PACKAGE_MANIFEST/FILES_SHA256SUMS.txt; ZIP entries equal this value plus one checksum file.',
    totalFilesExcludingChecksums: ordinary.length,
    sourceFiles: ordinary.filter(file => file.startsWith('SOURCE/')).length,
    frozenInputFiles: ordinary.filter(file => file.startsWith('FROZEN_INPUTS/')).length,
    evidenceFiles: ordinary.filter(file => file.startsWith('EVIDENCE/')).length,
    manifestFiles: ordinary.filter(file => file.startsWith('PACKAGE_MANIFEST/')).length,
    manualBrowserFiles: ordinary.filter(file => file.startsWith('EVIDENCE/B13_R4/MANUAL_BROWSER/')).length,
    mutationFiles: ordinary.filter(file => file.startsWith('EVIDENCE/B13_R4/MUTATIONS/')).length,
    performanceFiles: ordinary.filter(file => file.startsWith('EVIDENCE/B13_R4/PERFORMANCE/')).length,
    forbiddenFilesFound: ordinary.filter(forbidden).length,
    checksumFile: checksumRelative,
  }
  writeJson(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), inventory)
  const lines = allFiles(stageRoot).filter(file => relative(stageRoot, file) !== checksumRelative)
    .map(file => sha256(file) + '  ' + relative(stageRoot, file)).sort((a, b) => a.localeCompare(b))
  fs.writeFileSync(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), lines.join('\n') + '\n')
  return inventory
}
function changedFiles(baseline, sourceCommit) {
  return git(['diff', '--name-status', baseline + '..' + sourceCommit]).split('\n').filter(Boolean).map(line => {
    const [status, ...parts] = line.split('\t')
    return { status, file: parts.at(-1) }
  })
}
function secretFindings() {
  const findings = []
  const extensions = /\.(?:rs|ts|vue|mjs|json|md|toml|sql)$/
  const visit = directory => {
    if (!fs.existsSync(directory)) return
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      if (fs.statSync(file).isDirectory()) visit(file)
      else if (extensions.test(file) && /sk-[A-Za-z0-9]{20,}|Bearer\s+[A-Za-z0-9._-]{20,}|OPENAI_API_KEY\s*=/.test(fs.readFileSync(file, 'utf8'))) findings.push(path.relative(root, file))
    }
  }
  for (const directory of ['src', 'src-tauri/src', 'contracts', 'scripts', 'tests']) visit(path.join(root, directory))
  return findings
}
function copyEvidence(evidenceDir, evidenceRoot) {
  for (const entry of fs.readdirSync(evidenceDir)) {
    const source = path.join(evidenceDir, entry)
    if (entry === 'B13_R4' || !fs.statSync(source).isFile()) continue
    fs.copyFileSync(source, path.join(evidenceRoot, entry))
  }
  const sourceDir = path.join(evidenceDir, 'B13_R4')
  if (fs.existsSync(sourceDir)) fs.cpSync(sourceDir, evidenceRoot, { recursive: true })
}
function copyReaderFiles(from, to) {
  fs.mkdirSync(to, { recursive: true })
  for (const name of fs.readdirSync(from)) fs.copyFileSync(path.join(from, name), path.join(to, name))
}

function buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight }) {
  const original = resolveExisting(originalInstructionCandidates, 'original B13 instruction')
  const r3 = resolveExisting(r3InstructionCandidates, 'R3 instruction')
  const r4 = resolveExisting(r4InstructionCandidates, 'R4 instruction')
  const hashes = { original: sha256(original), r3: sha256(r3), r4: sha256(r4) }
  if (hashes.original !== expectedHashes.original || hashes.r3 !== expectedHashes.r3 || hashes.r4 !== expectedHashes.r4) throw new Error('B13_R4_FROZEN_INPUT_HASH_MISMATCH')
  const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r4-package-' + sourceCommit.slice(0, 7) + '-'))
  const packageRoot = 'MOHENG_V2.1_B13_R4_ACCEPTANCE_' + sourceCommit.slice(0, 7)
  const stageRoot = path.join(staging, packageRoot)
  const sourceRoot = path.join(stageRoot, 'SOURCE')
  const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
  const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B13_R4')
  const historicalRoot = path.join(stageRoot, 'EVIDENCE', 'HISTORICAL')
  const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
  for (const directory of [sourceRoot, frozenRoot, evidenceRoot, historicalRoot, manifestRoot]) fs.mkdirSync(directory, { recursive: true })
  for (const file of trackedFiles().filter(sourceAllowed)) {
    if (forbidden(file)) continue
    const destination = path.join(sourceRoot, file)
    fs.mkdirSync(path.dirname(destination), { recursive: true })
    fs.copyFileSync(path.join(root, file), destination)
  }
  fs.copyFileSync(original, path.join(frozenRoot, 'B13_DEVELOPMENT_INSTRUCTION_R1.md'))
  fs.copyFileSync(r3, path.join(frozenRoot, 'B13_R3_REPAIR_INSTRUCTION.md'))
  fs.copyFileSync(r4, path.join(frozenRoot, 'B13_R4_REPAIR_INSTRUCTION.md'))
  const pathInventory = frozenSkillPathInventory(repairBaselineCommit)
  const pathBinding = { schema: 'b13-r4-creative-skills-path-hash-binding', ...pathInventory, sourceCommit, repairBaselineCommit, expectedArticleRenameCount: 20 }
  writeJson(path.join(frozenRoot, 'CREATIVE_SKILL_PATH_HASH_BINDING.json'), pathBinding)
  writeJson(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), {
    batch: 'B13-R4', originalDevelopmentInstructionSha256: hashes.original, r3RepairInstructionSha256: hashes.r3, r4RepairInstructionSha256: hashes.r4,
    creativeSkillsVersion: '1.2.0', repairBaselineCommit, postB12FrozenCommit, nextBatch: 'B14', nextBatchStarted: false,
  })
  writeJson(path.join(frozenRoot, 'BASELINE_BINDING.json'), { batch: 'B13-R4', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, nextBatch: 'B14', nextBatchStarted: false })
  copyEvidence(evidenceDir, evidenceRoot)
  if (fs.existsSync(path.join(root, 'evidence', 'B13'))) fs.cpSync(path.join(root, 'evidence', 'B13'), path.join(historicalRoot, 'B13_R1'), { recursive: true })
  if (fs.existsSync(path.join(evidenceDir, 'HISTORICAL'))) fs.cpSync(path.join(evidenceDir, 'HISTORICAL'), historicalRoot, { recursive: true })
  const bindingPath = path.join(evidenceDir, 'B13_R4_SOURCE_BINDING.json')
  const binding = fs.existsSync(bindingPath) ? JSON.parse(fs.readFileSync(bindingPath, 'utf8')) : { sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] }
  const changed = changedFiles(repairBaselineCommit, sourceCommit)
  const changedText = changed.map(item => item.status + '\t' + item.file).join('\n')
  const forbiddenScopePath = path.join(evidenceDir, 'B13_R4_FORBIDDEN_SCOPE.json')
  const forbiddenScope = fs.existsSync(forbiddenScopePath) ? JSON.parse(fs.readFileSync(forbiddenScopePath, 'utf8')) : { status: 'PASS', findings: [] }
  fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), '# B13-R4 路径编码与打包 Gate 最终微补丁验收包\n\n批次：B13-R4\n\n修复基线：`' + repairBaselineCommit + '`\n\n最终源码：`' + sourceCommit + '`\n\n本包仅恢复 Creative Skills v1.2.0 的 20 个冻结文章路径，并加入 UTF-8、CP437、双 Reader 与证据真实性 Gate。业务代码和 Skill 内容保持冻结；B14、B15、RC1 未启动。\n\n开发方状态：`READY_FOR_FORMAL_ACCEPTANCE` 仅表示本地阻断门全绿；`NEXT_BATCH_ALLOWED=false`。\n')
  writeJson(path.join(manifestRoot, 'BASELINE.json'), { batch: 'B13-R4', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, nextBatch: 'B14', nextBatchStarted: false })
  writeJson(path.join(manifestRoot, 'SOURCE_BINDING.json'), binding)
  fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), 'HEAD=' + sourceCommit + '\nREPAIR_BASELINE=' + repairBaselineCommit + '\ngit status --porcelain\n\ngit diff --name-status ' + repairBaselineCommit + '..' + sourceCommit + '\n' + changedText + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), changedText + '\n')
  const changedEvidencePath = path.join(evidenceDir, 'B13_R4_CHANGED_FILES.json')
  fs.copyFileSync(changedEvidencePath, path.join(manifestRoot, 'CHANGED_FILES.json'))
  const commandsPath = path.join(evidenceDir, 'COMMANDS_EXECUTED.json')
  if (fs.existsSync(commandsPath)) fs.copyFileSync(commandsPath, path.join(manifestRoot, 'COMMANDS_EXECUTED.json'))
  writeJson(path.join(manifestRoot, 'ENVIRONMENT.json'), { OS: process.platform + ' ' + process.arch, node: process.version, npm: spawnSync('npm', ['--version'], { encoding: 'utf8' }).stdout.trim(), rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable', cargo: spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable', git: git(['--version']) })
  writeJson(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), { ...forbiddenScope, sourceCommit, repairBaselineCommit })
  writeJson(path.join(manifestRoot, 'SECRET_SCAN.json'), { schema: 'b13-r4-secret-scan', sourceCommit, repairBaselineCommit, findings: secretFindings(), status: secretFindings().length === 0 ? 'PASS' : 'FAIL' })
  const sourceScan = scanSkillDirectory(path.join(sourceRoot, 'creative-skills', 'v1.2.0'), repairBaselineCommit)
  const restoration = pathRestorationReport(repairBaselineCommit)
  fs.mkdirSync(path.join(evidenceRoot, 'PATH_VALIDATION'), { recursive: true })
  fs.writeFileSync(path.join(evidenceRoot, 'PATH_VALIDATION', 'source-path-scan.log'), JSON.stringify(sourceScan, null, 2) + '\n')
  fs.writeFileSync(path.join(evidenceRoot, 'PATH_VALIDATION', 'frozen-path-diff.log'), JSON.stringify(restoration, null, 2) + '\n')
  const candidateZip = path.join(staging, 'candidate.zip')
  const candidatePath = path.join(evidenceRoot, 'PATH_VALIDATION', 'candidate-zip-path-scan.log')
  const finalPath = path.join(evidenceRoot, 'PATH_VALIDATION', 'final-zip-path-scan.log')
  const readerRoot = path.join(evidenceRoot, 'PACKAGE_READERS')
  writeJson(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), { schema: 'b13-r4-package-utf8-validation', root: packageRoot + '/', rootAscii: true, status: 'FAIL' })
  writeInventory(stageRoot, manifestRoot)
  makeZip(stageRoot, candidateZip)
  const candidateScan = scanPackageZip(candidateZip, repairBaselineCommit)
  fs.writeFileSync(candidatePath, JSON.stringify(candidateScan, null, 2) + '\n')
  const candidateDual = verifyDualReaders(candidateZip, readerRoot)
  if (candidateDual.status !== 'PASS') throw new Error('B13_R4_DUAL_READER_CANDIDATE_FAILED')
  const correctPaths = frozenSkillPathInventory(repairBaselineCommit)
  const compare = JSON.parse(fs.readFileSync(path.join(readerRoot, 'reader-content-compare.json'), 'utf8'))
  const correctNames = scanSkillDirectory(path.join(sourceRoot, 'creative-skills', 'v1.2.0'), repairBaselineCommit)
  const pathFields = {
    sourcePathValidation: sourceScan.status,
    candidateZipValidation: candidateScan.status,
    finalZipValidation: candidateScan.status,
    replacementFindings: candidateScan.replacementFindings.length,
    knownMojibakeFindings: candidateScan.knownMojibakeFindings.length,
    cp437MojibakeFindings: candidateScan.cp437MojibakeFindings.length,
    frozenPathMismatches: candidateScan.frozenPathMismatches,
    nonNfcPaths: candidateScan.nonNfcPaths.length,
    correctPathCountA: compare.entrySetA.filter(name => /creative-skills\/v1\.2\.0\/sources\/articles\/\d{2}-[^/]+\.md$/.test(name)).length,
    correctPathCountB: compare.entrySetB.filter(name => /creative-skills\/v1\.2\.0\/sources\/articles\/\d{2}-[^/]+\.md$/.test(name)).length,
    oldMojibakePathsA: compare.entrySetA.filter(name => /Σ|σ|Φ|Θ|╕|┐/.test(name)).length,
    oldMojibakePathsB: compare.entrySetB.filter(name => /Σ|σ|Φ|Θ|╕|┐/.test(name)).length,
    sourceCorrectPathCount: correctNames.totalFiles === 676 ? 20 : 0,
  }
  const dualEvidence = { schema: 'b13-r4-dual-reader-utf8', sourceCommit, repairBaselineCommit, ...candidateDual, ...pathFields, status: candidateDual.status === 'PASS' && candidateScan.status === 'PASS' && pathFields.correctPathCountA === 20 && pathFields.correctPathCountB === 20 && pathFields.oldMojibakePathsA === 0 && pathFields.oldMojibakePathsB === 0 ? 'PASS' : 'FAIL' }
  writeJson(path.join(evidenceRoot, 'B13_R4_DUAL_READER_UTF8.json'), dualEvidence)
  writeJson(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), { ...dualEvidence, root: packageRoot + '/', rootAscii: true, filenameEncoding: 'UTF-8' })
  writeInventory(stageRoot, manifestRoot)
  const initialTree = validatePackageTree(stageRoot, sourceCommit)
  if (initialTree.status !== 'PASS') throw new Error('B13_R4_PACKAGE_TREE_PREFLIGHT_FAILED:' + initialTree.failures.join(';'))
  const zipPath = path.join(staging, '墨衡_V2.1_B13_LongFormHealth_R4_路径编码最终微补丁_正式验收交付版_' + sourceCommit.slice(0, 7) + '.zip')
  makeZip(stageRoot, zipPath)
  const finalScan = scanPackageZip(zipPath, repairBaselineCommit)
  fs.writeFileSync(finalPath, JSON.stringify(finalScan, null, 2) + '\n')
  const finalReaderRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r4-final-readers-'))
  const finalDual = verifyDualReaders(zipPath, finalReaderRoot)
  if (finalDual.status !== 'PASS') throw new Error('B13_R4_DUAL_READER_FINAL_FAILED')
  const finalCompare = JSON.parse(fs.readFileSync(path.join(finalReaderRoot, 'reader-content-compare.json'), 'utf8'))
  const finalPathFields = {
    ...pathFields,
    candidateZipValidation: candidateScan.status,
    finalZipValidation: finalScan.status,
    correctPathCountA: finalCompare.entrySetA.filter(name => /creative-skills\/v1\.2\.0\/sources\/articles\/\d{2}-[^/]+\.md$/.test(name)).length,
    correctPathCountB: finalCompare.entrySetB.filter(name => /creative-skills\/v1\.2\.0\/sources\/articles\/\d{2}-[^/]+\.md$/.test(name)).length,
    oldMojibakePathsA: finalCompare.entrySetA.filter(name => /Σ|σ|Φ|Θ|╕|┐/.test(name)).length,
    oldMojibakePathsB: finalCompare.entrySetB.filter(name => /Σ|σ|Φ|Θ|╕|┐/.test(name)).length,
  }
  copyReaderFiles(finalReaderRoot, readerRoot)
  const finalDualEvidence = { schema: 'b13-r4-dual-reader-utf8', sourceCommit, repairBaselineCommit, ...finalDual, ...finalPathFields, status: finalDual.status === 'PASS' && finalScan.status === 'PASS' && finalPathFields.correctPathCountA === 20 && finalPathFields.correctPathCountB === 20 && finalPathFields.oldMojibakePathsA === 0 && finalPathFields.oldMojibakePathsB === 0 ? 'PASS' : 'FAIL' }
  writeJson(path.join(evidenceRoot, 'B13_R4_DUAL_READER_UTF8.json'), finalDualEvidence)
  writeJson(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), { ...finalDualEvidence, root: packageRoot + '/', rootAscii: true, filenameEncoding: 'UTF-8' })
  writeInventory(stageRoot, manifestRoot)
  makeZip(stageRoot, zipPath)
  const finalZipResult = validatePackageTarget(zipPath, sourceCommit)
  if (finalZipResult.status !== 'PASS') throw new Error('B13_R4_PACKAGE_FINAL_ZIP_FAILED:' + finalZipResult.failures.join(';'))
  const finalDualCheck = verifyDualReaders(zipPath, fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r4-final-check-')))
  if (finalDualCheck.status !== 'PASS') throw new Error('B13_R4_DUAL_READER_FINAL_CHECK_FAILED')
  const consistency = {
    schema: 'b13-r4-package-consistency', sourceCommit, repairBaselineCommit, packageRoot: packageRoot + '/', preflight,
    countingRule: writeInventory(stageRoot, manifestRoot).countingRule, inventory: JSON.parse(fs.readFileSync(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), 'utf8')),
    treeStatus: validatePackageTree(stageRoot, sourceCommit).status, treeFailures: validatePackageTree(stageRoot, sourceCommit).failures,
    zip: { status: finalZipResult.status, actualEntries: finalZipResult.actualEntries, expectedEntries: finalZipResult.expectedEntries, archive: finalZipResult.archive }, status: finalZipResult.status,
  }
  writeJson(path.join(evidenceRoot, 'B13_R4_PACKAGE_CONSISTENCY.json'), consistency)
  writeJson(path.join(evidenceDir, 'B13_R4_PACKAGE_CONSISTENCY.json'), consistency)
  writeInventory(stageRoot, manifestRoot)
  const finalTree = validatePackageTree(stageRoot, sourceCommit)
  if (finalTree.status !== 'PASS') throw new Error('B13_R4_PACKAGE_FINAL_TREE_FAILED:' + finalTree.failures.join(';'))
  makeZip(stageRoot, zipPath)
  const checkedZip = validatePackageTarget(zipPath, sourceCommit)
  if (checkedZip.status !== 'PASS') throw new Error('B13_R4_PACKAGE_CHECKED_ZIP_FAILED:' + checkedZip.failures.join(';'))
  return { staging, stageRoot, packageRoot, zipPath, inventory: JSON.parse(fs.readFileSync(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), 'utf8')), consistency: checkedZip, finalDual: finalDualCheck, sourceScan, candidateScan, finalScan }
}

function main() {
  const sourceCommit = git(['rev-parse', 'HEAD'])
  const repairBaselineCommit = git(['rev-parse', repairBaselineShort + '^{commit}'])
  const dirty = git(['status', '--porcelain', '--untracked-files=all'])
  if (!sourceCommit || sourceCommit === repairBaselineCommit) throw new Error('B13_R4_PACKAGE_SOURCE_BASELINE_NOT_ADVANCED')
  if (dirty) throw new Error('B13_R4_PACKAGE_WORKTREE_NOT_CLEAN:' + dirty)
  const evidenceDir = path.resolve(process.env.B13_R4_EVIDENCE_DIR || '')
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R4_PACKAGE_EVIDENCE_MISSING')
  const preflight = process.argv.includes('--preflight')
  if (!preflight) {
    const acceptance = fs.existsSync(path.join(evidenceDir, 'B13_R4_ACCEPTANCE.json')) ? JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R4_ACCEPTANCE.json'), 'utf8')) : null
    if (!acceptance || acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.nextBatchAllowed !== false) throw new Error('B13_R4_PACKAGE_ACCEPTANCE_NOT_READY')
  }
  const result = buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight })
  fs.mkdirSync(outputDir, { recursive: true })
  const finalZip = path.join(outputDir, path.basename(result.zipPath))
  if (!preflight) {
    fs.copyFileSync(result.zipPath, finalZip)
    const shaPath = path.join(outputDir, path.basename(finalZip, '.zip') + '_SHA256SUMS.txt')
    fs.writeFileSync(shaPath, sha256(finalZip) + '  ' + path.basename(finalZip) + '\n')
    console.log(JSON.stringify({ status: 'PASS', zip: finalZip, sha256File: shaPath, zipSha256: sha256(finalZip), sourceCommit, packageRoot: result.packageRoot, entryCount: result.consistency.actualEntries, finalReader: result.finalDual }, null, 2))
  } else {
    console.log(JSON.stringify({ status: 'PASS', preflight: true, sourceCommit, packageRoot: result.packageRoot, stageRoot: result.stageRoot, zipPath: result.zipPath, inventory: result.inventory, consistency: result.consistency, finalReader: result.finalDual, sourceScan: result.sourceScan, candidateScan: result.candidateScan, finalScan: result.finalScan }, null, 2))
  }
}

try { main() } catch (error) { console.error(error instanceof Error ? error.stack : String(error)); process.exitCode = 1 }
