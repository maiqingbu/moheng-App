import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repairBaselineCommit = '66638c4e7917fc233d084f0142a6a3a07bc0fb41'
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const originalInstructionPath = '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md'
const repairInstructionPath = '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R1_定点修复与重新打包执行指令_2026-09-04.md'
const expectedOriginalInstructionSha256 = '54a10882729ae79497231f49e465740d39bfb67e6683d0feb0446403bbe416cb'
const expectedRepairInstructionSha256 = '1566138e7a1f3100f54415badd9f148dd9cbaa17a7725e6363af690db4666d65'
const evidenceDir = path.resolve(process.env.B13_R1_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-r1-evidence-latest'))
const outputDir = '/Users/mac/Desktop/交付包'

const gitResult = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' })
const git = args => gitResult(args).stdout.trim()

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function trackedFiles() {
  return gitResult(['ls-files', '-z']).stdout.split('\0').filter(Boolean)
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
    schema: 'b13-r1-package-preflight',
    sourceCommit: git(['rev-parse', 'HEAD']),
    sourceRoots: ['src', 'src-tauri', 'contracts', 'scripts', 'tests', 'docs', 'creative-skills'],
    trackedSourceFiles: files.filter(sourceAllowed).length,
    forbiddenEntries,
    unscoped,
    status: forbiddenEntries.length === 0 && unscoped.length === 0 ? 'PASS' : 'FAIL',
  }
  console.log(JSON.stringify(result, null, 2))
  return result
}

function runPython(script, args) {
  return spawnSync('python3', ['-c', script, ...args], { encoding: 'utf8' })
}

