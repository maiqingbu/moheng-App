import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { validatePackageTarget, validatePackageTree } from './verify-package-consistency.mjs'
import { verifyDualReaders } from './verify-b13-r3-dual-reader.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outputDir = '/Users/mac/Desktop/交付包'
const repairBaselineShort = 'e6419ce'
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const originalInstructionCandidates = [
  process.env.B13_ORIGINAL_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md',
  '/Users/mac/Desktop/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md',
].filter(Boolean)
const r2InstructionCandidates = [
  process.env.B13_R2_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R2_最终集成修复与重新打包执行指令_2026-09-05.md',
  '/Users/mac/Desktop/墨衡_V2.1_B13_LongFormHealth_R2_最终集成修复与重新打包执行指令_2026-09-05.md',
].filter(Boolean)
const r3InstructionCandidates = [
  process.env.B13_R3_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R3_最终封口修复与正式重新打包执行指令_2026-09-05.md',
  '/Users/mac/Desktop/墨衡_V2.1_B13_LongFormHealth_R3_最终封口修复与正式重新打包执行指令_2026-09-05.md',
].filter(Boolean)
const expectedOriginalInstructionSha256 = '54a10882729ae79497231f49e465740d39bfb67e6683d0feb0446403bbe416cb'
const expectedR2InstructionSha256 = '2407c392e496dae0677f2dfd0c5fa0d7414af56e47f963bf544e7ea5bffabfce'
const expectedR3InstructionSha256 = '9340d016721506b1fe5d63f6ba4319530257bf4eba2c506573cc0a19aa71f5b9'

function gitResult(args) {
  return spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
}

function git(args) {
  return gitResult(args).stdout.trim()
}

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex')
}

function sha256(file) {
  return sha256Bytes(fs.readFileSync(file))
}

function resolveExisting(candidates, label) {
  const file = candidates.find(candidate => fs.existsSync(candidate) && fs.statSync(candidate).isFile())
  if (!file) throw new Error('B13_R3_FROZEN_INPUT_MISSING:' + label + ':' + candidates.join('|'))
  return file
}

function trackedFiles() {
  return gitResult(['ls-files', '-z']).stdout.split('\0').filter(Boolean)
}

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
    for (const name of fs.readdirSync(current).sort((left, right) => left.localeCompare(right))) {
      const file = path.join(current, name)
      if (fs.statSync(file).isDirectory()) visit(file)
      else files.push(file)
    }
  }
  visit(directory)
  return files
}

function relative(stageRoot, file) {
  return path.relative(stageRoot, file).split(path.sep).join('/')
}

