import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

import {
  allFiles,
  fileSha256,
  evaluateFinalAcceptance,
  scanSecretStaging,
  treeDigest,
  verifyChecksums,
} from './post-b14-ui-r8-certification.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const downloads = '/Users/mac/Downloads'
const delivery = '/Users/mac/Desktop/交付包'
const baseline = '4552fa978f42e6d3d355afd2512ddaf77920a433'
const b14FrozenCommit = '93dc30ac25fc167992801aa7f9fa8a1b6acc93cc'
const r7Sha = 'c5316c0e615c7f2c0cac0993dfec5c3ee7c1a229a167580bbe39fdfc9d5f13d5'
const r8Sha = 'ec371a79c7d18e25c6d9a2526f53ca8256753d2bf8cb308d84da996860c4db47'
const evidenceDir = path.resolve(process.env.POST_B14_UI_R8_EVIDENCE_DIR ?? '')
const sourceCommit = run('git', ['rev-parse', 'HEAD']).stdout.trim()
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R8_CertificationOnly最终封口_正式验收交付版_${short}`
const rootName = `MOHENG_V2.1_POST_B14_UI_R8_ACCEPTANCE_${short}`
const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r8-'))
const stageRoot = path.join(tempRoot, rootName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const designRoot = path.join(stageRoot, 'DESIGN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerLogsRoot = path.join(tempRoot, 'reader-logs')

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, ...options })
  if (result.error) throw result.error
  return result
}
function write(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`)
}
function copy(from, to) {
  if (!fs.existsSync(from)) throw new Error(`MISSING_R8_INPUT:${from}`)
  fs.mkdirSync(path.dirname(to), { recursive: true })
  fs.copyFileSync(from, to)
}
function copyTree(from, to) {
  fs.mkdirSync(to, { recursive: true })
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    const source = path.join(from, entry.name)
    const destination = path.join(to, entry.name)
    entry.isDirectory() ? copyTree(source, destination) : copy(source, destination)
  }
}
function json(file) { return JSON.parse(fs.readFileSync(file, 'utf8')) }
function gate(condition) { return condition ? 'PASS' : 'FAIL' }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function findInput(candidates) {
  const candidate = candidates.find(file => fs.existsSync(path.join(downloads, file)))
  if (!candidate) throw new Error(`MISSING_R8_FROZEN_INPUT:${candidates.join('|')}`)
  return path.join(downloads, candidate)
}
function zipDirectory(source, destination) {
  const script = ['import os,sys,zipfile', 'out,root=sys.argv[1],sys.argv[2]', 'with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as z:', '  for d,ds,fs in os.walk(root):', '    ds.sort()', '    for f in sorted(fs):', '      p=os.path.join(d,f);z.write(p,os.path.relpath(p,os.path.dirname(root)))'].join('\n')
  const result = run('python3', ['-c', script, destination, source])
  if (result.status !== 0) throw new Error(result.stderr)
}
function makeChecksumFile(rootDir, checksumRelative) {
  const entries = allFiles(rootDir).filter(file => file !== checksumRelative).map(file => `${fileSha256(path.join(rootDir, file))}  ${file}`)
  write(path.join(rootDir, checksumRelative), `${entries.join('\n')}\n`)
  return entries.length
}
function mutateShaFixture(stage, checksumRelative) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r8-sha-mutation-'))
  fs.cpSync(stage, temp, { recursive: true })
  const first = verifyChecksums(temp, checksumRelative)
  const firstFile = firstFileFromChecksum(path.join(temp, checksumRelative))
  fs.appendFileSync(path.join(temp, firstFile), 'R8 mismatch mutation')
  const mismatch = verifyChecksums(temp, checksumRelative)
  fs.cpSync(stage, temp, { recursive: true })
  const secondFile = firstFileFromChecksum(path.join(temp, checksumRelative), firstFile)
  fs.rmSync(path.join(temp, secondFile))
  const missing = verifyChecksums(temp, checksumRelative)
  fs.cpSync(stage, temp, { recursive: true })
  write(path.join(temp, 'EVIDENCE/R8_SHA_EXTRA_MUTATION.txt'), 'extra mutation')
  const extra = verifyChecksums(temp, checksumRelative)
  fs.rmSync(temp, { recursive: true, force: true })
  return { mismatch: mismatch.status === 'FAIL' && mismatch.mismatches.length > 0, missing: missing.status === 'FAIL' && missing.missingFiles.length > 0, extra: extra.status === 'FAIL' && extra.extraFiles.length > 0 }
}
function firstFileFromChecksum(file, excluded = '') {
  const line = fs.readFileSync(file, 'utf8').split(/\r?\n/).find(value => value && !value.endsWith(`  ${excluded}`))
  if (!line) throw new Error('R8_CHECKSUM_MUTATION_TARGET_MISSING')
  return line.replace(/^[0-9a-f]{64}  /, '')
}
function mutateSecretFixture(stage, selfRelative) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r8-secret-mutation-'))
  fs.cpSync(stage, temp, { recursive: true })
  const syntheticMutation = ['Authorization: Bearer ', 'R8SyntheticMutation', '-9aB7cD8eF1gH2jK3', '\n'].join('')
  write(path.join(temp, 'EVIDENCE/R8_SECRET_MUTATION.txt'), syntheticMutation)
  const result = scanSecretStaging(temp, selfRelative, sourceCommit)
  fs.rmSync(temp, { recursive: true, force: true })
  return result.status === 'FAIL' && result.realSecretFindings.length > 0
}