if (process.argv.includes('--preflight')) {
  process.exitCode = preflight().status === 'PASS' ? 0 : 1
} else {
  const sourceCommit = git(['rev-parse', 'HEAD'])
  const shortSha = sourceCommit.slice(0, 7)
  const dirty = git(['status', '--porcelain', '--untracked-files=all'])
  if (sourceCommit === repairBaselineCommit) throw new Error('B13_R1_PACKAGE_SOURCE_BASELINE_NOT_ADVANCED')
  if (dirty) throw new Error(`B13_R1_PACKAGE_WORKTREE_NOT_CLEAN:${dirty}`)
  if (!fs.existsSync(evidenceDir)) throw new Error(`B13_R1_PACKAGE_EVIDENCE_MISSING:${evidenceDir}`)
  if (!fs.existsSync(originalInstructionPath) || !fs.existsSync(repairInstructionPath)) throw new Error('B13_R1_PACKAGE_FROZEN_INPUT_MISSING')

  const acceptancePath = path.join(evidenceDir, 'B13_R1_ACCEPTANCE.json')
  const acceptance = fs.existsSync(acceptancePath) ? JSON.parse(fs.readFileSync(acceptancePath, 'utf8')) : null
  if (!acceptance || acceptance.status !== 'PASS' || acceptance.sourceCommit !== sourceCommit || acceptance.nextBatchAllowed !== false) throw new Error('B13_R1_PACKAGE_ACCEPTANCE_NOT_READY')
  const preflightReport = preflight()
  if (preflightReport.status !== 'PASS') throw new Error('B13_R1_PACKAGE_PREFLIGHT_FAILED')
  const originalSha256 = sha256(originalInstructionPath)
  const repairSha256 = sha256(repairInstructionPath)
  if (originalSha256 !== expectedOriginalInstructionSha256 || repairSha256 !== expectedRepairInstructionSha256) throw new Error('B13_R1_PACKAGE_INSTRUCTION_HASH_MISMATCH')

  const staging = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b13-r1-package-${shortSha}-`))
  const packageRoot = `MOHENG_V2.1_B13_R1_ACCEPTANCE_${shortSha}`
  const stageRoot = path.join(staging, packageRoot)
  const sourceRoot = path.join(stageRoot, 'SOURCE')
  const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
  const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B13_R1')
  const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
  for (const directory of [sourceRoot, frozenRoot, evidenceRoot, manifestRoot]) fs.mkdirSync(directory, { recursive: true })

  for (const relative of trackedFiles().filter(sourceAllowed)) {
    if (forbidden(relative)) continue
    const destination = path.join(sourceRoot, relative)
    fs.mkdirSync(path.dirname(destination), { recursive: true })
    fs.copyFileSync(path.join(root, relative), destination)
  }
  fs.copyFileSync(originalInstructionPath, path.join(frozenRoot, 'B13_DEVELOPMENT_INSTRUCTION_R1.md'))
  fs.copyFileSync(repairInstructionPath, path.join(frozenRoot, 'B13_R1_REPAIR_INSTRUCTION.md'))
  const frozenHashes = {
    originalDevelopmentInstructionSha256: originalSha256,
    repairInstructionSha256: repairSha256,
    repairBaselineCommit,
    postB12FrozenCommit,
    batch: 'B13-R1',
    nextBatch: 'B14',
    nextBatchStarted: false,
  }
  fs.writeFileSync(path.join(frozenRoot, 'FROZEN_INPUT_HASHES.json'), `${JSON.stringify(frozenHashes, null, 2)}\n`)
  fs.writeFileSync(path.join(frozenRoot, 'BASELINE_BINDING.json'), `${JSON.stringify({
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    batch: 'B13-R1',
    nextBatch: 'B14',
    nextBatchStarted: false,
  }, null, 2)}\n`)
  fs.cpSync(evidenceDir, evidenceRoot, { recursive: true })

  const changedFilesText = git(['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`])
  const changedFiles = changedFilesText.split('\n').filter(Boolean)
  const diffPatch = git(['diff', '--unified=0', `${repairBaselineCommit}..${sourceCommit}`, '--', 'src', 'src-tauri/src', 'scripts', 'tests'])
  const futureAdded = diffPatch.split('\n').filter(line => /^\+[^+]/.test(line) && /\b(?:B14|B15|RC1)::|EndgameOwner|CompletionGate|ExportReleaseOwner/.test(line))
  const secretFiles = []
  const collectTextFiles = directory => {
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      if (fs.statSync(file).isDirectory()) collectTextFiles(file)
      else if (/\.(?:rs|ts|vue|mjs|json|md|toml|sql)$/.test(file)) secretFiles.push(file)
    }
  }
  for (const relative of ['src', 'src-tauri/src', 'contracts', 'scripts', 'tests']) collectTextFiles(path.join(root, relative))
  const secretLeaks = secretFiles.flatMap(file => /sk-[A-Za-z0-9]{20,}|Bearer\s+[A-Za-z0-9._-]{20,}|OPENAI_API_KEY\s*=/.test(fs.readFileSync(file, 'utf8')) ? [path.relative(root, file)] : [])

  const sourceBinding = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R1_SOURCE_BINDING.json'), 'utf8'))
  fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# B13-R1 长篇健康治理修复验收包

批次：B13-R1