function runPython(script, args) {
  return spawnSync('python3', ['-c', script, ...args], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
}

function makeZip(stageRoot, zipPath) {
  const script = [
    'import os,sys,zipfile',
    'root=sys.argv[1]',
    'out=sys.argv[2]',
    'with zipfile.ZipFile(out,\"w\",zipfile.ZIP_DEFLATED) as z:',
    '    for base,dirs,files in os.walk(root):',
    '        dirs.sort(); files.sort()',
    '        for name in files:',
    '            p=os.path.join(base,name)',
    '            arc=os.path.relpath(p,os.path.dirname(root)).replace(os.sep,\"/\")',
    '            z.write(p,arc)',
    '',
  ].join('\n')
  const result = runPython(script, [stageRoot, zipPath])
  if (result.status !== 0) throw new Error('B13_R3_ZIP_CREATE_FAILED:' + result.stderr)
}

function writeInventory(stageRoot, manifestRoot) {
  const sumsRelative = 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt'
  const inventoryPath = path.join(manifestRoot, 'PACKAGE_INVENTORY.json')
  if (!fs.existsSync(inventoryPath)) fs.writeFileSync(inventoryPath, '{}\n')
  const files = allFiles(stageRoot)
  const ordinary = files.map(file => relative(stageRoot, file)).filter(name => name !== sumsRelative).sort((left, right) => left.localeCompare(right))
  const forbiddenFiles = ordinary.filter(name => forbidden(name))
  const inventory = {
    schema: 'b13-r3-package-inventory',
    countingRule: 'totalFilesExcludingChecksums counts every regular file under the package root, including PACKAGE_INVENTORY.json and all evidence/manifests, excluding only PACKAGE_MANIFEST/FILES_SHA256SUMS.txt; ZIP file entries equal this value plus one checksum file.',
    totalFilesExcludingChecksums: ordinary.length,
    sourceFiles: ordinary.filter(name => name.startsWith('SOURCE/')).length,
    frozenInputFiles: ordinary.filter(name => name.startsWith('FROZEN_INPUTS/')).length,
    evidenceFiles: ordinary.filter(name => name.startsWith('EVIDENCE/')).length,
    manifestFiles: ordinary.filter(name => name.startsWith('PACKAGE_MANIFEST/')).length,
    manualBrowserFiles: ordinary.filter(name => name.startsWith('EVIDENCE/B13_R3/MANUAL_BROWSER/')).length,
    mutationFiles: ordinary.filter(name => name.startsWith('EVIDENCE/B13_R3/MUTATIONS/')).length,
    performanceFiles: ordinary.filter(name => name.startsWith('EVIDENCE/B13_R3/PERFORMANCE/')).length,
    forbiddenFilesFound: forbiddenFiles.length,
    checksumFile: sumsRelative,
  }
  fs.writeFileSync(inventoryPath, JSON.stringify(inventory, null, 2) + '\n')
  const sumsPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
  const lines = allFiles(stageRoot)
    .filter(file => relative(stageRoot, file) !== sumsRelative)
    .map(file => sha256(file) + '  ' + relative(stageRoot, file))
    .sort((left, right) => left.localeCompare(right))
  fs.writeFileSync(sumsPath, lines.join('\n') + '\n')
  return inventory
}

function changedFiles(baseline, sourceCommit) {
  return git(['diff', '--name-status', baseline + '..' + sourceCommit])
    .split('\n')
    .filter(Boolean)
    .map(line => {
      const [status, ...parts] = line.split('\t')
      return { status, file: parts.at(-1) }
    })
}

function secretFindings() {
  const extensions = /\.(?:rs|ts|vue|mjs|json|md|toml|sql)$/
  const findings = []
  const scan = directory => {
    if (!fs.existsSync(directory)) return
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      if (fs.statSync(file).isDirectory()) scan(file)
      else if (extensions.test(file) && /sk-[A-Za-z0-9]{20,}|Bearer\s+[A-Za-z0-9._-]{20,}|OPENAI_API_KEY\s*=/.test(fs.readFileSync(file, 'utf8'))) findings.push(path.relative(root, file))
    }
  }
  for (const directory of ['src', 'src-tauri/src', 'contracts', 'scripts', 'tests']) scan(path.join(root, directory))
  return findings
}

function copyEvidence(evidenceDir, evidenceRoot) {
  for (const entry of fs.readdirSync(evidenceDir)) {
    if (entry === 'B13_R3') continue
    const source = path.join(evidenceDir, entry)
    if (!fs.statSync(source).isFile()) continue
    fs.copyFileSync(source, path.join(evidenceRoot, entry))
  }
  const sourceDir = path.join(evidenceDir, 'B13_R3')
  if (fs.existsSync(sourceDir)) fs.cpSync(sourceDir, evidenceRoot, { recursive: true })
}

function buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight }) {
  const originalInstruction = resolveExisting(originalInstructionCandidates, 'original B13 development instruction')
  const r2Instruction = resolveExisting(r2InstructionCandidates, 'B13-R2 repair instruction')
  const r3Instruction = resolveExisting(r3InstructionCandidates, 'B13-R3 repair instruction')
  const hashes = {
    original: sha256(originalInstruction),
    r2: sha256(r2Instruction),
    r3: sha256(r3Instruction),
  }
  if (hashes.original !== expectedOriginalInstructionSha256
    || hashes.r2 !== expectedR2InstructionSha256
    || hashes.r3 !== expectedR3InstructionSha256) throw new Error('B13_R3_FROZEN_INPUT_HASH_MISMATCH')
  const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-package-' + sourceCommit.slice(0, 7) + '-'))
  const packageRoot = 'MOHENG_V2.1_B13_R3_ACCEPTANCE_' + sourceCommit.slice(0, 7)
  const stageRoot = path.join(staging, packageRoot)
  const sourceRoot = path.join(stageRoot, 'SOURCE')
  const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
  const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B13_R3')
  const historicalRoot = path.join(stageRoot, 'EVIDENCE', 'HISTORICAL')
  const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
  for (const directory of [sourceRoot, frozenRoot, evidenceRoot, historicalRoot, manifestRoot]) fs.mkdirSync(directory, { recursive: true })

  for (const file of trackedFiles().filter(sourceAllowed)) {
    if (forbidden(file)) continue
    const destination = path.join(sourceRoot, file)
    fs.mkdirSync(path.dirname(destination), { recursive: true })
    fs.copyFileSync(path.join(root, file), destination)
  }
  fs.copyFileSync(originalInstruction, path.join(frozenRoot, 'B13_DEVELOPMENT_INSTRUCTION_R1.md'))
  fs.copyFileSync(r2Instruction, path.join(frozenRoot, 'B13_R2_REPAIR_INSTRUCTION.md'))
  fs.copyFileSync(r3Instruction, path.join(frozenRoot, 'B13_R3_REPAIR_INSTRUCTION.md'))
  fs.writeFileSync(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), JSON.stringify({
    batch: 'B13-R3',
    originalDevelopmentInstructionSha256: hashes.original,
    r2RepairInstructionSha256: hashes.r2,
    r3RepairInstructionSha256: hashes.r3,
    repairBaselineCommit,
    postB12FrozenCommit,
    nextBatch: 'B14',
    nextBatchStarted: false,
  }, null, 2) + '\n')
  fs.writeFileSync(path.join(frozenRoot, 'BASELINE_BINDING.json'), JSON.stringify({
    batch: 'B13-R3',
    repairBaselineShort,
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    nextBatch: 'B14',
    nextBatchStarted: false,
  }, null, 2) + '\n')
  copyEvidence(evidenceDir, evidenceRoot)
  if (fs.existsSync(path.join(root, 'evidence', 'B13'))) fs.cpSync(path.join(root, 'evidence', 'B13'), path.join(historicalRoot, 'B13_R1'), { recursive: true })
  if (fs.existsSync(path.join(evidenceDir, 'HISTORICAL'))) fs.cpSync(path.join(evidenceDir, 'HISTORICAL'), historicalRoot, { recursive: true })

  const binding = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R3_SOURCE_BINDING.json'), 'utf8'))
  const changed = changedFiles(repairBaselineCommit, sourceCommit)
  const changedText = changed.map(item => item.status + '\t' + item.file).join('\n')
  const forbiddenScope = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R3_FORBIDDEN_SCOPE.json'), 'utf8'))
  const secrets = secretFindings()
  fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), '# B13-R3 长篇健康治理最终封口修复验收包\n\n批次：B13-R3\n\n修复基线：\`' + repairBaselineCommit + '\`\n\n最终源码：\`' + sourceCommit + '\`\n\n本包只包含三组定点修复：bounded 但完整的 Task Reality、无内部规划类型名的正式用户界面、以及当前 H03/H04 Query Plan 与双 Reader ZIP 证据。B01-B13 已有通过内容保持冻结；B14、B15、RC1 未启动。\n\n开发方状态：\`READY_FOR_FORMAL_ACCEPTANCE\` 仅表示本地阻断门全绿，不代表外部正式签字；\`NEXT_BATCH_ALLOWED=false\`。\n')
  fs.writeFileSync(path.join(manifestRoot, 'BASELINE.json'), JSON.stringify({ batch: 'B13-R3', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, nextBatch: 'B14', nextBatchStarted: false }, null, 2) + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'SOURCE_BINDING.json'), JSON.stringify(binding, null, 2) + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), 'HEAD=' + sourceCommit + '\nREPAIR_BASELINE=' + repairBaselineCommit + '\ngit status --porcelain\n\ngit diff --name-status ' + repairBaselineCommit + '..' + sourceCommit + '\n' + changedText + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), changedText + '\n')
  const changedEvidencePath = path.join(evidenceDir, 'B13_R3_CHANGED_FILES.json')
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.json'), fs.existsSync(changedEvidencePath) ? fs.readFileSync(changedEvidencePath, 'utf8') : JSON.stringify({ diffBase: repairBaselineCommit, paths: changed }, null, 2) + '\n')
  const commandsPath = path.join(evidenceDir, 'COMMANDS_EXECUTED.json')
  fs.writeFileSync(path.join(manifestRoot, 'COMMANDS_EXECUTED.json'), fs.existsSync(commandsPath) ? fs.readFileSync(commandsPath, 'utf8') : '{}\n')
  fs.writeFileSync(path.join(manifestRoot, 'ENVIRONMENT.json'), JSON.stringify({
    OS: process.platform + ' ' + process.arch,
    node: process.version,
    npm: spawnSync('npm', ['--version'], { encoding: 'utf8' }).stdout.trim(),
    rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    cargo: spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    git: git(['--version']),
  }, null, 2) + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), JSON.stringify({ ...forbiddenScope, sourceCommit, repairBaselineCommit }, null, 2) + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'SECRET_SCAN.json'), JSON.stringify({ schema: 'b13-r3-secret-scan', sourceCommit, repairBaselineCommit, realSecretFindings: secrets, status: secrets.length === 0 ? 'PASS' : 'FAIL' }, null, 2) + '\n')
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), JSON.stringify({ schema: 'b13-r3-package-utf8-validation', root: packageRoot + '/', rootAscii: true, status: 'FAIL' }, null, 2) + '\n')
  const consistencyRelative = 'EVIDENCE/B13_R3/B13_R3_PACKAGE_CONSISTENCY.json'
  const consistencyStagePath = path.join(stageRoot, consistencyRelative)
  if (!fs.existsSync(consistencyStagePath)) fs.writeFileSync(consistencyStagePath, JSON.stringify({ schema: 'b13-r3-package-consistency', sourceCommit, repairBaselineCommit, status: 'PASS' }, null, 2) + '\n')
  let inventory = writeInventory(stageRoot, manifestRoot)
  const candidateZip = path.join(staging, 'candidate.zip')
  makeZip(stageRoot, candidateZip)
  const readerRoot = path.join(stageRoot, 'EVIDENCE', 'B13_R3', 'PACKAGE_READERS')
  fs.mkdirSync(readerRoot, { recursive: true })
  const candidateDual = verifyDualReaders(candidateZip, readerRoot)
  if (candidateDual.status !== 'PASS') throw new Error('B13_R3_DUAL_READER_CANDIDATE_FAILED')
  const packageUtf8 = { ...candidateDual, root: packageRoot + '/', rootAscii: true, filenameEncoding: 'UTF-8' }
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), JSON.stringify(packageUtf8, null, 2) + '\n')
  fs.copyFileSync(path.join(readerRoot, 'B13_R3_DUAL_READER_UTF8.json'), path.join(evidenceRoot, 'B13_R3_DUAL_READER_UTF8.json'))
  fs.copyFileSync(path.join(readerRoot, 'B13_R3_DUAL_READER_UTF8.json'), path.join(evidenceDir, 'B13_R3_DUAL_READER_UTF8.json'))
  inventory = writeInventory(stageRoot, manifestRoot)
  let tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') throw new Error('B13_R3_PACKAGE_TREE_PREFLIGHT_FAILED:' + tree.failures.join(';'))
  const report = {
    schema: 'b13-r3-package-consistency',
    sourceCommit,
    repairBaselineCommit,
    packageRoot: packageRoot + '/',
    preflight,
    countingRule: inventory.countingRule,
    inventory,
    treeStatus: tree.status,
    treeFailures: tree.failures,
    status: 'PASS',
  }
  fs.writeFileSync(consistencyStagePath, JSON.stringify(report, null, 2) + '\n')
  fs.writeFileSync(path.join(evidenceDir, 'B13_R3_PACKAGE_CONSISTENCY.json'), JSON.stringify(report, null, 2) + '\n')
  writeInventory(stageRoot, manifestRoot)
  tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') throw new Error('B13_R3_PACKAGE_TREE_FAILED:' + tree.failures.join(';'))
  const zipPath = path.join(staging, '墨衡_V2.1_B13_LongFormHealth_R3_最终封口修复_正式验收交付版_' + sourceCommit.slice(0, 7) + '.zip')
  makeZip(stageRoot, zipPath)
  const zipResult = validatePackageTarget(zipPath, sourceCommit)
  if (zipResult.status !== 'PASS') throw new Error('B13_R3_PACKAGE_ZIP_FAILED:' + zipResult.failures.join(';'))
  const finalReport = {
    ...report,
    treeStatus: tree.status,
    treeFailures: tree.failures,
    zip: { status: zipResult.status, actualEntries: zipResult.actualEntries, expectedEntries: zipResult.expectedEntries, archive: zipResult.archive },
    status: 'PASS',
  }
  fs.writeFileSync(consistencyStagePath, JSON.stringify(finalReport, null, 2) + '\n')
  fs.writeFileSync(path.join(evidenceDir, 'B13_R3_PACKAGE_CONSISTENCY.json'), JSON.stringify(finalReport, null, 2) + '\n')
  writeInventory(stageRoot, manifestRoot)
  tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') throw new Error('B13_R3_PACKAGE_FINAL_TREE_FAILED:' + tree.failures.join(';'))
  makeZip(stageRoot, zipPath)
  const finalZipResult = validatePackageTarget(zipPath, sourceCommit)
  if (finalZipResult.status !== 'PASS') throw new Error('B13_R3_PACKAGE_FINAL_ZIP_FAILED:' + finalZipResult.failures.join(';'))
  const finalReaderRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-final-readers-'))
  const finalDual = verifyDualReaders(zipPath, finalReaderRoot)
  if (finalDual.status !== 'PASS') throw new Error('B13_R3_DUAL_READER_FINAL_FAILED')
  return { staging, stageRoot, packageRoot, zipPath, inventory, consistency: finalZipResult, finalDual }
}