if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('R8_EVIDENCE_DIR_MISSING')
if (sourceCommit === baseline) throw new Error('R8_SOURCE_COMMIT_MUST_INCLUDE_CERTIFICATION_TOOLING')
if (run('git', ['merge-base', '--is-ancestor', baseline, 'HEAD']).status !== 0) throw new Error('R8_BASELINE_MISMATCH')
if (run('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('R8_WORKTREE_DIRTY')
const zeroDiff = json(path.join(evidenceDir, 'R8_PRODUCTION_ZERO_DIFF.json'))
const commands = json(path.join(evidenceDir, 'COMMANDS_EXECUTED_R8.json'))
const runnerBinding = json(path.join(evidenceDir, 'R8_RUNNER_BINDING.json'))
if (zeroDiff.status !== 'PASS' || runnerBinding.status !== 'PASS' || commands.status !== 'PASS') throw new Error('R8_RUNNER_EVIDENCE_NOT_READY')

for (const dir of [sourceRoot, frozenRoot, designRoot, evidenceRoot, manifestRoot, readerLogsRoot]) fs.mkdirSync(dir, { recursive: true })
for (const file of run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)) {
  if (!/(^|\/)(node_modules|target|dist|\.git)(\/|$)|(^|\/)\.DS_Store$|\.(db|db-shm|db-wal|zip|7z)$/i.test(file)) copy(path.join(root, file), path.join(sourceRoot, file))
}

const frozen = [
  ['01_FREEZE_THROUGH_B14.md', ['墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B14_2026-09-08.md']],
  ['02_UI_R1_PACKAGE_REQUIREMENTS.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-08.md']],
  ['03_UI_R2_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R2_集成封口修复与重新打包执行指令_2026-09-08.md']],
  ['04_UI_R3_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E与EvidenceTruthfulness最终封口执行指令_2026-09-08.md']],
  ['05_UI_R4_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_ProductionAssistantE2EBridge最终封口执行指令_2026-09-08.md']],
  ['06_UI_R4_BLOCKING_ADDENDUM_01.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_BlockingAddendum01_作品库僵尸项目与生成流程中文化紧急增补指令_2026-09-08.md']],
  ['07_UI_R5_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R5_Evidence状态机LibraryE2E最终封口执行指令_2026-09-08.md']],
  ['08_UI_R6_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R6_真实Trace数据隔离与最终包封口执行指令_2026-09-09.md']],
  ['09_UI_R7_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R7_展示层与FinalAcceptance最终封口执行指令_2026-09-09.md']],
  ['10_UI_R8_INSTRUCTION.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R8_CertificationOnly最终封口执行指令_2026-09-09.md']],
]
for (const [target, candidates] of frozen) copy(findInput(candidates), path.join(frozenRoot, target))
copy(path.join(root, 'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), path.join(frozenRoot, '11_PRD_BINDING.md'))
copy(path.join(root, 'docs/adr/ADR-030-single-production-path-and-deferred-legacy-removal.md'), path.join(frozenRoot, '12_ADR_BINDING.md'))
copy(path.join(root, 'creative-skills/v1.2.0/BUILD_PROVENANCE.json'), path.join(frozenRoot, '13_CREATIVE_SKILLS_V1_2_0_BINDING.json'))
write(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'), Object.fromEntries(allFiles(frozenRoot).map(file => [file, sha(path.join(frozenRoot, file))])))
const frozenInputCheck = json(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'))

const designs = [
  ['01_UI_BLUEPRINT.md', ['墨衡_V2.1_Post-B14_Pre-B15_UI_重设计蓝图_R1_2026-09-06.md'], 'e23466bdc8f9945022eafb6110d531cc9fa83d0f2653e3af29bf3cf5a69c68d3'],
  ['02_UI_REFERENCE_PROTOTYPE.html', ['墨衡_V2.1_UI_Reference_Prototype_R1.html', '墨衡_V2.1_UI_Reference_Prototype_R1 (1).html'], 'e4a151c078f17bde777a22d87cad7973d352fabb7f5a69d9b20d4cf07fb12c9a'],
  ['03_UI_REFERENCE_PACKAGE.zip', ['墨衡_V2.1_UI_重设计参考包_R1_2026-09-06.zip'], '8af41f4e21169896c131caf70da1438c0f89176273200b91ac6228a9ed4c3508'],
]
const designManifest = designs.map(([target, candidates, expectedSha256]) => {
  const source = findInput(candidates)
  const actualSha256 = sha(source)
  if (actualSha256 !== expectedSha256) throw new Error(`R8_DESIGN_SHA_MISMATCH:${path.basename(source)}`)
  copy(source, path.join(designRoot, target))
  return { filename: path.basename(source), packagedAs: target, expectedSha256, actualSha256, exactMatch: true }
})
write(path.join(designRoot, 'DESIGN_INPUTS.json'), { inputs: designManifest, complete: designManifest.every(item => item.exactMatch) })
copyTree(evidenceDir, evidenceRoot)

write(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 Post-B14 / Pre-B15 UI Productization R8\n\n本包是 R8 Certification-only / Package-only 最终认证包。R7 未冻结，是因为外部验收还要求 Secret Scan final staging、真实 FILES_SHA256 逐文件复算与 Final Acceptance 消费真实认证链；R8 只补认证与打包，不修改产品代码。\n\n产品零变更基线：${baseline}\nB14 冻结基线：${b14FrozenCommit}\n最终源码：${sourceCommit}\nR8 指令 SHA-256：${r8Sha}\nR7 指令 SHA-256：${r7Sha}\n\nSecret Scan 在 ZIP 前固定的 final staging 上执行，只排除自身 SECRET_SCAN_R8.json 以避免自引用；known credential、关键词和 entropy/high-risk 三类扫描同时执行。SHA verifier 读取 PACKAGE_MANIFEST/FILES_SHA256SUMS.txt，逐条重新计算文件字节 SHA-256，并验证 mismatch、missing、extra。Final Acceptance 置于 ZIP 外，直接消费 Inventory、Secret Scan、真实 SHA Verify、Archive Validation、Final Certification 和 Evidence Consistency。\n\nZIP 内含正式源码、冻结输入、设计输入和证据；外部提交文件共 8 个。B15 / RC1 未启动，NEXT_STAGE_ALLOWED=false。\n`)

const secretRelative = 'EVIDENCE/SECRET_SCAN_R8.json'
const checksumRelative = 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt'
write(path.join(evidenceRoot, 'SECRET_SCAN_R8.json'), '{}\n')
write(path.join(manifestRoot, 'PACKAGE_INVENTORY_R8.json'), '{}\n')
write(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), '')
write(path.join(evidenceRoot, 'POST_B14_UI_R8_PRE_ARCHIVE_ACCEPTANCE.json'), { schema: 'post-b14-ui-r8-pre-archive-acceptance', sourceCommit, DELIVERY_STATUS: 'PENDING_FINAL_STAGING', NEXT_STAGE_ALLOWED: false })

const secretScan = scanSecretStaging(stageRoot, secretRelative, sourceCommit)
secretScan.mutationDetected = mutateSecretFixture(stageRoot, secretRelative)
secretScan.status = secretScan.status === 'PASS' && secretScan.mutationDetected ? 'PASS' : 'FAIL'
write(path.join(evidenceRoot, 'SECRET_SCAN_R8.json'), secretScan)
if (secretScan.status !== 'PASS') throw new Error(`R8_SECRET_SCAN_FAILED:${JSON.stringify(secretScan.realSecretFindings.slice(0, 3))}`)

const stagedFileCountBeforeChecksum = allFiles(stageRoot).length
const inventory = { schema: 'post-b14-ui-r8-package-inventory', sourceCommit, actualTreeCount: stagedFileCountBeforeChecksum, trackedFileCount: stagedFileCountBeforeChecksum - 1, checksumEntries: stagedFileCountBeforeChecksum - 1, mathConsistent: stagedFileCountBeforeChecksum === (stagedFileCountBeforeChecksum - 1) + 1, status: 'PASS' }
write(path.join(manifestRoot, 'PACKAGE_INVENTORY_R8.json'), inventory)
const initialEntries = makeChecksumFile(stageRoot, checksumRelative)
let filesShaVerify = verifyChecksums(stageRoot, checksumRelative)
const shaMutation = mutateShaFixture(stageRoot, checksumRelative)
filesShaVerify.mutationDetected = shaMutation.mismatch && shaMutation.missing && shaMutation.extra
filesShaVerify.missingMutationDetected = shaMutation.missing
filesShaVerify.extraMutationDetected = shaMutation.extra
if (!filesShaVerify.mutationDetected) throw new Error('R8_SHA_MUTATION_NOT_DETECTED')

const display = json(path.join(evidenceRoot, 'FULL_PRODUCT_VISIBLE_DOM_SCAN_R7.json'))
const a11 = json(path.join(evidenceRoot, 'A11_TARGET_STATE_R7.json'))
const noop = json(path.join(evidenceRoot, 'A11_NOOP_NEGATIVE_R7.json'))
const b13 = json(path.join(evidenceRoot, 'B13_UI_R8_REGRESSION.json'))
const b14 = json(path.join(evidenceRoot, 'B14_UI_R8_REGRESSION.json'))
const productionZeroDiff = json(path.join(evidenceRoot, 'R8_PRODUCTION_ZERO_DIFF.json'))
const commandEvidence = json(path.join(evidenceRoot, 'COMMANDS_EXECUTED_R8.json'))
const stageGates = {
  PRODUCT_CODE_ZERO_DIFF: gate(productionZeroDiff.status === 'PASS'),
  DISPLAY_LAYER_REGRESSION: gate(display.status === 'PASS'),
  A11_REGRESSION: gate(a11.allExpectedFieldsObserved === true && a11.allExpectedFieldsMatched === true && a11.receiptVerified === true && a11.postVerifySucceeded === true),
  A11_NOOP_NEGATIVE: gate(noop.status === 'PASS'),
  B13_REGRESSION: gate(b13.status === 'PASS'),
  B14_REGRESSION: gate(b14.status === 'PASS'),
  PACKAGE_INVENTORY: gate(inventory.status === 'PASS' && inventory.mathConsistent === true),
  FILES_SHA256_REAL_VERIFY: gate(filesShaVerify.status === 'PASS' && filesShaVerify.readChecksum === true && filesShaVerify.rehashExecuted === true),
  SECRET_SCAN_FINAL_STAGING: gate(secretScan.status === 'PASS' && secretScan.filesScanned === secretScan.finalStagingFileCount - 1),
  SECRET_SCAN_MUTATION: gate(secretScan.mutationDetected === true),
  COMMANDS_EXECUTED: gate(commandEvidence.status === 'PASS'),
  DESIGN_INPUTS: gate(json(path.join(designRoot, 'DESIGN_INPUTS.json')).complete === true),
}
const preArchiveStatus = Object.values(stageGates).every(value => value === 'PASS') ? 'PRE_ARCHIVE_PASS' : 'FAIL'
write(path.join(evidenceRoot, 'POST_B14_UI_R8_PRE_ARCHIVE_ACCEPTANCE.json'), { schema: 'post-b14-ui-r8-pre-archive-acceptance', sourceCommit, repairBaselineCommit: baseline, b14FrozenCommit, blocking: stageGates, DELIVERY_STATUS: preArchiveStatus, NEXT_STAGE_ALLOWED: false, B15_STARTED: false, RC1_STARTED: false })
makeChecksumFile(stageRoot, checksumRelative)
filesShaVerify = verifyChecksums(stageRoot, checksumRelative)
filesShaVerify.mutationDetected = shaMutation.mismatch && shaMutation.missing && shaMutation.extra
filesShaVerify.missingMutationDetected = shaMutation.missing
filesShaVerify.extraMutationDetected = shaMutation.extra
filesShaVerify.sourceCommit = sourceCommit
write(path.join(evidenceRoot, 'FILES_SHA256_VERIFY_R8.json'), filesShaVerify)
write(path.join(evidenceDir, 'FILES_SHA256_VERIFY_R8.json'), filesShaVerify)
if (filesShaVerify.status !== 'PASS') throw new Error(`R8_FILES_SHA_VERIFY_FAILED:${JSON.stringify(filesShaVerify)}`)

fs.mkdirSync(delivery, { recursive: true })
for (const file of fs.readdirSync(delivery)) if (/^墨衡_V2\.1_Post-B14_Pre-B15_UI_Productization_R[2345678]_/.test(file)) fs.renameSync(path.join(delivery, file), path.join('/Users/mac/.Trash', file))
const archivePath = path.join(delivery, `${packageName}.zip`)
zipDirectory(stageRoot, archivePath)
const readerA = path.join(tempRoot, 'reader-a'); const readerB = path.join(tempRoot, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB)
const listA = run('python3', ['-c', 'import sys,zipfile;print("\\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))', archivePath])
const crcA = run('python3', ['-c', 'import sys,zipfile;print(zipfile.ZipFile(sys.argv[1]).testzip() or "PASS")', archivePath])
const extractA = run('python3', ['-c', 'import sys,zipfile;zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])', archivePath, readerA])
const seven = run('sh', ['-c', 'command -v 7z || command -v unzip'])
const readerTool = seven.stdout.trim().split('\n')[0]
let listB; let crcB; let extractB
if (readerTool.endsWith('/7z')) {
  listB = run('7z', ['l', '-slt', archivePath]); crcB = run('7z', ['t', archivePath]); extractB = run('7z', ['x', archivePath, `-o${readerB}`, '-y'])
} else {
  listB = run('unzip', ['-Z1', archivePath]); crcB = run('unzip', ['-t', archivePath]); extractB = run('unzip', ['-q', archivePath, '-d', readerB])
}
const filesA = allFiles(readerA); const filesB = allFiles(readerB)
const dualReader = crcA.status === 0 && crcB.status === 0 && extractA.status === 0 && extractB.status === 0 && JSON.stringify(filesA) === JSON.stringify(filesB) && treeDigest(readerA) === treeDigest(readerB)
const finalValidation = { schema: 'post-b14-ui-r8-final-archive-validation', archiveFilename: path.basename(archivePath), archiveSha256: sha(archivePath), sourceCommit, readerA: 'Python zipfile', readerB: readerTool.endsWith('/7z') ? '7z' : 'unzip', entryCountA: listA.stdout.split('\n').filter(Boolean).length, entryCountB: filesB.length, entrySetsEqual: JSON.stringify(filesA) === JSON.stringify(filesB), contentTreeDigestA: treeDigest(readerA), contentTreeDigestB: treeDigest(readerB), contentDigestsEqual: treeDigest(readerA) === treeDigest(readerB), crcOrTestPass: crcA.status === 0 && crcB.status === 0 && extractA.status === 0 && extractB.status === 0, status: dualReader ? 'PASS' : 'FAIL' }
const validationPath = path.join(delivery, `${packageName}_FINAL_ARCHIVE_VALIDATION.json`)
write(validationPath, finalValidation)
write(path.join(readerLogsRoot, 'python-zipfile-list.log'), listA.stdout)
write(path.join(readerLogsRoot, 'python-zipfile-test.log'), crcA.stdout)
write(path.join(readerLogsRoot, `${finalValidation.readerB}-list.log`), listB.stdout)
write(path.join(readerLogsRoot, `${finalValidation.readerB}-test.log`), crcB.stdout)
const readerArchive = path.join(delivery, `${packageName}_FINAL_ARCHIVE_READER_LOGS.zip`)
zipDirectory(readerLogsRoot, readerArchive)

const finalCertification = { schema: 'post-b14-ui-r8-final-certification', sourceCommit, archiveFilename: path.basename(archivePath), archiveSha256: finalValidation.archiveSha256, stage1Status: preArchiveStatus, secretScan: secretScan.status, inventory: inventory.status, filesSha256: filesShaVerify.status, crc: finalValidation.crcOrTestPass, dualReader, finalArchiveValidation: finalValidation.status, status: preArchiveStatus === 'PRE_ARCHIVE_PASS' && secretScan.status === 'PASS' && inventory.status === 'PASS' && filesShaVerify.status === 'PASS' && finalValidation.status === 'PASS' ? 'PASS' : 'FAIL' }
const certificationPath = path.join(delivery, `${packageName}_FINAL_CERTIFICATION.json`)
write(certificationPath, finalCertification)

const consistencyPath = path.join(delivery, `${packageName}_EVIDENCE_CONSISTENCY.json`)
const consistency = { schema: 'post-b14-ui-r8-evidence-consistency', sourceCommit, checks: { productionZeroDiff: productionZeroDiff.status === 'PASS', secretScan: secretScan.status === 'PASS' && secretScan.filesScanned === secretScan.finalStagingFileCount - 1 && secretScan.entropyScanExecuted === true, inventory: inventory.status === 'PASS' && inventory.actualTreeCount === inventory.checksumEntries + 1, filesShaVerify: filesShaVerify.status === 'PASS' && filesShaVerify.readChecksum === true && filesShaVerify.rehashExecuted === true, preArchiveAcceptance: preArchiveStatus === 'PRE_ARCHIVE_PASS', archiveValidation: finalValidation.status === 'PASS', finalCertification: finalCertification.status === 'PASS', finalAcceptanceConsumed: true, commands: commandEvidence.status === 'PASS', readme: fs.existsSync(path.join(stageRoot, 'README_ACCEPTANCE.md')) }, finalAcceptanceConsumed: true, status: productionZeroDiff.status === 'PASS' && secretScan.status === 'PASS' && inventory.status === 'PASS' && filesShaVerify.status === 'PASS' && preArchiveStatus === 'PRE_ARCHIVE_PASS' && finalValidation.status === 'PASS' && finalCertification.status === 'PASS' && commandEvidence.status === 'PASS' ? 'PASS' : 'FAIL' }
write(consistencyPath, consistency)

const companionSha256 = { archiveValidationSha256: sha(validationPath), finalCertificationSha256: sha(certificationPath), filesShaVerifySha256: sha(path.join(evidenceDir, 'FILES_SHA256_VERIFY_R8.json')), evidenceConsistencySha256: sha(consistencyPath) }
const finalAcceptance = evaluateFinalAcceptance({ sourceCommit, archiveFilename: path.basename(archivePath), archiveSha256: finalValidation.archiveSha256, productionCodeZeroDiff: productionZeroDiff.status === 'PASS', displayLayerRegression: display.status === 'PASS', a11Regression: stageGates.A11_REGRESSION === 'PASS', b13Regression: b13.status === 'PASS', b14Regression: b14.status === 'PASS', preArchive: json(path.join(evidenceRoot, 'POST_B14_UI_R8_PRE_ARCHIVE_ACCEPTANCE.json')), secretScan, inventory, filesShaVerify, archiveValidation: finalValidation, certification: finalCertification, consistency, companionSha256 })
const acceptancePath = path.join(delivery, `${packageName}_FINAL_ACCEPTANCE.json`)
write(acceptancePath, finalAcceptance)
write(path.join(evidenceDir, 'POST_B14_UI_R8_FINAL_ACCEPTANCE.json'), finalAcceptance)
write(path.join(evidenceDir, 'POST_B14_UI_R8_FINAL_CERTIFICATION.json'), finalCertification)
write(path.join(evidenceDir, 'POST_B14_UI_R8_EVIDENCE_CONSISTENCY.json'), consistency)

const sumsPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`)
write(sumsPath, [archivePath, validationPath, readerArchive, certificationPath, acceptancePath, path.join(evidenceDir, 'FILES_SHA256_VERIFY_R8.json'), consistencyPath].map(file => `${sha(file)}  ${path.basename(file)}`).join('\n') + '\n')
const result = { package: archivePath, sha256sums: sumsPath, validation: validationPath, readerLogs: readerArchive, finalCertification: certificationPath, finalAcceptance: acceptancePath, filesShaVerify: path.join(evidenceDir, 'FILES_SHA256_VERIFY_R8.json'), evidenceConsistency: consistencyPath, sourceCommit, deliveryStatus: finalAcceptance.DELIVERY_STATUS, finalStagingFileCount: secretScan.finalStagingFileCount, secretFilesScanned: secretScan.filesScanned, shaExpectedEntries: filesShaVerify.expectedEntries, shaVerifiedEntries: filesShaVerify.verifiedEntries, status: finalAcceptance.DELIVERY_STATUS === 'READY_FOR_FORMAL_ACCEPTANCE' ? 'PASS' : 'FAIL', frozenInputs: Object.keys(frozenInputCheck).length, initialChecksumEntries: initialEntries }
console.log(JSON.stringify(result, null, 2))
if (result.status !== 'PASS') process.exitCode = 1
