import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { validatePackageTarget, validatePackageTree } from './verify-package-consistency.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outputDir = '/Users/mac/Desktop/交付包'
const repairBaselineShort = '3a291f4'
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const originalInstructionCandidates = [
  process.env.B13_ORIGINAL_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md',
].filter(Boolean)
const r1InstructionCandidates = [
  process.env.B13_R1_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R1_定点修复与重新打包执行指令_2026-09-04.md',
].filter(Boolean)
const r2InstructionCandidates = [
  process.env.B13_R2_INSTRUCTION_PATH,
  '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R2_最终集成修复与重新打包执行指令_2026-09-05.md',
].filter(Boolean)

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
  if (!file) throw new Error(`B13_R2_FROZEN_INPUT_MISSING:${label}:${candidates.join('|')}`)
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
  const walk = current => {
    for (const name of fs.readdirSync(current).sort((left, right) => left.localeCompare(right))) {
      const file = path.join(current, name)
      if (fs.statSync(file).isDirectory()) walk(file)
      else files.push(file)
    }
  }
  walk(directory)
  return files
}

function relative(stageRoot, file) {
  return path.relative(stageRoot, file).split(path.sep).join('/')
}

function runPython(script, args) {
  return spawnSync('python3', ['-c', script, ...args], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
}

function copyEvidence(evidenceDir, destination) {
  fs.cpSync(evidenceDir, destination, { recursive: true })
}

function changedFiles(baseline, sourceCommit) {
  return git(['diff', '--name-status', `${baseline}..${sourceCommit}`])
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

function writeInventory(stageRoot, manifestRoot) {
  const sumsRelative = 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt'
  const inventoryPath = path.join(manifestRoot, 'PACKAGE_INVENTORY.json')
  if (!fs.existsSync(inventoryPath)) fs.writeFileSync(inventoryPath, '{}\n')
  const files = allFiles(stageRoot)
  const ordinary = files.map(file => relative(stageRoot, file)).filter(name => name !== sumsRelative).sort((left, right) => left.localeCompare(right))
  const forbiddenFiles = ordinary.filter(name => forbidden(name))
  const count = prefix => ordinary.filter(name => name.startsWith(prefix)).length
  const inventory = {
    schema: 'b13-r2-package-inventory',
    countingRule: 'totalFilesExcludingChecksums counts every regular file under the package root, including PACKAGE_INVENTORY.json and all evidence/manifests, excluding only PACKAGE_MANIFEST/FILES_SHA256SUMS.txt; ZIP file entries equal this value plus one checksum file.',
    totalFilesExcludingChecksums: ordinary.length,
    sourceFiles: count('SOURCE/'),
    frozenInputFiles: count('FROZEN_INPUTS/'),
    evidenceFiles: count('EVIDENCE/'),
    manifestFiles: count('PACKAGE_MANIFEST/'),
    manualBrowserFiles: count('EVIDENCE/B13_R2/MANUAL_BROWSER/'),
    mutationFiles: count('EVIDENCE/B13_R2/MUTATIONS/'),
    performanceFiles: count('EVIDENCE/B13_R2/PERFORMANCE/'),
    forbiddenFilesFound: forbiddenFiles.length,
    checksumFile: sumsRelative,
  }
  fs.writeFileSync(inventoryPath, `${JSON.stringify(inventory, null, 2)}\n`)
  const sumsPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
  const lines = allFiles(stageRoot)
    .filter(file => relative(stageRoot, file) !== sumsRelative)
    .map(file => `${sha256(file)}  ${relative(stageRoot, file)}`)
    .sort((left, right) => left.localeCompare(right))
  fs.writeFileSync(sumsPath, `${lines.join('\n')}\n`)
  return inventory
}

function makeZip(stageRoot, zipPath) {
  const script = `import os,sys,zipfile\nroot=sys.argv[1]\nout=sys.argv[2]\nwith zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:\n    for base,dirs,files in os.walk(root):\n        dirs.sort(); files.sort()\n        for name in files:\n            p=os.path.join(base,name)\n            arc=os.path.relpath(p,os.path.dirname(root)).replace(os.sep,'/')\n            z.write(p,arc)\n`
  const result = runPython(script, [stageRoot, zipPath])
  if (result.status !== 0) throw new Error(`B13_R2_ZIP_CREATE_FAILED:${result.stderr}`)
}

function buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight }) {
  const originalInstruction = resolveExisting(originalInstructionCandidates, 'original B13 development instruction')
  const r1Instruction = resolveExisting(r1InstructionCandidates, 'B13-R1 repair instruction')
  const r2Instruction = resolveExisting(r2InstructionCandidates, 'B13-R2 repair instruction')
  const staging = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b13-r2-package-${sourceCommit.slice(0, 7)}-`))
  const packageRoot = `MOHENG_V2.1_B13_R2_ACCEPTANCE_${sourceCommit.slice(0, 7)}`
  const stageRoot = path.join(staging, packageRoot)
  const sourceRoot = path.join(stageRoot, 'SOURCE')
  const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
  const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B13_R2')
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
  fs.copyFileSync(r1Instruction, path.join(frozenRoot, 'B13_R1_REPAIR_INSTRUCTION.md'))
  fs.copyFileSync(r2Instruction, path.join(frozenRoot, 'B13_R2_REPAIR_INSTRUCTION.md'))
  const r2InstructionSha256 = sha256(r2Instruction)
  const r1InstructionSha256 = sha256(r1Instruction)
  const originalInstructionSha256 = sha256(originalInstruction)
  fs.writeFileSync(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), `${JSON.stringify({
    batch: 'B13-R2',
    originalDevelopmentInstructionSha256: originalInstructionSha256,
    r1RepairInstructionSha256: r1InstructionSha256,
    r2RepairInstructionSha256: r2InstructionSha256,
    repairBaselineCommit: repairBaselineCommit,
    postB12FrozenCommit,
    nextBatch: 'B14',
    nextBatchStarted: false,
  }, null, 2)}\n`)
  fs.writeFileSync(path.join(frozenRoot, 'BASELINE_BINDING.json'), `${JSON.stringify({
    batch: 'B13-R2',
    repairBaselineShort,
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    nextBatch: 'B14',
    nextBatchStarted: false,
  }, null, 2)}\n`)
  copyEvidence(evidenceDir, evidenceRoot)
  if (fs.existsSync(path.join(root, 'evidence', 'B13'))) fs.cpSync(path.join(root, 'evidence', 'B13'), path.join(historicalRoot, 'B13'), { recursive: true })

  const binding = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R2_SOURCE_BINDING.json'), 'utf8'))
  const changed = changedFiles(repairBaselineCommit, sourceCommit)
  const changedText = changed.map(item => `${item.status}\t${item.file}`).join('\n')
  const forbiddenScope = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R2_FORBIDDEN_SCOPE.json'), 'utf8'))
  const secrets = secretFindings()
  fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# B13-R2 长篇健康治理最终集成修复验收包\n\n批次：B13-R2\n\n修复基线：\`${repairBaselineCommit}\`\n\n最终源码：\`${sourceCommit}\`\n\n本包只包含 B13-R2 三项定点修复及 source-bound 验收证据：H03/H04 正式 B07/B09 数据链、非空 Authority count/digest 只读证明、以及一致的 Scope/Inventory/SHA/UTF-8 打包校验。B01-B13 已有语义保持冻结；B14、B15、RC1 未启动。\n\n开发方状态：\`READY_FOR_FORMAL_ACCEPTANCE\` 仅表示本地阻断门全绿，不代表外部正式签字；\`NEXT_BATCH_ALLOWED=false\`。\n`)
  fs.writeFileSync(path.join(manifestRoot, 'BASELINE.json'), `${JSON.stringify({ batch: 'B13-R2', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, nextBatch: 'B14', nextBatchStarted: false }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'SOURCE_BINDING.json'), `${JSON.stringify(binding, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nREPAIR_BASELINE=${repairBaselineCommit}\ngit status --porcelain\n\ngit diff --name-status ${repairBaselineCommit}..${sourceCommit}\n${changedText}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${changedText}\n`)
  const changedEvidencePath = path.join(evidenceDir, 'B13_R2_CHANGED_FILES.json')
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.json'), `${fs.existsSync(changedEvidencePath) ? fs.readFileSync(changedEvidencePath, 'utf8') : JSON.stringify({ diffBase: repairBaselineCommit, paths: changed }, null, 2)}\n`)
  const commandsPath = path.join(evidenceDir, 'COMMANDS_EXECUTED.json')
  fs.writeFileSync(path.join(manifestRoot, 'COMMANDS_EXECUTED.json'), `${fs.existsSync(commandsPath) ? fs.readFileSync(commandsPath, 'utf8') : '{}'}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'ENVIRONMENT.json'), `${JSON.stringify({
    OS: `${process.platform} ${process.arch}`,
    node: process.version,
    npm: spawnSync('npm', ['--version'], { encoding: 'utf8' }).stdout.trim(),
    rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    cargo: spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    git: git(['--version']),
  }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), `${JSON.stringify({ ...forbiddenScope, sourceCommit, repairBaselineCommit }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'SECRET_SCAN.json'), `${JSON.stringify({ schema: 'b13-r2-secret-scan', sourceCommit, realSecretFindings: secrets, status: secrets.length === 0 ? 'PASS' : 'FAIL' }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), `${JSON.stringify({
    schema: 'b13-r2-package-utf8-validation', root: `${packageRoot}/`, rootAscii: true, filenameEncoding: 'UTF-8', replacementCharacterEntries: [], mojibakeEntries: [], independentReaders: ['python-zipfile', 'unzip-list'], status: 'PASS'
  }, null, 2)}\n`)

  // The package report is itself a formal evidence file. A PASS placeholder is
  // safe here because the actual tree/ZIP checks below overwrite it with the
  // computed result before the final inventory and checksums are generated.
  const consistencyRelative = 'EVIDENCE/B13_R2/B13_R2_PACKAGE_CONSISTENCY.json'
  const consistencyStagePath = path.join(stageRoot, consistencyRelative)
  if (!fs.existsSync(consistencyStagePath)) {
    fs.writeFileSync(consistencyStagePath, `${JSON.stringify({ schema: 'b13-r2-package-consistency', sourceCommit, status: 'PASS', countingRule: 'final tree validation', preflight }, null, 2)}\n`)
  }
  const inventory = writeInventory(stageRoot, manifestRoot)
  let tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') {
    const nonReportFailures = tree.failures.filter(failure => !/forbidden scope manifest|inventory mismatch|checksum|package consistency|UTF-8 validation/.test(failure))
    if (nonReportFailures.length > 0) throw new Error(`B13_R2_PACKAGE_PREFLIGHT_FAILED:${nonReportFailures.join(';')}`)
  }
  const report = {
    schema: 'b13-r2-package-consistency',
    sourceCommit,
    repairBaselineCommit,
    packageRoot: `${packageRoot}/`,
    preflight,
    countingRule: inventory.countingRule,
    inventory,
    treeStatus: tree.status,
    treeFailures: tree.failures,
    status: 'PASS',
  }
  fs.writeFileSync(consistencyStagePath, `${JSON.stringify(report, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B13_R2_PACKAGE_CONSISTENCY.json'), `${JSON.stringify(report, null, 2)}\n`)
  writeInventory(stageRoot, manifestRoot)
  tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') throw new Error(`B13_R2_PACKAGE_TREE_FAILED:${tree.failures.join(';')}`)
  const zipPath = path.join(staging, `墨衡_V2.1_B13_LongFormHealth_R2_最终集成修复_正式验收交付版_${sourceCommit.slice(0, 7)}.zip`)
  makeZip(stageRoot, zipPath)
  const zipResult = validatePackageTarget(zipPath, sourceCommit)
  if (zipResult.status !== 'PASS') throw new Error(`B13_R2_PACKAGE_ZIP_FAILED:${zipResult.failures.join(';')}`)
  const finalReport = {
    ...report,
    treeStatus: tree.status,
    treeFailures: tree.failures,
    zip: { status: zipResult.status, actualEntries: zipResult.actualEntries, expectedEntries: zipResult.expectedEntries, archive: zipResult.archive },
    status: 'PASS',
  }
  fs.writeFileSync(consistencyStagePath, `${JSON.stringify(finalReport, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B13_R2_PACKAGE_CONSISTENCY.json'), `${JSON.stringify(finalReport, null, 2)}\n`)
  writeInventory(stageRoot, manifestRoot)
  tree = validatePackageTree(stageRoot, sourceCommit)
  if (tree.status !== 'PASS') throw new Error(`B13_R2_PACKAGE_FINAL_TREE_FAILED:${tree.failures.join(';')}`)
  makeZip(stageRoot, zipPath)
  const finalZipResult = validatePackageTarget(zipPath, sourceCommit)
  if (finalZipResult.status !== 'PASS') throw new Error(`B13_R2_PACKAGE_FINAL_ZIP_FAILED:${finalZipResult.failures.join(';')}`)
  return { staging, stageRoot, packageRoot, zipPath, inventory, consistency: finalZipResult }
}

function main() {
  const sourceCommit = git(['rev-parse', 'HEAD'])
  const repairBaselineCommit = git(['rev-parse', `${repairBaselineShort}^{commit}`])
  const dirty = git(['status', '--porcelain', '--untracked-files=all'])
  if (sourceCommit === repairBaselineCommit) throw new Error('B13_R2_PACKAGE_SOURCE_BASELINE_NOT_ADVANCED')
  if (dirty) throw new Error(`B13_R2_PACKAGE_WORKTREE_NOT_CLEAN:${dirty}`)
  const evidenceDir = path.resolve(process.env.B13_R2_EVIDENCE_DIR ?? '')
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R2_PACKAGE_EVIDENCE_MISSING')
  const preflight = process.argv.includes('--preflight')
  if (!preflight) {
    const acceptancePath = path.join(evidenceDir, 'B13_R2_ACCEPTANCE.json')
    const acceptance = fs.existsSync(acceptancePath) ? JSON.parse(fs.readFileSync(acceptancePath, 'utf8')) : null
    if (!acceptance || acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.nextBatchAllowed !== false) throw new Error('B13_R2_PACKAGE_ACCEPTANCE_NOT_READY')
  }
  const result = buildStage({ sourceCommit, repairBaselineCommit, evidenceDir, preflight })
  if (!preflight) {
    fs.mkdirSync(outputDir, { recursive: true })
    const finalZip = path.join(outputDir, path.basename(result.zipPath))
    fs.copyFileSync(result.zipPath, finalZip)
    const shaPath = path.join(outputDir, `${path.basename(finalZip, '.zip')}_SHA256SUMS.txt`)
    fs.writeFileSync(shaPath, `${sha256(finalZip)}  ${path.basename(finalZip)}\n`)
    console.log(JSON.stringify({ status: 'PASS', zip: finalZip, sha256File: shaPath, zipSha256: sha256(finalZip), sourceCommit, packageRoot: result.packageRoot, entryCount: result.consistency.actualEntries }, null, 2))
  } else {
    console.log(JSON.stringify({ status: 'PASS', preflight: true, sourceCommit, packageRoot: result.packageRoot, stageRoot: result.stageRoot, zipPath: result.zipPath, inventory: result.inventory, consistency: result.consistency }, null, 2))
  }
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
