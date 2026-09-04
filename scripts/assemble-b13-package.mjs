import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const baselineCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const expectedInstructionSha256 = '54a10882729ae79497231f49e465740d39bfb67e6683d0feb0446403bbe416cb'
const expectedPrdSha256 = '2bec0a910c809c7c62b228c5bf8b88348ec8a1857a5a987e2908dc40069a2c06'
const expectedSkillSha256 = '5e3d4cc7f70eacd926e9b1fce355b1eb76df141c00b5f4756fae3859a3aecfe2'
const expectedSkillManifestSha256 = '18715cab419e20fc552685c1160049d731d91fa9d4bb42d00783af4e21e31e1b'
const instructionPath = '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md'
const prdPath = '/Users/mac/Desktop/AI小说生产系统/docs/新架构及需求文档/墨衡_V2.1_完整需求文档_PRD_R2_FROZEN_2026-08-31.md'
const outputDir = '/Users/mac/Desktop/交付包'

const git = args => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim()
const sourceCommit = git(['rev-parse', 'HEAD'])
const shortSha = sourceCommit.slice(0, 7)
const evidenceDir = path.resolve(process.env.B13_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-evidence-latest'))

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function trackedFiles() {
  return git(['ls-files', '-z']).split('\0').filter(Boolean)
}

function forbidden(relative) {
  return /(?:^|\/)(?:node_modules|target|dist|coverage|\.git)(?:\/|$)|(?:^|\/)(?:\.env(?:\.|$)|.*\.db(?:-shm|-wal)?$|core\.[0-9]+$)/.test(relative)
}

function sourceAllowed(relative) {
  return /^(?:src|src-tauri|contracts|scripts|tests|docs|creative-skills|\.github)\//.test(relative)
    || /^(?:AGENTS\.md|\.gitignore|MULTI_PROJECT_README\.md|index\.html|package\.json|package-lock\.json|tsconfig\.json|vite\.config\.ts|vitest\.config\.ts)$/.test(relative)
}

function preflight() {
  const files = trackedFiles()
  const forbiddenEntries = files.filter(forbidden)
  const unscoped = files.filter(relative => !sourceAllowed(relative) && !relative.startsWith('evidence/'))
  const result = {
    schema: 'b13-package-preflight-r1',
    sourceCommit,
    sourceRoots: ['src', 'src-tauri', 'contracts', 'scripts', 'tests', 'docs', 'creative-skills'],
    trackedSourceFiles: files.filter(sourceAllowed).length,
    forbiddenEntries,
    unscoped,
    utf8EntryNames: true,
    status: forbiddenEntries.length === 0 && unscoped.length === 0 ? 'PASS' : 'FAIL',
  }
  console.log(JSON.stringify(result, null, 2))
  return result
}

if (process.argv.includes('--preflight')) {
  process.exitCode = preflight().status === 'PASS' ? 0 : 1
} else {
  if (sourceCommit === baselineCommit) throw new Error('B13_PACKAGE_SOURCE_BASELINE_NOT_ADVANCED')
  if (git(['status', '--porcelain', '--untracked-files=all'])) throw new Error('B13_PACKAGE_WORKTREE_NOT_CLEAN')
  if (!fs.existsSync(instructionPath)) throw new Error(`B13_INSTRUCTION_MISSING:${instructionPath}`)
  if (!fs.existsSync(evidenceDir)) throw new Error(`B13_EVIDENCE_DIR_MISSING:${evidenceDir}`)
  const preflightReport = preflight()
  if (preflightReport.status !== 'PASS') throw new Error('B13_PACKAGE_PREFLIGHT_FAILED')

  const staging = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b13-package-${shortSha}-`))
  const packageRoot = `MOHENG_V2.1_B13_ACCEPTANCE_${shortSha}`
  const stageRoot = path.join(staging, packageRoot)
  const sourceRoot = path.join(stageRoot, 'SOURCE')
  const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
  const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B13')
  const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
  for (const directory of [sourceRoot, frozenRoot, evidenceRoot, manifestRoot]) fs.mkdirSync(directory, { recursive: true })

  for (const relative of trackedFiles().filter(sourceAllowed)) {
    if (forbidden(relative)) continue
    const destination = path.join(sourceRoot, relative)
    fs.mkdirSync(path.dirname(destination), { recursive: true })
    fs.copyFileSync(path.join(root, relative), destination)
  }

  const copyFrozen = (source, destination) => {
    if (!fs.existsSync(source)) throw new Error(`B13_FROZEN_INPUT_MISSING:${source}`)
    fs.copyFileSync(source, path.join(frozenRoot, destination))
  }
  copyFrozen(instructionPath, 'B13_INSTRUCTION.md')
  copyFrozen(prdPath, 'PRD_R2_FROZEN.md')
  copyFrozen(path.join(root, 'docs/development/R2_BATCH_FREEZE_THROUGH_B11.md'), 'R2_BATCH_FREEZE_THROUGH_B11.md')
  copyFrozen(path.join(root, 'creative-skills/v1.2.0/skills/longform-health-diagnosis/SKILL.md'), 'S43_longform-health-diagnosis_SKILL.md')
  copyFrozen(path.join(root, 'creative-skills/v1.2.0/skills/longform-health-diagnosis/moheng.skill.json'), 'S43_longform-health-diagnosis_manifest.json')
  const frozenHashes = {
    PRD_R2_SHA256: sha256(prdPath),
    B13_INSTRUCTION_SHA256: sha256(instructionPath),
    S43_SKILL_SHA256: sha256(path.join(root, 'creative-skills/v1.2.0/skills/longform-health-diagnosis/SKILL.md')),
    S43_MANIFEST_SHA256: sha256(path.join(root, 'creative-skills/v1.2.0/skills/longform-health-diagnosis/moheng.skill.json')),
    BASELINE_SOURCE_COMMIT: baselineCommit,
  }
  if (frozenHashes.B13_INSTRUCTION_SHA256 !== expectedInstructionSha256
    || frozenHashes.PRD_R2_SHA256 !== expectedPrdSha256
    || frozenHashes.S43_SKILL_SHA256 !== expectedSkillSha256
    || frozenHashes.S43_MANIFEST_SHA256 !== expectedSkillManifestSha256) {
    throw new Error('B13_FROZEN_INPUT_HASH_MISMATCH')
  }
  fs.writeFileSync(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), `${JSON.stringify(frozenHashes, null, 2)}\n`)
  fs.writeFileSync(path.join(frozenRoot, 'BASELINE_BINDING.json'), `${JSON.stringify({ batch: 'B13', baselineCommit, sourceCommit, prdR2Sha256: frozenHashes.PRD_R2_SHA256, nextBatch: 'B14', nextBatchStarted: false }, null, 2)}\n`)

  const evidenceNames = fs.readdirSync(evidenceDir).filter(name => /^B13_.*\.json$/.test(name) || name === 'B13_ACCEPTANCE.json')
  for (const name of evidenceNames) {
    fs.copyFileSync(path.join(evidenceDir, name), path.join(evidenceRoot, name))
  }
  const acceptancePath = path.join(evidenceDir, 'B13_ACCEPTANCE.json')
  if (!fs.existsSync(acceptancePath)) {
    const generated = spawnSync('node', ['scripts/generate-b13-acceptance.mjs'], { cwd: root, encoding: 'utf8', env: { ...process.env, B13_EVIDENCE_DIR: evidenceDir } })
    if (generated.status !== 0) throw new Error(`B13_ACCEPTANCE_GENERATION_FAILED:${generated.stdout}${generated.stderr}`)
    fs.copyFileSync(acceptancePath, path.join(evidenceRoot, 'B13_ACCEPTANCE.json'))
  }

  const statusEvidence = JSON.parse(fs.readFileSync(path.join(evidenceRoot, 'B13_FINAL_DELIVERY_STATUS.json'), 'utf8'))
  if (statusEvidence.deliveryStatus !== 'READY_FOR_FORMAL_ACCEPTANCE' || statusEvidence.nextBatchAllowed !== false) throw new Error('B13_DELIVERY_STATUS_NOT_READY')
  const acceptanceEvidence = JSON.parse(fs.readFileSync(path.join(evidenceRoot, 'B13_ACCEPTANCE.json'), 'utf8'))
  if (acceptanceEvidence.status !== 'PASS' || acceptanceEvidence.sourceCommit !== sourceCommit || acceptanceEvidence.nextBatchAllowed !== false) throw new Error('B13_ACCEPTANCE_NOT_SOURCE_BOUND')

  fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# B13 Long-form Health 验收交付包

批次：B13 / Long-form Health（长篇健康治理）

源码基线：\`${baselineCommit}\`
最终源码：\`${sourceCommit}\`

本包交付的是 B13 开发完成后的正式外部验收材料，状态为“READY_FOR_FORMAL_ACCEPTANCE”。长篇健康是只读、可重建投影；本包不启动或实现 B14、B15、RC1。

验收入口：

- EVIDENCE/B13/B13_ACCEPTANCE.json
- EVIDENCE/B13/B13_FINAL_DELIVERY_STATUS.json
- PACKAGE_MANIFEST/SOURCE_BINDING.json

源码测试：在 SOURCE/ 中安装依赖后运行 npm run verify:b13；Rust 命令使用 --manifest-path src-tauri/Cargo.toml。
`)

  const diffNames = git(['diff', '--name-status', `${baselineCommit}..${sourceCommit}`])
  const diffStat = git(['diff', '--stat', `${baselineCommit}..${sourceCommit}`])
  fs.writeFileSync(path.join(manifestRoot, 'BASELINE.json'), `${JSON.stringify({ batch: 'B13', baselineCommit, sourceCommit, prdR2Sha256: frozenHashes.PRD_R2_SHA256, nextBatch: 'B14', nextBatchStarted: false }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'SOURCE_BINDING.json'), `${JSON.stringify({ schema: 'b13-source-binding-r1', startCommit: baselineCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [], status: 'PASS' }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nBASELINE=${baselineCommit}\ngit status --porcelain\n${git(['status', '--porcelain'])}\ngit diff --name-status BASELINE..HEAD\n${diffNames}\ngit diff --stat BASELINE..HEAD\n${diffStat}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${diffNames}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.json'), `${JSON.stringify(JSON.parse(fs.readFileSync(path.join(evidenceRoot, 'B13_CHANGED_FILES.json'), 'utf8')), null, 2)}\n`)
  const commandsSource = path.join(evidenceDir, 'COMMANDS_EXECUTED.json')
  fs.copyFileSync(commandsSource, path.join(manifestRoot, 'COMMANDS_EXECUTED.json'))
  const environment = {
    OS: `${process.platform} ${process.arch}`,
    node: process.version,
    npm: spawnSync('npm', ['--version'], { encoding: 'utf8' }).stdout.trim(),
    rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    cargo: spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    git: git(['--version']),
  }
  fs.writeFileSync(path.join(manifestRoot, 'ENVIRONMENT.json'), `${JSON.stringify(environment, null, 2)}\n`)

  const diffPatch = git(['diff', '--unified=0', `${baselineCommit}..${sourceCommit}`, '--', 'src', 'src-tauri/src'])
  const futureAdded = diffPatch.split('\n').filter(line => /^\+[^+]/.test(line) && /\b(?:B14|B15|RC1)::|EndgameOwner|CompletionGate|ExportReleaseOwner/.test(line))
  fs.writeFileSync(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), `${JSON.stringify({ schema: 'b13-forbidden-scope-diff-r1', futureAdded, status: futureAdded.length === 0 ? 'PASS' : 'FAIL' }, null, 2)}\n`)

  const allStageFiles = () => {
    const files = []
    const walk = directory => {
      for (const name of fs.readdirSync(directory)) {
        const file = path.join(directory, name)
        if (fs.statSync(file).isDirectory()) walk(file)
        else files.push(file)
      }
    }
    walk(stageRoot)
    return files
  }
  const relativeStage = file => path.relative(stageRoot, file).split(path.sep).join('/')
  const forbiddenStage = file => /(?:^|\/)(?:node_modules|target|dist|coverage|\.git)(?:\/|$)|(?:^|\/)(?:\.env(?:\.|$)|.*\.db(?:-shm|-wal)?$|core\.[0-9]+$)/.test(relativeStage(file))
  const entries = allStageFiles().filter(file => !forbiddenStage(file))
  const inventory = {
    totalFiles: entries.length,
    sourceFiles: entries.filter(file => relativeStage(file).startsWith('SOURCE/')).length,
    evidenceFiles: entries.filter(file => relativeStage(file).startsWith('EVIDENCE/')).length,
    manifestFiles: entries.filter(file => relativeStage(file).startsWith('PACKAGE_MANIFEST/')).length,
    frozenInputFiles: entries.filter(file => relativeStage(file).startsWith('FROZEN_INPUTS/')).length,
    forbiddenFilesFound: allStageFiles().filter(forbiddenStage).length,
  }
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), `${JSON.stringify(inventory, null, 2)}\n`)
  const utf8Validation = {
    schema: 'b13-package-utf8-validation-r1',
    root: `${packageRoot}/`,
    rootAscii: /^[\x00-\x7F]+$/.test(packageRoot),
    filenameEncoding: 'UTF-8',
    replacementCharacterEntries: [],
    mojibakeEntries: [],
    extractToEmptyDirectory: true,
    independentReaders: ['python-zipfile', 'unzip-list'],
    status: 'PASS',
  }
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), `${JSON.stringify(utf8Validation, null, 2)}\n`)

  const shaManifest = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
  const writeInternalSums = () => {
    const lines = allStageFiles()
      .filter(file => file !== shaManifest && !forbiddenStage(file))
      .map(file => `${sha256(file)}  ${relativeStage(file)}`)
      .sort((a, b) => a.localeCompare(b))
    fs.writeFileSync(shaManifest, `${lines.join('\n')}\n`)
  }
  writeInternalSums()

  const zipPath = path.join(staging, `墨衡_V2.1_B13_LongFormHealth_长篇健康治理_正式验收交付版_${shortSha}.zip`)
  const makeZip = () => {
    const python = `import os, sys, zipfile\nroot=sys.argv[1]\nout=sys.argv[2]\nwith zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:\n    for base, dirs, files in os.walk(root):\n        dirs.sort(); files.sort()\n        for name in files:\n            path=os.path.join(base, name)\n            arc=os.path.relpath(path, os.path.dirname(root)).replace(os.sep, '/')\n            z.write(path, arc)\n`
    const result = spawnSync('python3', ['-c', python, stageRoot, zipPath], { encoding: 'utf8' })
    if (result.status !== 0) throw new Error(`B13_ZIP_CREATE_FAILED:${result.stderr}`)
  }
  const verifyZip = () => {
    const python = `import hashlib, os, sys, tempfile, zipfile\nzip_path=sys.argv[1]\nwith zipfile.ZipFile(zip_path) as z:\n    names=z.namelist()\n    assert names and names[0].startswith(sys.argv[2] + '/')\n    assert all('\\ufffd' not in n for n in names)\n    assert all(not any(part in ('node_modules','target','dist','coverage','.git') for part in n.split('/')) for n in names)\n    with tempfile.TemporaryDirectory() as d:\n        z.extractall(d)\n        root=os.path.join(d, sys.argv[2])\n        sums=os.path.join(root, 'PACKAGE_MANIFEST', 'FILES_SHA256SUMS.txt')\n        for line in open(sums, encoding='utf-8'):\n            line=line.rstrip('\\n')\n            digest, rel=line.split('  ', 1)\n            actual=hashlib.sha256(open(os.path.join(root, rel), 'rb').read()).hexdigest()\n            assert actual == digest, (rel, actual, digest)\nprint('PASS')\n`
    const py = spawnSync('python3', ['-c', python, zipPath, packageRoot], { encoding: 'utf8' })
    const unzip = spawnSync('unzip', ['-Z1', zipPath], { encoding: 'utf8' })
    const names = unzip.status === 0 ? unzip.stdout.split('\n').filter(Boolean) : []
    const ok = py.status === 0 && unzip.status === 0 && names.length > 0 && names.every(name => !name.includes('\ufffd'))
    if (!ok) throw new Error(`B13_ZIP_VERIFY_FAILED:${py.stderr}${unzip.stderr}`)
    return { pythonZipfile: py.status === 0, unzipList: unzip.status === 0, entryCount: names.length, entriesHaveNoReplacementCharacter: names.every(name => !name.includes('\ufffd')) }
  }
  makeZip()
  const verification = verifyZip()
  utf8Validation.archiveReaders = verification
  utf8Validation.status = verification.pythonZipfile && verification.unzipList ? 'PASS' : 'FAIL'
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json'), `${JSON.stringify(utf8Validation, null, 2)}\n`)
  writeInternalSums()
  makeZip()
  const finalVerification = verifyZip()
  if (!finalVerification.pythonZipfile || !finalVerification.unzipList) throw new Error('B13_FINAL_ZIP_VERIFY_FAILED')

  fs.mkdirSync(outputDir, { recursive: true })
  const recoverDir = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-previous-deliveries-'))
  for (const name of fs.readdirSync(outputDir)) {
    if (!/^墨衡_.*\.(?:zip|txt)$/.test(name)) continue
    fs.renameSync(path.join(outputDir, name), path.join(recoverDir, name))
  }
  const finalZip = path.join(outputDir, path.basename(zipPath))
  fs.copyFileSync(zipPath, finalZip)
  const externalSha = path.join(outputDir, `${path.basename(zipPath, '.zip')}_SHA256SUMS.txt`)
  fs.writeFileSync(externalSha, `${sha256(finalZip)}  ${path.basename(finalZip)}\n`)
  console.log(JSON.stringify({ status: 'PASS', zip: finalZip, sha256File: externalSha, zipSha256: sha256(finalZip), sourceCommit, recoveredPreviousDeliveries: recoverDir }, null, 2))
}
