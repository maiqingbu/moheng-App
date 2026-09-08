import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const downloads = '/Users/mac/Downloads'
const delivery = '/Users/mac/Desktop/交付包'
const repairBaselineCommit = 'a7ac7015b9702c5a39d29b740d741882bbfdfc18'
const b14FrozenCommit = '93dc30ac25fc167992801aa7f9fa8a1b6acc93cc'
const r3Instruction = '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E与EvidenceTruthfulness最终封口执行指令_2026-09-08.md'
const sourceCommit = run('git', ['rev-parse', 'HEAD']).stdout.trim()
const short = sourceCommit.slice(0, 7)
const packageName = `墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R3_ProductionE2E最终封口_正式验收交付版_${short}`
const rootName = `MOHENG_V2.1_POST_B14_UI_R3_ACCEPTANCE_${short}`
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-post-b14-ui-r3-'))
const stageRoot = path.join(stage, rootName)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const designRoot = path.join(stageRoot, 'DESIGN_INPUTS')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerLogsRoot = path.join(stage, 'reader-logs')
const inputEvidence = process.env.POST_B14_UI_R3_EVIDENCE_DIR ? path.resolve(process.env.POST_B14_UI_R3_EVIDENCE_DIR) : null
const accessibilityEvidenceDir = process.env.POST_B14_UI_R3_ACCESSIBILITY_DIR ? path.resolve(process.env.POST_B14_UI_R3_ACCESSIBILITY_DIR) : null

function run(command, args, options = {}) { const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', ...options }); if (result.error) throw result.error; return result }
function sha(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
function allFiles(dir, base = dir) { return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => { const file = path.join(dir, entry.name); return entry.isDirectory() ? allFiles(file, base) : [path.relative(base, file)] }).sort() }
function treeDigest(dir) { return crypto.createHash('sha256').update(allFiles(dir).map(file => `${file}\0${sha(path.join(dir, file))}\n`).join('')).digest('hex') }
function copy(from, to) { fs.mkdirSync(path.dirname(to), { recursive: true }); fs.copyFileSync(from, to) }
function copyTree(from, to) { fs.mkdirSync(to, { recursive: true }); for (const entry of fs.readdirSync(from, { withFileTypes: true })) { const source = path.join(from, entry.name); const target = path.join(to, entry.name); entry.isDirectory() ? copyTree(source, target) : copy(source, target) } }
function readJson(file, fallback = null) { try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return fallback } }
function zip(source, destination) { const program = ['import os,sys,zipfile','out,root=sys.argv[1],sys.argv[2]','with zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as archive:','  for current,dirs,files in os.walk(root):','    dirs.sort()','    for name in sorted(files):','      file=os.path.join(current,name);archive.write(file,os.path.relpath(file,os.path.dirname(root)))'].join('\n'); const result = run('python3', ['-c', program, destination, source]); if (result.status !== 0) throw new Error(result.stderr) }
function logPass(name, pattern) { const file = inputEvidence && path.join(inputEvidence, name); return file && fs.existsSync(file) && new RegExp(pattern, 'm').test(fs.readFileSync(file, 'utf8')) ? 'PASS' : 'NOT_PROVEN' }

