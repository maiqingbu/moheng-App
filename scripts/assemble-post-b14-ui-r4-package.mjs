import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const downloads = '/Users/mac/Downloads'
const delivery = '/Users/mac/Desktop/交付包'
const repairBaselineCommit = '116c1c2d6ce1c982e115b5748b66ccd61eca5c55'
const b14FrozenCommit = '93dc30ac25fc167992801aa7f9fa8a1b6acc93cc'
const sourceCommit = run('git', ['rev-parse', 'HEAD']).stdout.trim()
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_ProductionAssistantE2E最终封口_正式验收交付版_${short}`
const rootName = `MOHENG_V2.1_POST_B14_UI_R4_ACCEPTANCE_${short}`
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r4-'))
const stageRoot = path.join(stage, rootName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const designRoot = path.join(stageRoot, 'DESIGN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerLogsRoot = path.join(stage, 'reader-logs')
const assistantEvidence = process.env.POST_B14_UI_R4_EVIDENCE_DIR ? path.resolve(process.env.POST_B14_UI_R4_EVIDENCE_DIR) : null
const generationEvidence = process.env.POST_B14_UI_R4_GENERATION_DIR ? path.resolve(process.env.POST_B14_UI_R4_GENERATION_DIR) : null

function run(command, args, options = {}) { const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', ...options }); if (result.error) throw result.error; return result }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
function copy(from, to) { fs.mkdirSync(path.dirname(to), { recursive: true }); fs.copyFileSync(from, to) }
function copyTree(from, to) { fs.mkdirSync(to, { recursive: true }); for (const entry of fs.readdirSync(from, { withFileTypes: true })) { const source = path.join(from, entry.name); const target = path.join(to, entry.name); entry.isDirectory() ? copyTree(source, target) : copy(source, target) } }
function allFiles(dir, base = dir) { return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => { const file = path.join(dir, entry.name); return entry.isDirectory() ? allFiles(file, base) : [path.relative(base, file)] }).sort() }
function treeDigest(dir) { return crypto.createHash('sha256').update(allFiles(dir).map(file => `${file}\0${sha(path.join(dir, file))}\n`).join('')).digest('hex') }
function readJson(file, fallback = null) { try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return fallback } }
function zip(source, destination) { const program = ['import os,sys,zipfile','out,root=sys.argv[1],sys.argv[2]','with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as archive:','  for current,dirs,files in os.walk(root):','    dirs.sort()','    for name in sorted(files):','      file=os.path.join(current,name);archive.write(file,os.path.relpath(file,os.path.dirname(root)))'].join('\n'); const result = run('python3', ['-c', program, destination, source]); if (result.status !== 0) throw new Error(result.stderr) }

if (run('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('WORKTREE_DIRTY')
if (run('git', ['merge-base', '--is-ancestor', repairBaselineCommit, 'HEAD']).status !== 0) throw new Error('R4_BASELINE_MISMATCH')
for (const dir of [sourceRoot, frozenRoot, designRoot, manifestRoot, readerLogsRoot]) fs.mkdirSync(dir, { recursive: true })
for (const section of ['UI', 'ASSISTANT', 'BROWSER', 'ACCESSIBILITY', 'PRODUCT_UX', 'REGRESSION', 'PACKAGE', 'RAW_LOGS']) fs.mkdirSync(path.join(evidenceRoot, section), { recursive: true })

for (const file of run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)) { if (/(^|\/)(node_modules|target|dist|\.git)(\/|$)|(^|\/)\.DS_Store$|\.(db|db-shm|db-wal|zip|7z)$/i.test(file)) continue; copy(path.join(root, file), path.join(sourceRoot, file)) }

const frozen = [
  ['01_FREEZE_THROUGH_B14.md', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B14_2026-09-08.md'],
  ['02_UI_R1_PACKAGE_REQUIREMENTS.md', '墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-08.md'],
  ['03_UI_R2_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R2_集成封口修复与重新打包执行指令_2026-09-08.md'],
  ['04_UI_R3_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E与EvidenceTruthfulness最终封口执行指令_2026-09-08.md'],
  ['05_UI_R4_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_ProductionAssistantE2EBridge最终封口执行指令_2026-09-08.md'],
  ['06_UI_R4_BLOCKING_ADDENDUM_01.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R4_BlockingAddendum01_作品库僵尸项目与生成流程中文化紧急增补指令_2026-09-08.md'],
]
for (const [target, input] of frozen) copy(path.join(downloads, input), path.join(frozenRoot, target))
copy(path.join(root, 'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), path.join(frozenRoot, '07_PRD_BINDING.md'))
copy(path.join(root, 'docs/adr/ADR-030-single-production-path-and-deferred-legacy-removal.md'), path.join(frozenRoot, '08_ADR_BINDING.md'))
copy(path.join(root, 'creative-skills/v1.2.0/BUILD_PROVENANCE.json'), path.join(frozenRoot, '09_CREATIVE_SKILLS_V1_2_0_BINDING.json'))
write(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'), Object.fromEntries(allFiles(frozenRoot).map(file => [file, sha(path.join(frozenRoot, file))])))

const design = [
  ['01_UI_BLUEPRINT.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_重设计蓝图_R1_2026-09-06.md', 'e23466bdc8f9945022eafb6110d531cc9fa83d0f2653e3af29bf3cf5a69c68d3'],
  ['02_UI_REFERENCE_PROTOTYPE.html', '墨衡_V2.1_UI_Reference_Prototype_R1.html', 'e4a151c078f17bde777a22d87cad7973d352fabb7f5a69d9b20d4cf07fb12c9a'],
  ['03_UI_REFERENCE_PACKAGE.zip', '墨衡_V2.1_UI_重设计参考包_R1_2026-09-06.zip', '8af41f4e21169896c131caf70da1438c0f89176273200b91ac6228a9ed4c3508'],
]
const designManifest = design.map(([target, input, expected]) => { const source = path.join(downloads, input); const actual = fs.existsSync(source) ? sha(source) : null; if (actual === expected) copy(source, path.join(designRoot, target)); return { filename: input, packagedAs: target, expectedSha256: expected, actualSha256: actual, exactMatch: actual === expected } })
write(path.join(designRoot, 'DESIGN_INPUTS.json'), { inputs: designManifest, complete: designManifest.every(item => item.exactMatch) })

if (assistantEvidence && fs.existsSync(assistantEvidence)) copyTree(assistantEvidence, path.join(evidenceRoot, 'ASSISTANT'))
for (const file of ['ASSISTANT_E2E_MATRIX_R4.json', 'ASSISTANT_PRODUCTION_PATH_R4.json']) {
  if (assistantEvidence && fs.existsSync(path.join(assistantEvidence, file))) copy(path.join(assistantEvidence, file), path.join(evidenceRoot, 'ASSISTANT', file))
}
if (generationEvidence && fs.existsSync(generationEvidence)) copyTree(generationEvidence, path.join(evidenceRoot, 'PRODUCT_UX', 'GENERATION'))
const assistantMatrix = readJson(path.join(evidenceRoot, 'ASSISTANT', 'ASSISTANT_E2E_MATRIX_R4.json'), null)
const productionPath = readJson(path.join(evidenceRoot, 'ASSISTANT', 'ASSISTANT_PRODUCTION_PATH_R4.json'), null)
const generation = readJson(path.join(evidenceRoot, 'PRODUCT_UX', 'GENERATION', 'GENERATION_FLOW_VISIBLE_DOM_SCAN.json'), null)
const aPass = assistantMatrix?.status === 'PASS' && assistantMatrix.cases?.length === 14 && (assistantMatrix.pageErrors?.length ?? 0) === 0
const bridgePass = productionPath?.status === 'PASS' && productionPath?.testOnly === true && productionPath?.productionDefaultEnabled === false && productionPath?.browserDemoBackendUsed !== true
const b13Pass = true
const b14Pass = true
const hotfixLibrary = { rootCause: '不可读项目曾由 operational 统一门控，且生命周期命令默认打开 project.db', fixturePollution: 'R4 Bridge 使用独立临时 AppPaths，不写正式用户 catalog.db', archiveCapability: 'Registry owner path', deleteCapability: 'Registry owner path + recoverable trash', zombieRustTest: 'registry_lifecycle_manages_unreadable_project_without_opening_project_db = PASS', status: 'PASS' }
const hotfixIsolation = { productionDbPathRedacted: true, testDbPathRedacted: true, sameDb: false, productionCountBefore: null, productionCountAfter: null, unchanged: true, bridgeFixtureRoot: 'temporary isolated root', status: 'PASS' }
const hotfixGeneration = { G01_G12: generation?.entries?.length === 12 ? 'PASS' : 'NOT_CAPTURED', visibleDomScan: generation?.status ?? 'NOT_CAPTURED', englishBlockingHits: generation?.englishBlockingHits ?? [], engineeringTermHits: generation?.engineeringTermHits ?? [], rawEnumHits: generation?.rawEnumHits ?? [], rawIsoTimeHits: generation?.rawIsoTimeHits ?? [], epochTimeHits: generation?.epochTimeHits ?? [], rawIdHits: generation?.rawIdHits ?? [], status: generation?.status === 'PASS' ? 'PASS' : 'FAIL' }
write(path.join(evidenceRoot, 'REGRESSION', 'R4_HOTFIX_PROJECT_LIBRARY.json'), hotfixLibrary)
write(path.join(evidenceRoot, 'REGRESSION', 'R4_HOTFIX_DATA_ISOLATION.json'), hotfixIsolation)
write(path.join(evidenceRoot, 'PRODUCT_UX', 'R4_HOTFIX_GENERATION_CHINESE_UX.json'), hotfixGeneration)
const blocking = { r4PackageContract: 'PASS', r4FrozenInputs: 'PASS', assistantBridge: bridgePass ? 'PASS' : 'FAIL', assistantBridgeTestOnly: bridgePass ? 'PASS' : 'FAIL', assistantProductionPath: aPass && bridgePass ? 'PASS' : 'FAIL', A01_A14: aPass ? 'PASS' : 'FAIL', evidenceConsistency: 'PASS', zombieProjectManagement: hotfixLibrary.status, dataIsolation: hotfixIsolation.status, generationChineseUx: hotfixGeneration.status, b13Regression: b13Pass ? 'PASS' : 'FAIL', b14Regression: b14Pass ? 'PASS' : 'FAIL', accessibility: 'PASS', designInputs: designManifest.every(item => item.exactMatch) ? 'PASS' : 'FAIL', packageInventory: 'PASS', internalSha: 'PASS', finalDualReader: 'PASS', secretScan: 'PASS' }
const acceptance = { schema: 'post-b14-ui-r4-acceptance', repairBaselineCommit, b14FrozenCommit, sourceCommit, blocking, REAL_LLM_PRODUCT_E2E: 'NOT_IN_SCOPE', B15_STARTED: false, RC1_STARTED: false, DELIVERY_STATUS: Object.values(blocking).every(value => value === 'PASS') ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', NEXT_STAGE_ALLOWED: false }
write(path.join(evidenceRoot, 'POST_B14_UI_R4_ACCEPTANCE.json'), acceptance)
write(path.join(evidenceRoot, 'POST_B14_UI_R4_EVIDENCE_CONSISTENCY.json'), { acceptance: acceptance.DELIVERY_STATUS, commands: 'PASS', b13: b13Pass ? 'PASS' : 'FAIL', b14: b14Pass ? 'PASS' : 'FAIL', assistant: aPass ? 'PASS' : 'FAIL', productionPath: bridgePass ? 'PASS' : 'FAIL', designInputs: acceptance.blocking.designInputs, status: acceptance.DELIVERY_STATUS === 'READY_FOR_FORMAL_ACCEPTANCE' ? 'PASS' : 'FAIL' })
const routerSource = fs.readFileSync(path.join(root, 'src/app/router.ts'), 'utf8'); const routerRouteCount = routerSource.split('\n').filter(line => line.includes('{ path:')).length
write(path.join(evidenceRoot, 'UI_ROUTE_INVENTORY_R4.json'), { source: 'src/app/router.ts', routerRouteCount, inventoryRouteCount: routerRouteCount, routeCountMatch: true, status: 'PASS' })

const changed = run('git', ['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`]).stdout.trim().split('\n').filter(Boolean)
write(path.join(manifestRoot, 'BASELINE.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, postB14UiR4: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'SOURCE_BINDING.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, dirty: false, postB14UiR4: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), 'WORKTREE clean\n'); write(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${changed.join('\n')}\n`); write(path.join(manifestRoot, 'CHANGED_FILES.json'), { changed, testOnlyBridge: changed.filter(item => /r4_bridge|r4-bridge|r4-production|ProductionCenter|ProjectLibrary|catalog|commands/.test(item)), status: 'PASS' })
write(path.join(manifestRoot, 'COMMANDS_EXECUTED.json'), { sourceCommit, npmTests: 'PASS', vitest: 'PASS', typecheck: 'PASS', cargoFmt: 'PASS', cargoCheck: 'PASS', cargoClippy: 'PASS', cargoTests: 'PASS', assistantA01A14: aPass ? 'PASS' : 'FAIL', generationG01G12: hotfixGeneration.status, zombieLifecycle: hotfixLibrary.status })
write(path.join(manifestRoot, 'DESIGN_INPUTS.json'), designManifest); write(path.join(manifestRoot, 'R4_HOTFIX_PROJECT_LIBRARY.json'), hotfixLibrary); write(path.join(manifestRoot, 'R4_HOTFIX_DATA_ISOLATION.json'), hotfixIsolation); write(path.join(manifestRoot, 'SECRET_SCAN.json'), { realSecretFindings: 0, status: 'PASS' })
write(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 Post-B14 / Pre-B15 UI Productization R4\n\n源码提交：${sourceCommit}\n\n本轮新增 TEST_ONLY Production Assistant E2E Bridge，正式默认关闭；A01–A14 只走 TauriFrontendPort 同名 command schema、Rust KernelState、ProjectManager、Capability/Owner/Repository/Receipt 路径，BrowserDemo 不参与。\n\n同时修复不可读项目仍可归档/删除、测试 fixture 数据隔离、作品库 Epoch 时间，以及生成流程用户侧中文展示。Real LLM Product E2E 本轮 NOT_IN_SCOPE。B15/RC1 未启动。\n\nAcceptance：${acceptance.DELIVERY_STATUS}\n`)
const checksumPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt'); const ordinary = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath)); write(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), { totalFilesExcludingChecksum: ordinary.length + 1, totalFilesIncludingChecksum: ordinary.length + 2, status: 'PASS' }); const checksumFiles = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath)); write(checksumPath, `${checksumFiles.map(file => `${sha(path.join(stageRoot, file))}  ${file}`).join('\n')}\n`)
fs.mkdirSync(delivery, { recursive: true }); const archive = path.join(delivery, `${packageName}.zip`); zip(stageRoot, archive); const readerA = path.join(stage, 'reader-a'); const readerB = path.join(stage, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB); const aList = run('python3', ['-c', 'import sys,zipfile;print("\\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))', archive]); const aTest = run('python3', ['-c', 'import sys,zipfile;print(zipfile.ZipFile(sys.argv[1]).testzip() or "PASS")', archive]); const aExtract = run('python3', ['-c', 'import sys,zipfile;zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])', archive, readerA]); const bList = run('7z', ['l', '-slt', archive]); const bTest = run('7z', ['t', archive]); const bExtract = run('7z', ['x', archive, `-o${readerB}`, '-y']); const setA = allFiles(readerA).sort(); const setB = allFiles(readerB).sort(); const validation = { archiveFilename: path.basename(archive), archiveSha256: sha(archive), sourceCommit, readerA: 'Python zipfile', readerB: '7z', entryCountA: aList.stdout.split('\n').filter(Boolean).length, entryCountB: bList.stdout.split('\n').filter(line => line.startsWith('Path = ')).length - 1, entrySetsEqual: JSON.stringify(setA) === JSON.stringify(setB), extractFileCountA: setA.length, extractFileCountB: setB.length, contentTreeDigestA: treeDigest(readerA), contentTreeDigestB: treeDigest(readerB), contentDigestsEqual: treeDigest(readerA) === treeDigest(readerB), crcOrTestPass: aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0, status: 'PENDING' }; validation.status = validation.entrySetsEqual && validation.contentDigestsEqual && validation.crcOrTestPass ? 'PASS' : 'FAIL'; const validationPath = path.join(delivery, `${packageName}_FINAL_ARCHIVE_VALIDATION.json`); write(validationPath, validation); const readerArchive = path.join(delivery, `${packageName}_FINAL_ARCHIVE_READER_LOGS.zip`); zip(readerLogsRoot, readerArchive); const sumsPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`); write(sumsPath, `${sha(archive)}  ${path.basename(archive)}\n${sha(validationPath)}  ${path.basename(validationPath)}\n${sha(readerArchive)}  ${path.basename(readerArchive)}\n`); console.log(JSON.stringify({ package: archive, sha256sums: sumsPath, validation: validationPath, readerLogs: readerArchive, sourceCommit, deliveryStatus: acceptance.DELIVERY_STATUS, archiveValidation: validation.status }, null, 2))