function main() {
  const sourceCommit = git(['rev-parse', 'HEAD'])
  const repairBaselineCommit = git(['rev-parse', repairBaselineShort + '^{commit}'])
  const dirty = git(['status', '--porcelain', '--untracked-files=all'])
  if (!sourceCommit || sourceCommit === repairBaselineCommit) throw new Error('B13_R3_PACKAGE_SOURCE_BASELINE_NOT_ADVANCED')
  if (dirty) throw new Error('B13_R3_PACKAGE_WORKTREE_NOT_CLEAN:' + dirty)
  const evidenceDir = path.resolve(process.env.B13_R3_EVIDENCE_DIR || '')
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R3_PACKAGE_EVIDENCE_MISSING')
  const preflight = process.argv.includes('--preflight')
  if (!preflight) {
    const acceptancePath = path.join(evidenceDir, 'B13_R3_ACCEPTANCE.json')
    const acceptance = fs.existsSync(acceptancePath) ? JSON.parse(fs.readFileSync(acceptancePath, 'utf8')) : null
    if (!acceptance || acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.nextBatchAllowed !== false) throw new Error('B13_R3_PACKAGE_ACCEPTANCE_NOT_READY')
  }
  const result = buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight })
  fs.mkdirSync(outputDir, { recursive: true })
  const finalZip = path.join(outputDir, path.basename(result.zipPath).replace(/^candidate\.zip$/, ''))
  if (!preflight) {
    fs.copyFileSync(result.zipPath, finalZip)
    const shaPath = path.join(outputDir, path.basename(finalZip, '.zip') + '_SHA256SUMS.txt')
    fs.writeFileSync(shaPath, sha256(finalZip) + '  ' + path.basename(finalZip) + '\n')
    console.log(JSON.stringify({
      status: 'PASS',
      zip: finalZip,
      sha256File: shaPath,
      zipSha256: sha256(finalZip),
      sourceCommit,
      packageRoot: result.packageRoot,
      entryCount: result.consistency.actualEntries,
      finalReader: result.finalDual,
    }, null, 2))
  } else {
    console.log(JSON.stringify({
      status: 'PASS',
      preflight: true,
      sourceCommit,
      packageRoot: result.packageRoot,
      stageRoot: result.stageRoot,
      zipPath: result.zipPath,
      inventory: result.inventory,
      consistency: result.consistency,
      finalReader: result.finalDual,
    }, null, 2))
  }
}

try { main() } catch (error) { console.error(error instanceof Error ? error.stack : String(error)); process.exitCode = 1 }