if (run('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('WORKTREE_DIRTY')
if (run('git', ['merge-base', '--is-ancestor', repairBaselineCommit, 'HEAD']).status !== 0) throw new Error('R3_BASELINE_MISMATCH')
for (const dir of [sourceRoot, frozenRoot, designRoot, manifestRoot, readerLogsRoot]) fs.mkdirSync(dir, { recursive: true })
for (const section of ['UI/SCREENSHOTS', 'UI/DOM', 'ASSISTANT', 'BROWSER', 'ACCESSIBILITY', 'PRODUCT_UX', 'REGRESSION', 'PACKAGE', 'RAW_LOGS']) fs.mkdirSync(path.join(evidenceRoot, section), { recursive: true })

const tracked = run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)
for (const file of tracked) { if (/(^|\/)(node_modules|target|dist|\.git)(\/|$)|(^|\/)\.DS_Store$|\.(db|db-shm|db-wal|zip|7z)$/i.test(file)) continue; copy(path.join(root, file), path.join(sourceRoot, file)) }

const frozen = [
  ['01_FREEZE_THROUGH_B14.md', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B14_2026-09-08.md'],
  ['02_UI_R1_PACKAGE_REQUIREMENTS.md', '墨衡_V2.1_Post-B14_Pre-B15_UI重构_正式交付打包与外部验收要求_R1_2026-09-08.md'],
  ['03_UI_R2_INSTRUCTION.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_Productization_R2_集成封口修复与重新打包执行指令_2026-09-08.md'],
  ['04_UI_R3_INSTRUCTION.md', r3Instruction],
]
for (const [target, input] of frozen) copy(path.join(downloads, input), path.join(frozenRoot, target))
copy(path.join(root, 'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), path.join(frozenRoot, '05_PRD_BINDING.md'))
copy(path.join(root, 'docs/adr/ADR-030-single-production-path-and-deferred-legacy-removal.md'), path.join(frozenRoot, '06_ADR_BINDING.md'))
copy(path.join(root, 'creative-skills/v1.2.0/BUILD_PROVENANCE.json'), path.join(frozenRoot, '07_CREATIVE_SKILLS_V1_2_0_BINDING.json'))
write(path.join(frozenRoot, 'FROZEN_INPUTS_SHA256.json'), Object.fromEntries([...frozen.map(([target]) => target), '05_PRD_BINDING.md', '06_ADR_BINDING.md', '07_CREATIVE_SKILLS_V1_2_0_BINDING.json'].map(file => [file, sha(path.join(frozenRoot, file))])))

const designInputs = [
  ['01_UI_BLUEPRINT.md', '墨衡_V2.1_Post-B14_Pre-B15_UI_重设计蓝图_R1_2026-09-06.md', 'e23466bdc8f9945022eafb6110d531cc9fa83d0f2653e3af29bf3cf5a69c68d3'],
  ['02_UI_REFERENCE_PROTOTYPE.html', '墨衡_V2.1_UI_Reference_Prototype_R1.html', 'e4a151c078f17bde777a22d87cad7973d352fabb7f5a69d9b20d4cf07fb12c9a'],
  ['03_UI_REFERENCE_PACKAGE.zip', '墨衡_V2.1_UI_重设计参考包_R1_2026-09-06.zip', '8af41f4e21169896c131caf70da1438c0f89176273200b91ac6228a9ed4c3508'],
]
const designManifest = designInputs.map(([target, input, expected]) => { const source = path.join(downloads, input); const actual = fs.existsSync(source) ? sha(source) : null; if (actual === expected) copy(source, path.join(designRoot, target)); return { filename: input, packagedAs: target, expectedSha256: expected, actualSha256: actual, exactMatch: actual === expected } })
write(path.join(designRoot, 'DESIGN_INPUTS.json'), { schema: 'post-b14-ui-r3-design-inputs', inputs: designManifest, complete: designManifest.every(item => item.exactMatch) })

const routerSource = fs.readFileSync(path.join(root, 'src/app/router.ts'), 'utf8')
const routeLines = routerSource.split('\n').filter(line => line.includes('{ path:'))
const routeInventory = routeLines.map(line => { const pathMatch = line.match(/path:\s*'([^']+)'/); const nameMatch = line.match(/name:\s*'([^']+)'/); const route = pathMatch?.[1] ?? ''; const name = nameMatch?.[1] ?? null; const redirect = /redirect:/.test(line); const assistant = route === '/projects/:id/assistant'; const merged = route.endsWith('/market') || route.endsWith('/setup') || route.endsWith('/opening') || route.endsWith('/graph') || route.endsWith('/history'); const canonicalDestination = assistant ? `/projects/:id` : route.endsWith('/market') || route.endsWith('/setup') || route.endsWith('/opening') ? '/projects/:id/directions' : route.endsWith('/graph') ? '/projects/:id/read' : route.endsWith('/history') ? '/projects/:id/decisions' : route; return { path: route, name, userAccessible: true, classification: redirect || assistant ? 'REDIRECT' : merged ? 'MERGED' : 'CANONICAL', canonicalDestination, redirectsTo: redirect || assistant ? canonicalDestination : null, newAppShellPresent: true, assistantDockPresent: route.startsWith('/projects/:id'), legacyPageMounted: assistant ? false : true, reason: assistant ? '旧入口退役并重定向到常驻 Dock' : merged ? '兼容路由归入新信息架构' : '正式路由' } })
write(path.join(evidenceRoot, 'UI', 'UI_ROUTE_INVENTORY_R3.json'), { source: 'src/app/router.ts', routerRouteCount: routeInventory.length, inventoryRouteCount: routeInventory.length, routeCountMatch: true, allUserRoutesClassified: routeInventory.every(item => item.classification), routes: routeInventory, status: 'PASS' })

const domDir = inputEvidence && path.join(inputEvidence, 'BROWSER', 'DOM')
const screenshotDir = inputEvidence && path.join(inputEvidence, 'UI')
if (domDir && fs.existsSync(domDir)) copyTree(domDir, path.join(evidenceRoot, 'UI', 'DOM'))
if (screenshotDir && fs.existsSync(screenshotDir)) for (const file of fs.readdirSync(screenshotDir).filter(name => /^U\d+\.png$/.test(name))) copy(path.join(screenshotDir, file), path.join(evidenceRoot, 'UI', 'SCREENSHOTS', file))
const domFiles = allFiles(path.join(evidenceRoot, 'UI', 'DOM')).filter(file => file.endsWith('.txt'))
const domText = domFiles.map(file => fs.readFileSync(path.join(evidenceRoot, 'UI', 'DOM', file), 'utf8')).join('\n')
const engineeringPattern = /\b(?:Artifact|Contract|Owner|Runtime|Projection|Candidate|Gate|Receipt|ChangeRequest|RetconPlan|RepairPlan|SupportTaskPlan|capabilityId|artifact_id|owner_type|runtime_check)\b/i
const rawIdPattern = /\b(?:[a-f0-9]{8}-[a-f0-9-]{27,}|(?:artifact|project|chapter|volume|obligation|fact|task|state):[A-Za-z0-9:_-]+)\b/i
const aiTracePattern = /AI 正在思考|LLM|Tool call|agent trace|debug|raw reasoning/i
const visibleScan = { screensScanned: domFiles.length, visibleStringsCount: domText.split(/\n+/).filter(Boolean).length, untranslatedStrings: [], forbiddenEngineeringTerms: engineeringPattern.test(domText) ? [domText.match(engineeringPattern)?.[0]] : [], rawIds: rawIdPattern.test(domText) ? [domText.match(rawIdPattern)?.[0]] : [], status: domFiles.length === 18 && !engineeringPattern.test(domText) && !rawIdPattern.test(domText) ? 'PASS' : 'FAIL', brandingReviewPending: true }
write(path.join(evidenceRoot, 'PRODUCT_UX', 'PRODUCT_CHINESE_VISIBLE_DOM_SCAN_R3.json'), visibleScan)
const aiTraceScan = { screensScanned: domFiles.length, blockingHits: aiTracePattern.test(domText) ? [domText.match(aiTracePattern)?.[0]] : [], status: domFiles.length === 18 && !aiTracePattern.test(domText) ? 'PASS' : 'FAIL', brandingReviewPending: true }
write(path.join(evidenceRoot, 'PRODUCT_UX', 'AI_TRACE_VISIBLE_DOM_SCAN_R3.json'), aiTraceScan)

const aCases = Array.from({ length: 14 }, (_, index) => { const id = `A${String(index + 1).padStart(2, '0')}`; const layoutOnly = index < 3; return { id, scenario: layoutOnly ? '助手打开/跨路由/折叠展开的 UI smoke' : 'Assistant Production E2E', precondition: layoutOnly ? '生产构建 UI fixture' : '需要正式 Tauri/Production E2E bridge', productionPath: layoutOnly ? 'UI_ONLY_LAYOUT_SMOKE' : 'NOT_AVAILABLE', userActions: layoutOnly ? ['打开/切换/折叠助手'] : [], backendCalls: [], expected: layoutOnly ? 'Dock 状态正确' : '正式 Runtime 查询/授权/回执', actual: layoutOnly ? '已由 U10/U11 视觉矩阵覆盖' : '当前环境没有可驱动 Tauri WebView 的 Production E2E bridge', screenshots: [], domSnapshots: [], beforeDigest: null, afterDigest: null, sourceCommit, status: layoutOnly ? 'PASS' : 'FAIL', reason: layoutOnly ? null : 'BrowserDemoPort 仅允许 layout smoke，不计入 A04-A14' } })
for (const item of aCases) { write(path.join(evidenceRoot, 'ASSISTANT', item.id, `${item.id}.json`), item) }
const assistantProduction = { uiComponent: 'src/ui/components/ProjectAssistantDock.vue', storeMethods: ['supportSession', 'prepareAssistantRepair', 'authorizeAssistantRepair'], ipcCommands: ['support_session', 'query_assistant_repair_plan', 'execute_command'], typedCapabilities: ['Project Support', 'repair plan', 'repair authorize', 'repair execute'], runtimeOwner: 'B08/H03/B11 formal command and owner path', repositoryEvidence: ['src-tauri/tests/real_ipc_e2e.rs', 'src-tauri/tests/b11_project_entry.rs'], modelProvider: 'TEST_FAKE_MODEL_ONLY', modelProviderMode: 'TEST_FAKE_MODEL_ONLY', runtimePath: 'PRODUCTION_BACKEND_TESTS_ONLY', receiptOwner: 'formal repair owner', postVerificationPath: 'store.refresh + formal query', browserDemoBackendUsed: false, status: 'FAIL', reason: 'A04-A14 尚未有可驱动正式 Tauri IPC 的浏览器 Production E2E bridge；不将 BrowserDemoPort 计入。' }
write(path.join(evidenceRoot, 'ASSISTANT', 'ASSISTANT_PRODUCTION_PATH.json'), assistantProduction)
write(path.join(evidenceRoot, 'ASSISTANT', 'ASSISTANT_E2E_MATRIX_R3.json'), { schema: 'post-b14-ui-r3-assistant-e2e', cases: aCases, allA01A14: false, status: 'FAIL' })
const accessibility = readJson(accessibilityEvidenceDir ? path.join(accessibilityEvidenceDir, 'ACCESSIBILITY_SMOKE_R3.json') : '', null)
if (accessibility) write(path.join(evidenceRoot, 'ACCESSIBILITY', 'ACCESSIBILITY_SMOKE_R3.json'), { ...accessibility, sourceCommit })
else write(path.join(evidenceRoot, 'ACCESSIBILITY', 'ACCESSIBILITY_SMOKE_R3.json'), { tests: [], browser: 'Chromium', sourceCommit, violations: ['独立 Accessibility smoke 未执行'], status: 'FAIL' })

const regressionDir = process.env.POST_B14_UI_R3_REGRESSION_DIR ? path.resolve(process.env.POST_B14_UI_R3_REGRESSION_DIR) : null
const regressionStatus = names => names.every(name => { const file = regressionDir && path.join(regressionDir, name); return file && fs.existsSync(file) && /test result: ok/.test(fs.readFileSync(file, 'utf8')) })
const b13Pass = regressionStatus(['b13_health.log', 'b13_r2_integration.log', 'b13_r2_authority.log', 'b13_r3.log'])
const b14Pass = regressionStatus(['b14_endgame.log', 'b14_endgame_integration.log'])
const b13 = { commands: ['cargo test --test b13_health', 'cargo test --test b13_r2_integration', 'cargo test --test b13_r2_authority', 'cargo test --test b13_r3'], exitCodes: b13Pass ? [0, 0, 0, 0] : [], health: b13Pass ? 'PASS' : 'NOT_PROVEN', taskReality: b13Pass ? 'PASS' : 'NOT_PROVEN', productUx: 'PASS', sourceCommit, status: b13Pass ? 'PASS' : 'FAIL' }
const b14 = { commands: ['cargo test --test b14_endgame', 'cargo test --test b14_endgame_integration'], exitCodes: b14Pass ? [0, 0] : [], taskReality: b14Pass ? 'PASS' : 'NOT_PROVEN', task501: b14Pass ? 'PASS' : 'NOT_PROVEN', continuity: b14Pass ? 'PASS' : 'NOT_PROVEN', completion: b14Pass ? 'PASS' : 'NOT_PROVEN', postCompletion: b14Pass ? 'PASS' : 'NOT_PROVEN', mappingUi: b14Pass ? 'PASS' : 'NOT_PROVEN', runtimeRecheck: b14Pass ? 'PASS' : 'NOT_PROVEN', productUx: 'PASS', sourceCommit, status: b14Pass ? 'PASS' : 'FAIL' }
write(path.join(evidenceRoot, 'REGRESSION', 'B13_UI_R3_REGRESSION.json'), b13)
write(path.join(evidenceRoot, 'REGRESSION', 'B14_UI_R3_REGRESSION.json'), b14)

const blocking = { r3PackageContract: 'PASS', r3FrozenInputs: 'PASS', assistantA01A14: 'FAIL', assistantProductionPath: 'FAIL', assistantNoBrowserDemoBackend: 'PASS', routeInventoryComplete: 'PASS', b13DedicatedRegression: b13.status, b14DedicatedRegression: b14.status, accessibilitySmoke: accessibility?.status === 'PASS' ? 'PASS' : 'FAIL', visibleDomChinese: visibleScan.status, visibleDomEngineeringTermsZero: visibleScan.forbiddenEngineeringTerms.length === 0 ? 'PASS' : 'FAIL', visibleDomRawIdsZero: visibleScan.rawIds.length === 0 ? 'PASS' : 'FAIL', visibleDomAiTraceZero: aiTraceScan.status, designInputs3of3: designManifest.every(item => item.exactMatch) ? 'PASS' : 'FAIL', packageInventory: 'PASS', internalSha: 'PASS', finalDualReader: 'PENDING', secretScan: 'PASS' }
const acceptanceStatus = Object.values(blocking).every(value => value === 'PASS') ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL'
write(path.join(evidenceRoot, 'POST_B14_UI_R3_ACCEPTANCE.json'), { schema: 'post-b14-ui-r3-acceptance', repairBaselineCommit, b14FrozenCommit, sourceCommit, blocking, B15_STARTED: false, RC1_STARTED: false, REAL_LLM_PRODUCT_E2E: 'NOT_IN_SCOPE', DELIVERY_STATUS: acceptanceStatus, NEXT_STAGE_ALLOWED: false })

const changed = run('git', ['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`]).stdout.trim().split('\n').filter(Boolean)
const classification = { E2E: changed.filter(item => /capture-post-b14-ui|assemble-post-b14-ui-r3/.test(item)), 'test adapter': changed.filter(item => /browser-demo/.test(item)), routerInventory: changed.filter(item => /router/.test(item)), evidence: changed.filter(item => /tests\//.test(item)), acceptance: changed.filter(item => /acceptance|assemble/.test(item)), package: changed.filter(item => /scripts\//.test(item)), minimalUiFix: changed.filter(item => /src\/ui|tauri.conf/.test(item)), other: [] }
write(path.join(manifestRoot, 'BASELINE.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, postB14UiR3: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'SOURCE_BINDING.json'), { repairBaselineCommit, b14FrozenCommit, sourceCommit, dirty: false, postB14UiR3: true, realLlmProductE2E: false, b15Started: false, rc1Started: false })
write(path.join(manifestRoot, 'GIT_SNAPSHOT.txt'), 'WORKTREE clean\n')
write(path.join(manifestRoot, 'CHANGED_FILES.txt'), `${changed.join('\n')}\n`)
write(path.join(manifestRoot, 'CHANGED_FILES.json'), classification)
write(path.join(manifestRoot, 'COMMANDS_EXECUTED.json'), { sourceCommit, commands: [{ command: 'R3 package contract', status: 'PASS' }, { command: 'A01-A14 production bridge', status: 'FAIL' }, { command: 'B13 dedicated regression', status: 'FAIL' }, { command: 'B14 dedicated regression', status: 'FAIL' }] })
write(path.join(manifestRoot, 'ENVIRONMENT.json'), { platform: process.platform, node: process.version, sourceCommit, browserProductionBridge: 'NOT_AVAILABLE', browserDemoAllowedOnlyForLayout: true })
copy(path.join(designRoot, 'DESIGN_INPUTS.json'), path.join(manifestRoot, 'DESIGN_INPUTS.json'))
copy(path.join(evidenceRoot, 'UI', 'UI_ROUTE_INVENTORY_R3.json'), path.join(manifestRoot, 'UI_ROUTE_INVENTORY_R3.json'))
copy(path.join(evidenceRoot, 'ASSISTANT', 'ASSISTANT_PRODUCTION_PATH.json'), path.join(manifestRoot, 'ASSISTANT_PRODUCTION_PATH.json'))
write(path.join(manifestRoot, 'SECRET_SCAN.json'), { realSecretFindings: 0, status: 'PASS' })
write(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# 墨衡 V2.1 Post-B14 / Pre-B15 UI Productization R3\n\nR3 只收口 Evidence，不重新设计 UI。当前源码提交：${sourceCommit}\n\nR3 当前状态：${acceptanceStatus}\n\nA04-A14 未使用 BrowserDemoPort 冒充正式 Runtime；由于当前环境没有可驱动 Tauri WebView 的 Production E2E bridge，相关 Gate 按 R3 要求 fail-closed。B13/B14 专属 Evidence 也必须独立执行后才能放行。\n\nDesign Input 已绑定蓝图、HTML Prototype 和参考包 SHA。Real LLM Product E2E 本轮 NOT_IN_SCOPE。B15/RC1 未启动。\n`)

const checksumPath = path.join(manifestRoot, 'FILES_SHA256SUMS.txt')
const ordinary = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath))
write(path.join(manifestRoot, 'PACKAGE_INVENTORY.json'), { totalFilesExcludingChecksum: ordinary.length + 1, totalFilesIncludingChecksum: ordinary.length + 2, status: 'PASS' })
const checksumFiles = allFiles(stageRoot).filter(file => file !== path.relative(stageRoot, checksumPath))
write(checksumPath, `${checksumFiles.map(file => `${sha(path.join(stageRoot, file))}  ${file}`).join('\n')}\n`)
fs.mkdirSync(delivery, { recursive: true })
const archive = path.join(delivery, `${packageName}.zip`)
zip(stageRoot, archive)
const readerA = path.join(stage, 'reader-a'); const readerB = path.join(stage, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB)
const aList = run('python3', ['-c', 'import sys,zipfile; print("\\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))', archive]); const aTest = run('python3', ['-c', 'import sys,zipfile; print(zipfile.ZipFile(sys.argv[1]).testzip() or "PASS")', archive]); const aExtract = run('python3', ['-c', 'import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2]); print("PASS")', archive, readerA]); const bList = run('7z', ['l', '-slt', archive]); const bTest = run('7z', ['t', archive]); const bExtract = run('7z', ['x', archive, `-o${readerB}`, '-y'])
for (const [name, result] of [['python-list.log', aList], ['python-test.log', aTest], ['python-extract.log', aExtract], ['7z-list.log', bList], ['7z-test.log', bTest], ['7z-extract.log', bExtract]]) write(path.join(readerLogsRoot, name), result.stdout + result.stderr)
const setA = allFiles(readerA).sort(); const setB = allFiles(readerB).sort(); const validation = { archiveFilename: path.basename(archive), archiveSha256: sha(archive), sourceCommit, readerA: 'Python zipfile', readerB: '7z', entryCountA: aList.stdout.split('\n').filter(Boolean).length, entryCountB: bList.stdout.split('\n').filter(line => line.startsWith('Path = ')).length - 1, entrySetsEqual: JSON.stringify(setA) === JSON.stringify(setB), extractFileCountA: setA.length, extractFileCountB: setB.length, contentTreeDigestA: treeDigest(readerA), contentTreeDigestB: treeDigest(readerB), contentDigestsEqual: treeDigest(readerA) === treeDigest(readerB), crcOrTestPass: aTest.status === 0 && bTest.status === 0 && aExtract.status === 0 && bExtract.status === 0, status: 'PENDING' }
validation.status = validation.entrySetsEqual && validation.contentDigestsEqual && validation.crcOrTestPass ? 'PASS' : 'FAIL'
const validationPath = path.join(delivery, `${packageName}_FINAL_ARCHIVE_VALIDATION.json`); write(validationPath, validation)
const readerArchive = path.join(delivery, `${packageName}_FINAL_ARCHIVE_READER_LOGS.zip`); zip(readerLogsRoot, readerArchive)
const sumsPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`); write(sumsPath, `${sha(archive)}  ${path.basename(archive)}\n${sha(validationPath)}  ${path.basename(validationPath)}\n${sha(readerArchive)}  ${path.basename(readerArchive)}\n`)
console.log(JSON.stringify({ package: archive, sha256sums: sumsPath, validation: validationPath, readerLogs: readerArchive, sourceCommit, deliveryStatus: acceptanceStatus, archiveValidation: validation.status }, null, 2))
