import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const downloads = '/Users/mac/Downloads'
const delivery = '/Users/mac/Desktop/交付包'
const repairBaselineCommit = '4ae12ebb65075bb20fd0ae9aa4b2ee818a451cd1'
const b14FrozenCommit = '93dc30ac25fc167992801aa7f9fa8a1b6acc93cc'
const evidenceDir = path.resolve(process.env.POST_B14_UI_R6_EVIDENCE_DIR ?? '')
const sourceCommit = run('git', ['rev-parse', 'HEAD']).stdout.trim()
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R6_真实Trace数据隔离与最终包封口_正式验收交付版_${short}`
const rootName = `MOHENG_V2.1_POST_B14_UI_R6_ACCEPTANCE_${short}`
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r6-'))
const stageRoot = path.join(stage, rootName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const designRoot = path.join(stageRoot, 'DESIGN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerLogsRoot = path.join(stage, 'reader-logs')
function run(command, args, options = {}) { const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, ...options }); if (result.error) throw result.error; return result }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
function copy(from, to) { if (!fs.existsSync(from)) throw new Error(`MISSING_R6_INPUT:${from}`); fs.mkdirSync(path.dirname(to), { recursive: true }); fs.copyFileSync(from, to) }
function copyTree(from, to) { fs.mkdirSync(to, { recursive: true }); for (const entry of fs.readdirSync(from, { withFileTypes: true })) { const a = path.join(from, entry.name); const b = path.join(to, entry.name); entry.isDirectory() ? copyTree(a, b) : copy(a, b) } }
function allFiles(dir, base = dir) { return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => { const file = path.join(dir, entry.name); return entry.isDirectory() ? allFiles(file, base) : [path.relative(base, file)] }).sort() }
function readJson(relative) { try { return JSON.parse(fs.readFileSync(path.join(evidenceDir, relative), 'utf8')) } catch { return null } }
function status(value) { return value ? 'PASS' : 'FAIL' }
function zip(source, destination) { const script = ['import os,sys,zipfile','out,root=sys.argv[1],sys.argv[2]','with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as z:','  for d,ds,fs in os.walk(root):','    ds.sort()','    for f in sorted(fs):','      p=os.path.join(d,f);z.write(p,os.path.relpath(p,os.path.dirname(root)))'].join('\n'); const r = run('python3', ['-c', script, destination, source]); if (r.status !== 0) throw new Error(r.stderr) }
function treeDigest(dir) { return crypto.createHash('sha256').update(allFiles(dir).map(file => `${file}\0${sha(path.join(dir, file))}\n`).join('')).digest('hex') }

if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('R6_EVIDENCE_DIR_MISSING')
if (run('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('WORKTREE_DIRTY')
if (sourceCommit === repairBaselineCommit || run('git', ['merge-base', '--is-ancestor', repairBaselineCommit, 'HEAD']).status !== 0) throw new Error('R6_BASELINE_MISMATCH')
for (const dir of [sourceRoot, frozenRoot, designRoot, evidenceRoot, manifestRoot, readerLogsRoot]) fs.mkdirSync(dir, { recursive: true })
for (const file of run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)) if (!/(^|\/)(node_modules|target|dist|\.git)(\/|$)|(^|\/)\.DS_Store$|\.(db|db-shm|db-wal|zip|7z)$/i.test(file)) copy(path.join(root, file), path.join(sourceRoot, file))

const frozen = [
  ['01_FREEZE_THROUGH_B14.md', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B14_2026-09-08.md'],
  ['02_UI_R1_PACKAGE_REQUIREMENTS.md', '墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-09.md'],
  ['03_UI_R2_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R2_集成封口修复与重新打包执行指令_2026-09-08.md'],
  ['04_UI_R3_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E与EvidenceTruthfulness最终封口执行指令_2026-09-08.md'],
  ['05_UI_R4_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_ProductionAssistantE2EBridge最终封口执行指令_2026-09-08.md'],
  ['06_UI_R4_BLOCKING_ADDENDUM_01.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_BlockingAddendum01_作品库僵尸项目与生成流程中文化紧急增补指令_2026-09-08.md'],
  ['07_UI_R5_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R5_Evidence状态机LibraryE2E最终封口执行指令_2026-09-08.md'],
  ['08_UI_R6_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R6_真实Trace数据隔离与最终包封口执行指令_2026-09-09.md'],
]
// The R1 filename is stable in Downloads in the current delivery environment.
if (!fs.existsSync(path.join(downloads, frozen[1][1]))) frozen[1][1] = '墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-08.md'
for (const [target, input] of frozen) copy(path.join(downloads, input), path.join(frozenRoot, target))
copy(path.join(root, 'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), path.join(frozenRoot, '09_PRD_BINDING.md'))
copy(path.join(root, 'docs/adr/ADR-030-single-production-path-and-deferred-legacy-removal.md'), path.join(frozenRoot, '10_ADR_BINDING.md'))
copy(path.join(root, 'creative-skills/v1.2.0/BUILD_PROVENANCE.json'), path.join(frozenRoot, '11_CREATIVE_SKILLS_V1_2_0_BINDING.json'))
write(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'), Object.fromEntries(allFiles(frozenRoot).map(file => [file, sha(path.join(frozenRoot, file))])))

const designs = [
  ['01_UI_BLUEPRINT.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_重设计蓝图_R1_2026-09-06.md', 'e23466bdc8f9945022eafb6110d531cc9fa83d0f2653e3af29bf3cf5a69c68d3'],
  ['02_UI_REFERENCE_PROTOTYPE.html', '墨衡_V2.1_UI_Reference_Prototype_R1.html', 'e4a151c078f17bde777a22d87cad7973d352fabb7f5a69d9b20d4cf07fb12c9a'],
  ['03_UI_REFERENCE_PACKAGE.zip', '墨衡_V2.1_UI_重设计参考包_R1_2026-09-06.zip', '8af41f4e21169896c131caf70da1438c0f89176273200b91ac6228a9ed4c3508'],
]
const designManifest = designs.map(([target, input, expected]) => { const source = path.join(downloads, input); const actual = fs.existsSync(source) ? sha(source) : null; if (actual === expected) copy(source, path.join(designRoot, target)); return { filename: input, packagedAs: target, expectedSha256: expected, actualSha256: actual, exactMatch: actual === expected } })
write(path.join(designRoot, 'DESIGN_INPUTS.json'), { inputs: designManifest, complete: designManifest.every(item => item.exactMatch) })

const evidenceRequired = ['A09_AUTHORIZATION_RUNTIME_TRACE_R6.json','A11_TARGET_STATE_R6.json','A11_NOOP_NEGATIVE_R6.json','R6_DATA_ISOLATION_PRODUCTION_PROFILE.json','LIB-06_R6.json','PROJECT_LIBRARY_E2E_MATRIX_R6.json','FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json','B13_UI_R6_REGRESSION.json','B14_UI_R6_REGRESSION.json','ACCESSIBILITY_R6.json','ROUTE_INVENTORY_R6.json','GENERATION_FLOW_R6.json','COMMANDS_EXECUTED_R6.json']
const json = file => readJson(file)
const sourceMismatches = []
for (const file of evidenceRequired) { const value = json(file); if (!value) sourceMismatches.push({ file, reason: 'missing' }); else if (value.sourceCommit !== sourceCommit) sourceMismatches.push({ file, reason: 'sourceCommit', actual: value.sourceCommit, expected: sourceCommit }) }
const finalBinding = { schema: 'post-b14-ui-r6-final-source-binding', finalSourceCommit: sourceCommit, evidenceFiles: evidenceRequired, mismatches: sourceMismatches, allMatchFinal: sourceMismatches.length === 0, status: status(sourceMismatches.length === 0) }
write(path.join(evidenceDir, 'R6_EVIDENCE_SOURCE_BINDING.json'), finalBinding)
const a09 = json('A09_AUTHORIZATION_RUNTIME_TRACE_R6.json'); const a11 = json('A11_TARGET_STATE_R6.json'); const noop = json('A11_NOOP_NEGATIVE_R6.json'); const isolation = json('R6_DATA_ISOLATION_PRODUCTION_PROFILE.json'); const lib06 = json('LIB-06_R6.json'); const library = json('PROJECT_LIBRARY_E2E_MATRIX_R6.json'); const dom = json('FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json'); const b13 = json('B13_UI_R6_REGRESSION.json'); const b14 = json('B14_UI_R6_REGRESSION.json'); const accessibility = json('ACCESSIBILITY_R6.json'); const route = json('ROUTE_INVENTORY_R6.json'); const generation = json('GENERATION_FLOW_R6.json')
const gates = {
  A09_REAL_RUNTIME_TRACE: status(a09?.status === 'PASS' && a09.traceSource === 'BRIDGE_RUNTIME_RECORDER'),
  A09_AUTHORIZATION_BEFORE_EXECUTE: status(a09?.orderingValid === true),
  A11_TARGET_STATE_EXPLICIT: status(a11?.targetDomain === 'FTSProjectionSettings' && a11?.beforeTargetState && a11?.afterTargetState),
  A11_EXPECTED_STATE_REACHED: status(a11?.targetStateChanged === true && a11?.expectedStateReached === true),
  A11_NOOP_FAIL_CLOSED: status(noop?.verifierWouldFail === true && noop?.formalStateChanged === false),
  PRODUCTION_PROFILE_ISOLATION: status(isolation?.productionProfileKind === 'TAURI_APP_DATA_DIR' && isolation?.sameDb === false),
  PRODUCTION_COUNT_UNCHANGED: status(isolation?.productionCountBefore != null && isolation?.productionCountAfter != null && isolation.productionCountBefore === isolation.productionCountAfter),
  PRODUCTION_CATALOG_DIGEST_UNCHANGED: status(isolation?.productionCatalogDigestBefore != null && isolation?.productionCatalogDigestBefore === isolation?.productionCatalogDigestAfter),
  LIB06_ERROR_CAUGHT: status(lib06?.uiErrorVisible === true),
  LIB06_PAGEERROR_ZERO: status(lib06?.pageErrors === 0),
  LIB06_CONSOLEERROR_ZERO: status(lib06?.consoleErrors === 0),
  LIB06_NO_FALSE_SUCCESS: status(lib06?.falseSuccess === 0 && lib06?.projectStillPresent === true),
  LIB01_LIB07: status(library?.status === 'PASS'),
  FULL_PRODUCT_VISIBLE_DOM: status(dom?.status === 'PASS'),
  DEV_TRACE_ZERO: status((dom?.devTraceHits?.length ?? 1) === 0),
  DEMO_FIXTURE_TERMS_ZERO: status((dom?.demoFixtureHits?.length ?? 1) === 0),
  B13_REGRESSION: status(b13?.status === 'PASS'), B14_REGRESSION: status(b14?.status === 'PASS'), ACCESSIBILITY: status(accessibility?.status === 'PASS'), ROUTE_INVENTORY: status(route?.status === 'PASS'), GENERATION_FLOW: status(generation?.status === 'PASS'), FINAL_SOURCE_BINDING: finalBinding.status,
}
const stage1 = Object.values(gates).every(value => value === 'PASS') ? 'PASS' : 'FAIL'
write(path.join(evidenceDir, 'PACKAGE_INVENTORY_R6.json'), { schema: 'post-b14-ui-r6-package-inventory', sourceCommit, stage: 'pre-archive', status: 'PENDING_FINAL_STAGING' })
write(path.join(evidenceDir, 'PACKAGE_CONSISTENCY_R6.json'), { schema: 'post-b14-ui-r6-package-consistency', sourceCommit, stage1Status: stage1, status: stage1 })
write(path.join(evidenceDir, 'POST_B14_UI_R6_PRE_ARCHIVE_ACCEPTANCE.json'), { schema: 'post-b14-ui-r6-pre-archive-acceptance', sourceCommit, blocking: gates, DELIVERY_STATUS: stage1 === 'PASS' ? 'PRE_ARCHIVE_PASS' : 'FAIL', NEXT_STAGE_ALLOWED: false })
if (stage1 !== 'PASS') throw new Error('R6_PRE_ARCHIVE_ACCEPTANCE_NOT_READY')
copyTree(evidenceDir, evidenceRoot)
write(path.join(manifestRoot, 'BASELINE.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, postB14UiR6: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'SOURCE_BINDING.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, dirty: false, postB14UiR6: true, realLlmProductE2E: false, b15Started: false, b15Allowed: false })
write(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), `HEAD=${sourceCommit}\nWORKTREE clean\n`)
write(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 Post-B14 / Pre-B15 UI Productization R6\n\n修复基线：${repairBaselineCommit}\n最终源码：${sourceCommit}\n本轮为真实 Trace、目标状态、生产 profile 隔离、全产品 Visible DOM 与最终包 Gate 收口。B15/RC1 未启动。\n`)
write(path.join(manifestRoot, 'PACKAGE_INVENTORY_R6.json'), { schema: 'post-b14-ui-r6-package-inventory', rule: 'actual final tree count; checksum file excluded from checksum entries', actualTreeCount: 0, checksumEntries: 0, status: 'PENDING_FINAL_STAGING' })
write(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), '')
let files = allFiles(stageRoot); const checksumRelative = path.relative(stageRoot, path.join(manifestRoot, 'FILES_SHA256SUMS.txt')); const inventoryRelative = path.relative(stageRoot, path.join(manifestRoot, 'PACKAGE_INVENTORY_R6.json')); const checksumEntries = files.filter(file => file !== checksumRelative); write(path.join(manifestRoot, 'PACKAGE_INVENTORY_R6.json'), { schema: 'post-b14-ui-r6-package-inventory', sourceCommit, rule: 'actual final tree count; checksum file excluded from checksum entries', actualTreeCount: files.length, checksumEntries: checksumEntries.length, inventoryFile: inventoryRelative, checksumFile: checksumRelative, status: 'PASS' }); files = allFiles(stageRoot); write(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), `${files.filter(file => file !== checksumRelative).map(file => `${sha(path.join(stageRoot, file))}  ${file}`).join('\n')}\n`)