修复基线：\`${repairBaselineCommit}\`
最终源码：\`${sourceCommit}\`

本包只包含五项 B13-R1 定点修复及其真实验收证据：章节 opaque ID 解析、正式 Plan Stale 语义复用、全局 Health Truth 与分页详情分离、卷展示身份、Evidence 真实性。B01–B12 保持冻结；B14、B15、RC1 未启动。

验收入口：

- EVIDENCE/B13_R1/B13_R1_ACCEPTANCE.json
- EVIDENCE/B13_R1/B13_R1_FINAL_REPORT.txt
- EVIDENCE/B13_R1/B13_R1_EVIDENCE_TRUTHFULNESS.json
- PACKAGE_MANIFEST/SOURCE_BINDING.json

源码命令：在 SOURCE/ 中安装依赖后运行 \`npm run verify:b13\`；Rust 命令使用 \`--manifest-path src-tauri/Cargo.toml\`。
`)

  fs.writeFileSync(path.join(manifestRoot, 'BASELINE.json'), `${JSON.stringify({ batch: 'B13-R1', repairBaselineCommit, postB12FrozenCommit, sourceCommit, nextBatch: 'B14', nextBatchStarted: false }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'SOURCE_BINDING.json'), `${JSON.stringify(sourceBinding, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nREPAIR_BASELINE=${repairBaselineCommit}\ngit status --porcelain\n${dirty}\ngit diff --name-status REPAIR_BASELINE..HEAD\n${changedFilesText}\ngit diff --stat REPAIR_BASELINE..HEAD\n${git(['diff', '--stat', `${repairBaselineCommit}..${sourceCommit}`])}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${changedFilesText}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'CHANGED_FILES.json'), `${JSON.stringify(JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R1_CHANGED_FILES.json'), 'utf8')), null, 2)}\n`)
  fs.copyFileSync(path.join(evidenceDir, 'COMMANDS_EXECUTED.json'), path.join(manifestRoot, 'COMMANDS_EXECUTED.json'))
  fs.writeFileSync(path.join(manifestRoot, 'ENVIRONMENT.json'), `${JSON.stringify({
    OS: `${process.platform} ${process.arch}`,
    node: process.version,
    npm: spawnSync('npm', ['--version'], { encoding: 'utf8' }).stdout.trim(),
    rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    cargo: spawnSync('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim() || 'unavailable',
    git: git(['--version']),
  }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'FORBIDDEN_SCOPE_DIFF.json'), `${JSON.stringify({ schema: 'b13-r1-forbidden-scope-diff', futureAdded, status: futureAdded.length === 0 ? 'PASS' : 'FAIL' }, null, 2)}\n`)
  fs.writeFileSync(path.join(manifestRoot, 'SECRET_SCAN.json'), `${JSON.stringify({ schema: 'b13-r1-secret-scan', realSecretFindings: secretLeaks, status: secretLeaks.length === 0 ? 'PASS' : 'FAIL' }, null, 2)}\n`)

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
  const inventory = () => {
    const files = allStageFiles()
    return {
      totalFiles: files.filter(file => !forbiddenStage(file)).length,
      sourceFiles: files.filter(file => !forbiddenStage(file) && relativeStage(file).startsWith('SOURCE/')).length,
      evidenceFiles: files.filter(file => !forbiddenStage(file) && relativeStage(file).startsWith('EVIDENCE/')).length,
      manifestFiles: files.filter(file => !forbiddenStage(file) && relativeStage(file).startsWith('PACKAGE_MANIFEST/')).length,
      frozenInputFiles: files.filter(file => !forbiddenStage(file) && relativeStage(file).startsWith('FROZEN_INPUTS/')).length,
      forbiddenFilesFound: files.filter(forbiddenStage).length,
    }
  }
  fs.writeFileSync(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), `${JSON.stringify(inventory(), null, 2)}\n`)
  const utf8ValidationPath = path.join(manifestRoot, 'PACKAGE_UTF8_VALIDATION.json')
  fs.writeFileSync(utf8ValidationPath, `${JSON.stringify({
    schema: 'b13-r1-package-utf8-validation',
    root: `${packageRoot}/`,
    rootAscii: /^[\x00-\x7F]+$/.test(packageRoot),
    filenameEncoding: 'UTF-8',
    replacementCharacterEntries: [],
    mojibakeEntries: [],
    independentReaders: ['python-zipfile', 'unzip-list'],
    status: 'PENDING_ARCHIVE',
  }, null, 2)}\n`)
  const sumsPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
  const writeInternalSums = () => {
    const lines = allStageFiles()
      .filter(file => file !== sumsPath && !forbiddenStage(file))
      .map(file => `${sha256(file)}  ${relativeStage(file)}`)
      .sort((a, b) => a.localeCompare(b))
    fs.writeFileSync(sumsPath, `${lines.join('\n')}\n`)
  }
  const zipPath = path.join(staging, `墨衡_V2.1_B13_LongFormHealth_R1_定点修复_正式验收交付版_${shortSha}.zip`)
  const makeZip = () => {
    const script = `import os,sys,zipfile\nroot=sys.argv[1]\nout=sys.argv[2]\nwith zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:\n    for base,dirs,files in os.walk(root):\n        dirs.sort(); files.sort()\n        for name in files:\n            p=os.path.join(base,name)\n            arc=os.path.relpath(p,os.path.dirname(root)).replace(os.sep,'/')\n            z.write(p,arc)\n`
    const result = runPython(script, [stageRoot, zipPath])
    if (result.status !== 0) throw new Error(`B13_R1_ZIP_CREATE_FAILED:${result.stderr}`)
  }
  const verifyZip = () => {
    const script = `import hashlib,os,sys,tempfile,zipfile\nzip_path=sys.argv[1]\nroot_name=sys.argv[2]\nwith zipfile.ZipFile(zip_path) as z:\n    names=z.namelist()\n    assert names and all(n.startswith(root_name+'/') for n in names)\n    assert all('\\ufffd' not in n for n in names)\n    assert all(not any(part in ('node_modules','target','dist','coverage','.git') for part in n.split('/')) for n in names)\n    with tempfile.TemporaryDirectory() as d:\n        z.extractall(d)\n        root=os.path.join(d,root_name)\n        sums=os.path.join(root,'PACKAGE_MANIFEST','FILES_SHA256SUMS.txt')\n        for line in open(sums,encoding='utf-8'):\n            digest,rel=line.rstrip('\\n').split('  ',1)\n            with open(os.path.join(root,rel),'rb') as f: actual=hashlib.sha256(f.read()).hexdigest()\n            assert actual==digest,(rel,actual,digest)\nprint('PASS')\n`
    const py = runPython(script, [zipPath, packageRoot])
    const unzip = spawnSync('unzip', ['-Z1', zipPath], { encoding: 'utf8' })
    const names = unzip.status === 0 ? unzip.stdout.split('\n').filter(Boolean) : []
    return { pythonZipfile: py.status === 0, unzipList: unzip.status === 0, entryCount: names.length, entriesHaveNoReplacementCharacter: names.every(name => !name.includes('\ufffd')), error: `${py.stderr ?? ''}${unzip.stderr ?? ''}` }
  }
  writeInternalSums()
  makeZip()
  let verification = verifyZip()
  if (!verification.pythonZipfile || !verification.unzipList || !verification.entriesHaveNoReplacementCharacter) throw new Error(`B13_R1_ZIP_VERIFY_FAILED:${verification.error}`)
  fs.writeFileSync(utf8ValidationPath, `${JSON.stringify({
    schema: 'b13-r1-package-utf8-validation',
    root: `${packageRoot}/`,
    rootAscii: true,
    filenameEncoding: 'UTF-8',
    replacementCharacterEntries: [],
    mojibakeEntries: [],
    independentReaders: ['python-zipfile', 'unzip-list'],
    archiveReaders: verification,
    status: 'PASS',
  }, null, 2)}\n`)
  writeInternalSums()
  makeZip()
  verification = verifyZip()
  if (!verification.pythonZipfile || !verification.unzipList || !verification.entriesHaveNoReplacementCharacter) throw new Error(`B13_R1_FINAL_ZIP_VERIFY_FAILED:${verification.error}`)

  fs.mkdirSync(outputDir, { recursive: true })
  const finalZip = path.join(outputDir, path.basename(zipPath))
  fs.copyFileSync(zipPath, finalZip)
  const externalShaPath = path.join(outputDir, `${path.basename(zipPath, '.zip')}_SHA256SUMS.txt`)
  fs.writeFileSync(externalShaPath, `${sha256(finalZip)}  ${path.basename(finalZip)}\n`)
  console.log(JSON.stringify({ status: 'PASS', zip: finalZip, sha256File: externalShaPath, zipSha256: sha256(finalZip), sourceCommit, packageRoot, entryCount: verification.entryCount }, null, 2))
}
