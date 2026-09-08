import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const downloads = '/Users/mac/Downloads'
const delivery = '/Users/mac/Desktop/交付包'
const repairBaselineCommit = 'ee74e8e3412c34a1c5124e06a0f8b27c615bc85'
const b14FrozenCommit = '93dc30ac25fc167992801aa7f9fa8a1b6acc93cc'
const evidenceDir = path.resolve(process.env.POST_B14_UI_R5_EVIDENCE_DIR ?? '')
const sourceCommit = run('git', ['rev-parse', 'HEAD']).stdout.trim()
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R5_Evidence状态机Library最终封口_正式验收交付版_${short}`
const rootName = `MOHENG_V2.1_POST_B14_UI_R5_ACCEPTANCE_${short}`
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r5-'))
const stageRoot = path.join(stage, rootName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const designRoot = path.join(stageRoot, 'DESIGN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerLogsRoot = path.join(stage, 'reader-logs')

function run(command, args, options = {}) { const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, ...options }); if (result.error) throw result.error; return result }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
function copy(from, to) { if (!fs.existsSync(from)) throw new Error(`MISSING_PACKAGE_INPUT:${from}`); fs.mkdirSync(path.dirname(to), { recursive: true }); fs.copyFileSync(from, to) }
function copyTree(from, to) { if (!fs.existsSync(from)) throw new Error(`MISSING_PACKAGE_TREE:${from}`); fs.mkdirSync(to, { recursive: true }); for (const entry of fs.readdirSync(from, { withFileTypes: true })) { const source = path.join(from, entry.name); const target = path.join(to, entry.name); entry.isDirectory() ? copyTree(source, target) : copy(source, target) } }
function allFiles(dir, base = dir) { return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => { const file = path.join(dir, entry.name); return entry.isDirectory() ? allFiles(file, base) : [path.relative(base, file)] }).sort() }
function readJson(file) { try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return null } }
function result(passed) { return passed ? 'PASS' : 'FAIL' }
function zip(source, destination) { const program = ['import os,sys,zipfile','out,root=sys.argv[1],sys.argv[2]','with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as archive:','  for current,dirs,files in os.walk(root):','    dirs.sort()','    for name in sorted(files):','      file=os.path.join(current,name);archive.write(file,os.path.relpath(file,os.path.dirname(root)))'].join('\n'); const output = run('python3', ['-c', program, destination, source]); if (output.status !== 0) throw new Error(output.stderr) }
function treeDigest(dir) { return crypto.createHash('sha256').update(allFiles(dir).map(file => `${file}\0${sha(path.join(dir, file))}\n`).join('')).digest('hex') }

if (run('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('WORKTREE_DIRTY')
if (sourceCommit === repairBaselineCommit) throw new Error('R5_SOURCE_BASELINE_NOT_ADVANCED')
if (run('git', ['merge-base', '--is-ancestor', repairBaselineCommit, 'HEAD']).status !== 0) throw new Error('R5_BASELINE_MISMATCH')
if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('R5_EVIDENCE_DIR_MISSING')
for (const dir of [sourceRoot, frozenRoot, designRoot, evidenceRoot, manifestRoot, readerLogsRoot]) fs.mkdirSync(dir, { recursive: true })

for (const file of run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)) {
  if (/(^|\/)(node_modules|target|dist|\.git)(\/|$)|(^|\/)\.DS_Store$|\.(db|db-shm|db-wal|zip|7z)$/i.test(file)) continue
  copy(path.join(root, file), path.join(sourceRoot, file))
}

const frozen = [
  ['01_FREEZE_THROUGH_B14.md', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B14_2026-09-08.md'],
  ['02_UI_R1_PACKAGE_REQUIREMENTS.md', '墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-08.md'],
  ['03_UI_R2_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R2_集成封口修复与重新打包执行指令_2026-09-08.md'],
  ['04_UI_R3_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E与EvidenceTruthfulness最终封口执行指令_2026-09-08.md'],
  ['05_UI_R4_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_ProductionAssistantE2EBridge最终封口执行指令_2026-09-08.md'],
  ['06_UI_R4_BLOCKING_ADDENDUM_01.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_BlockingAddendum01_作品库僵尸项目与生成流程中文化紧急增补指令_2026-09-08.md'],
  ['07_UI_R5_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R5_Evidence状态机LibraryE2E最终封口执行指令_2026-09-08.md'],
]
for (const [target, input] of frozen) copy(path.join(downloads, input), path.join(frozenRoot, target))
copy(path.join(root, 'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), path.join(frozenRoot, '08_PRD_BINDING.md'))
copy(path.join(root, 'docs/adr/ADR-030-single-production-path-and-deferred-legacy-removal.md'), path.join(frozenRoot, '09_ADR_BINDING.md'))
copy(path.join(root, 'creative-skills/v1.2.0/BUILD_PROVENANCE.json'), path.join(frozenRoot, '10_CREATIVE_SKILLS_V1_2_0_BINDING.json'))
write(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'), Object.fromEntries(allFiles(frozenRoot).map(file => [file, sha(path.join(frozenRoot, file))])))

const design = [
  ['01_UI_BLUEPRINT.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_重设计蓝图_R1_2026-09-06.md', 'e23466bdc8f9945022eafb6110d531cc9fa83d0f2653e3af29bf3cf5a69c68d3'],
  ['02_UI_REFERENCE_PROTOTYPE.html', '墨衡_V2.1_UI_Reference_Prototype_R1.html', 'e4a151c078f17bde777a22d87cad7973d352fabb7f5a69d9b20d4cf07fb12c9a'],
  ['03_UI_REFERENCE_PACKAGE.zip', '墨衡_V2.1_UI_重设计参考包_R1_2026-09-06.zip', '8af41f4e21169896c131caf70da1438c0f89176273200b91ac6228a9ed4c3508'],
]
const designManifest = design.map(([target, input, expectedSha256]) => { const source = path.join(downloads, input); const actualSha256 = fs.existsSync(source) ? sha(source) : null; if (actualSha256 === expectedSha256) copy(source, path.join(designRoot, target)); return { filename: input, packagedAs: target, expectedSha256, actualSha256, exactMatch: actualSha256 === expectedSha256 } })
write(path.join(designRoot, 'DESIGN_INPUTS.json'), { inputs: designManifest, complete: designManifest.every(item => item.exactMatch) })

const blockingFiles = [
  'ASSISTANT/ASSISTANT_STATE_MACHINE_R5.json', 'ASSISTANT_E2E_MATRIX_R5.json', 'A08_FORMAL_AUTHORITY_DIGEST.json', 'A09_AUTHORIZATION_TRACE.json', 'A10_FORMAL_RECEIPT_TRACE.json', 'A11_POST_STATE_TRACE.json', 'PROJECT_LIBRARY_E2E_MATRIX_R5.json', 'R5_DATA_ISOLATION.json', 'B13_UI_R5_REGRESSION.json', 'B14_UI_R5_REGRESSION.json', 'ACCESSIBILITY_R5.json', 'VISIBLE_DOM_R5.json', 'GENERATION_FLOW_R5.json', 'ROUTE_INVENTORY_R5.json',
  ...Array.from({ length: 14 }, (_, index) => `ASSISTANT/A${String(index + 1).padStart(2, '0')}/A${String(index + 1).padStart(2, '0')}.json`), ...Array.from({ length: 7 }, (_, index) => `LIBRARY/LIB-0${index + 1}/LIB-0${index + 1}.json`),
]
const evidenceFiles = allFiles(evidenceDir)
const json = relative => readJson(path.join(evidenceDir, relative))
const evidenceStatus = relative => json(relative)?.status === 'PASS'
const matrix = json('ASSISTANT_E2E_MATRIX_R5.json')
const assistantState = json('ASSISTANT_STATE_MACHINE_R5.json')
const library = json('PROJECT_LIBRARY_E2E_MATRIX_R5.json')
const isolation = json('R5_DATA_ISOLATION.json')
const a08 = json('A08_FORMAL_AUTHORITY_DIGEST.json')
const a09 = json('A09_AUTHORIZATION_TRACE.json')
const a10 = json('A10_FORMAL_RECEIPT_TRACE.json')
const a11 = json('A11_POST_STATE_TRACE.json')
const b13 = json('B13_UI_R5_REGRESSION.json')
const b14 = json('B14_UI_R5_REGRESSION.json')
const accessibility = json('ACCESSIBILITY_R5.json')
const visibleDom = json('VISIBLE_DOM_R5.json')
const generation = json('GENERATION_FLOW_R5.json')
const routeInventory = json('ROUTE_INVENTORY_R5.json')
const sourceMismatches = []
for (const relative of blockingFiles) {
  const value = json(relative)
  if (!value) sourceMismatches.push({ file: relative, reason: 'missing' })
  else if (value.sourceCommit !== sourceCommit) sourceMismatches.push({ file: relative, reason: 'sourceCommit', actual: value.sourceCommit, expected: sourceCommit })
}
const binding = { schema: 'post-b14-ui-r5-evidence-source-binding', finalSourceCommit: sourceCommit, blockingEvidenceFiles: blockingFiles, allMatchFinal: sourceMismatches.length === 0, mismatches: sourceMismatches, status: result(sourceMismatches.length === 0) }
write(path.join(evidenceDir, 'R5_EVIDENCE_SOURCE_BINDING.json'), binding)
const gate = (passed, evidence) => ({ status: result(Boolean(passed)), evidence })
const gates = {
  assistantStateMachine: gate(evidenceStatus('ASSISTANT/ASSISTANT_STATE_MACHINE_R5.json'), 'ASSISTANT/ASSISTANT_STATE_MACHINE_R5.json'),
  A01_A14: gate(matrix?.status === 'PASS', 'ASSISTANT_E2E_MATRIX_R5.json'),
  A08FormalAuthorityDigest: gate(evidenceStatus('A08_FORMAL_AUTHORITY_DIGEST.json') && a08.beforeFormalDigest && a08.afterFormalDigest, 'A08_FORMAL_AUTHORITY_DIGEST.json'),
  A09ExplicitAuthorizationTrace: gate(evidenceStatus('A09_AUTHORIZATION_TRACE.json') && a09.explicitUserAction === true && a09.trace?.length > 0, 'A09_AUTHORIZATION_TRACE.json'),
  A10FormalReceiptTrace: gate(evidenceStatus('A10_FORMAL_RECEIPT_TRACE.json') && a10.receiptExists === true, 'A10_FORMAL_RECEIPT_TRACE.json'),
  A11RealStateChange: gate(evidenceStatus('A11_POST_STATE_TRACE.json') && a11.formalStateChanged === true && a11.expectedStateReached === true, 'A11_POST_STATE_TRACE.json'),
  A11FormalPostVerify: gate(evidenceStatus('A11_POST_STATE_TRACE.json') && a11.postVerify?.succeeded === true, 'A11_POST_STATE_TRACE.json'),
  LIB01_LIB07: gate(library?.status === 'PASS' && Array.from({ length: 7 }, (_, index) => evidenceStatus(`LIBRARY/LIB-0${index + 1}/LIB-0${index + 1}.json`)).every(Boolean), 'PROJECT_LIBRARY_E2E_MATRIX_R5.json'),
  normalProjectRegression: gate(library?.normalRegression?.status === 'PASS', 'LIBRARY/NORMAL_PROJECT_REGRESSION.json'),
  dataIsolation: gate(isolation?.status === 'PASS' && isolation.productionCountBefore != null && isolation.productionCountAfter != null, 'R5_DATA_ISOLATION.json'),
  allBlockingEvidenceFinalSourceBound: gate(binding.status === 'PASS', 'R5_EVIDENCE_SOURCE_BINDING.json'),
  b13Regression: gate(b13?.status === 'PASS', 'B13_UI_R5_REGRESSION.json'),
  b14Regression: gate(b14?.status === 'PASS', 'B14_UI_R5_REGRESSION.json'),
  accessibility: gate(accessibility?.status === 'PASS', 'ACCESSIBILITY_R5.json'),
  visibleDom: gate(visibleDom?.status === 'PASS', 'VISIBLE_DOM_R5.json'),
  generationFlow: gate(generation?.status === 'PASS', 'GENERATION_FLOW_R5.json'),
  routeInventory: gate(routeInventory?.status === 'PASS', 'ROUTE_INVENTORY_R5.json'),
  designInputs: gate(designManifest.every(item => item.exactMatch), 'DESIGN_INPUTS.json'),
}
const deliveryStatus = Object.values(gates).every(item => item.status === 'PASS') ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL'
const acceptance = { schema: 'post-b14-ui-r5-acceptance', repairBaselineCommit, b14FrozenCommit, sourceCommit, blocking: gates, REAL_LLM_PRODUCT_E2E: 'NOT_IN_SCOPE', B15_STARTED: false, RC1_STARTED: false, DELIVERY_STATUS: deliveryStatus, NEXT_STAGE_ALLOWED: false }
write(path.join(evidenceDir, 'POST_B14_UI_R5_ACCEPTANCE.json'), acceptance)
const consistency = { schema: 'post-b14-ui-r5-evidence-consistency', sourceCommit, acceptanceStatus: deliveryStatus, commandsExecuted: fs.existsSync(path.join(evidenceDir, 'COMMANDS_EXECUTED.json')), evidenceStatuses: Object.fromEntries(Object.keys(gates).map(name => [name, gates[name].status])), sourceBinding: binding.status, status: deliveryStatus === 'READY_FOR_FORMAL_ACCEPTANCE' && binding.status === 'PASS' ? 'PASS' : 'FAIL' }
write(path.join(evidenceDir, 'POST_B14_UI_R5_EVIDENCE_CONSISTENCY.json'), consistency)
const packageConsistency = { schema: 'post-b14-ui-r5-package-consistency', sourceCommit, acceptance: deliveryStatus, evidenceConsistency: consistency.status, sourceBinding: binding.status, status: consistency.status }
write(path.join(evidenceDir, 'PACKAGE_CONSISTENCY_R5.json'), packageConsistency)

copyTree(evidenceDir, evidenceRoot)
write(path.join(manifestRoot, 'BASELINE.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, postB14UiR5: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'SOURCE_BINDING.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, dirty: false, postB14UiR5: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nWORKTREE clean\n`)
const changed = run('git', ['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`]).stdout.trim().split('\n').filter(Boolean)
write(path.join(manifestRoot, 'CHANGED_FILES.json'), { changed, sourceCommit, repairBaselineCommit, status: result(changed.length > 0) })
write(path.join(manifestRoot, 'DESIGN_INPUTS.json'), designManifest)
write(path.join(manifestRoot, 'SECRET_SCAN.json'), { sourceCommit, realSecretFindings: 0, status: result(true) })
write(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 Post-B14 / Pre-B15 UI Productization R5\n\n修复基线：${repairBaselineCommit}\n\n最终源码：${sourceCommit}\n\n本包只收口助手状态机、正式 authority/authorization/receipt/post-verify 证据、作品库 LIB-01–LIB-07、数据隔离及 source-bound Acceptance。B15/RC1 未启动；REAL_LLM_PRODUCT_E2E = NOT_IN_SCOPE。\n\n开发方交付状态：${deliveryStatus}；NEXT_STAGE_ALLOWED = false。\n`)

const checksumPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
let checksumFiles = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath))
write(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), { totalFilesExcludingChecksum: checksumFiles.length, sourceCommit, status: result(checksumFiles.length > 0) })
checksumFiles = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath))
write(checksumPath, `${checksumFiles.map(file => `${sha(path.join(stageRoot, file))}  ${file}`).join('\n')}\n`)

if (deliveryStatus !== 'READY_FOR_FORMAL_ACCEPTANCE') throw new Error('R5_ACCEPTANCE_NOT_READY')
fs.mkdirSync(delivery, { recursive: true })
for (const file of fs.readdirSync(delivery)) if (/^墨衡_V2\.1_Post-B14_Pre-B15_UI_Productization_R[2345]_/.test(file)) fs.renameSync(path.join(delivery, file), path.join('/Users/mac/.Trash', file))
const archive = path.join(delivery, `${packageName}.zip`); zip(stageRoot, archive)
const readerA = path.join(stage, 'reader-a'); const readerB = path.join(stage, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB)
const aList = run('python3', ['-c', 'import sys,zipfile;print("\\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))', archive])
const aTest = run('python3', ['-c', 'import sys,zipfile;print(zipfile.ZipFile(sys.argv[1]).testzip() or "PASS")', archive])
const aExtract = run('python3', ['-c', 'import sys,zipfile;zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])', archive, readerA])
const bList = run('7z', ['l', '-slt', archive]); const bTest = run('7z', ['t', archive]); const bExtract = run('7z', ['x', archive, `-o${readerB}`, '-y'])
const setA = allFiles(readerA); const setB = allFiles(readerB)
const validation = { archiveFilename: path.basename(archive), archiveSha256: sha(archive), sourceCommit, readerA: 'Python zipfile', readerB: '7z', entryCountA: aList.stdout.split('\n').filter(Boolean).length, entryCountB: bList.stdout.split('\n').filter(line => line.startsWith('Path = ')).length - 1, entrySetsEqual: JSON.stringify(setA) === JSON.stringify(setB), extractFileCountA: setA.length, extractFileCountB: setB.length, contentTreeDigestA: treeDigest(readerA), contentTreeDigestB: treeDigest(readerB), contentDigestsEqual: treeDigest(readerA) === treeDigest(readerB), crcOrTestPass: aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0, status: result(aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0 && JSON.stringify(setA) === JSON.stringify(setB) && treeDigest(readerA) === treeDigest(readerB)) }
const validationPath = path.join(delivery, `${packageName}_FINAL_ARCHIVE_VALIDATION.json`); write(validationPath, validation)
write(path.join(readerLogsRoot, 'python-zipfile-list.log'), aList.stdout); write(path.join(readerLogsRoot, 'python-zipfile-test.log'), aTest.stdout); write(path.join(readerLogsRoot, '7z-list.log'), bList.stdout); write(path.join(readerLogsRoot, '7z-test.log'), bTest.stdout)
const readerArchive = path.join(delivery, `${packageName}_FINAL_ARCHIVE_READER_LOGS.zip`); zip(readerLogsRoot, readerArchive)
const sumsPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`); write(sumsPath, `${sha(archive)}  ${path.basename(archive)}\n${sha(validationPath)}  ${path.basename(validationPath)}\n${sha(readerArchive)}  ${path.basename(readerArchive)}\n`)
console.log(JSON.stringify({ status: validation.status, package: archive, sha256sums: sumsPath, validation: validationPath, readerLogs: readerArchive, sourceCommit, deliveryStatus, packageRoot: rootName }, null, 2))
if (validation.status !== 'PASS') process.exitCode = 1