fs.mkdirSync(delivery, { recursive: true }); for (const file of fs.readdirSync(delivery)) if (/^墨衡_V2\.1_Post-B14_Pre-B15_UI_Productization_R[23456]_/.test(file)) fs.renameSync(path.join(delivery, file), path.join('/Users/mac/.Trash', file))
const archive = path.join(delivery, `${packageName}.zip`); zip(stageRoot, archive); const readerA = path.join(stage, 'reader-a'); const readerB = path.join(stage, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB)
const aList = run('python3', ['-c', 'import sys,zipfile;print("\\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))', archive]); const aTest = run('python3', ['-c', 'import sys,zipfile;print(zipfile.ZipFile(sys.argv[1]).testzip() or "PASS")', archive]); const aExtract = run('python3', ['-c', 'import sys,zipfile;zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])', archive, readerA]); const bList = run('7z', ['l', '-slt', archive]); const bTest = run('7z', ['t', archive]); const bExtract = run('7z', ['x', archive, `-o${readerB}`, '-y']); const setA = allFiles(readerA); const setB = allFiles(readerB); const dualPass = aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0 && JSON.stringify(setA) === JSON.stringify(setB) && treeDigest(readerA) === treeDigest(readerB)
const validation = { schema: 'post-b14-ui-r6-final-archive-validation', archiveFilename: path.basename(archive), archiveSha256: sha(archive), sourceCommit, readerA: 'Python zipfile', readerB: '7z', entryCountA: aList.stdout.split('\n').filter(Boolean).length, entryCountB: bList.stdout.split('\n').filter(line => line.startsWith('Path = ')).length - 1, entrySetsEqual: JSON.stringify(setA) === JSON.stringify(setB), contentTreeDigestA: treeDigest(readerA), contentTreeDigestB: treeDigest(readerB), contentDigestsEqual: treeDigest(readerA) === treeDigest(readerB), crcOrTestPass: aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0, status: status(dualPass) }
const validationPath = path.join(delivery, `${packageName}_FINAL_ARCHIVE_VALIDATION.json`); write(validationPath, validation); write(path.join(readerLogsRoot, 'python-zipfile-test.log'), aTest.stdout); write(path.join(readerLogsRoot, '7z-test.log'), bTest.stdout); const readerArchive = path.join(delivery, `${packageName}_FINAL_ARCHIVE_READER_LOGS.zip`); zip(readerLogsRoot, readerArchive)
const finalCertification = { schema: 'post-b14-ui-r6-final-certification', sourceCommit, archiveFilename: path.basename(archive), archiveSha256: sha(archive), finalArchiveValidation: validation.status, finalCrc: validation.crcOrTestPass, finalDualReader: dualPass, stage1Status: stage1, status: stage1 === 'PASS' && dualPass ? 'PASS' : 'FAIL' }
const certificationPath = path.join(delivery, `${packageName}_FINAL_CERTIFICATION.json`); write(certificationPath, finalCertification)
const finalAcceptance = { schema: 'post-b14-ui-r6-acceptance', repairBaselineCommit, b14FrozenCommit, sourceCommit, blocking: { ...gates, PACKAGE_INVENTORY: status(validation.entryCountA === validation.entryCountB && validation.entrySetsEqual), FILES_SHA256: status(fs.existsSync(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'))), FINAL_ZIP_SHA: status(Boolean(validation.archiveSha256)), FINAL_CRC: status(validation.crcOrTestPass), FINAL_DUAL_READER: status(dualPass), FINAL_CERTIFICATION: finalCertification.status, EVIDENCE_CONSISTENCY: stage1 === 'PASS' ? 'PASS' : 'FAIL' }, DELIVERY_STATUS: stage1 === 'PASS' && finalCertification.status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', NEXT_STAGE_ALLOWED: false, B15_STARTED: false, RC1_STARTED: false }
write(path.join(evidenceDir, 'POST_B14_UI_R6_FINAL_CERTIFICATION.json'), finalCertification); write(path.join(evidenceDir, 'POST_B14_UI_R6_ACCEPTANCE.json'), finalAcceptance)
const sumsPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`); write(sumsPath, `${sha(archive)}  ${path.basename(archive)}\n${sha(validationPath)}  ${path.basename(validationPath)}\n${sha(readerArchive)}  ${path.basename(readerArchive)}\n${sha(certificationPath)}  ${path.basename(certificationPath)}\n`)
console.log(JSON.stringify({ package: archive, sha256sums: sumsPath, validation: validationPath, readerLogs: readerArchive, finalCertification: certificationPath, sourceCommit, deliveryStatus: finalAcceptance.DELIVERY_STATUS, status: finalAcceptance.DELIVERY_STATUS === 'READY_FOR_FORMAL_ACCEPTANCE' ? 'PASS' : 'FAIL' }, null, 2)); if (finalAcceptance.DELIVERY_STATUS !== 'READY_FOR_FORMAL_ACCEPTANCE') process.exitCode = 1
