import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { evaluateProductUx } from './verify-product-ux.mjs'
import { evaluateForbiddenScopeDiff } from './verify-b13-r2-forbidden-scope.mjs'
import { inspectB13R3Architecture } from './verify-b13-r3-architecture.mjs'
import { evaluateR3EvidenceTruthfulness } from './verify-b13-r3-evidence-truthfulness.mjs'
import { generateR3Acceptance } from './generate-b13-r3-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R3_EVIDENCE_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-evidence-')))
const r3Evidence = path.join(evidenceDir, 'B13_R3')
const automaticDir = path.join(r3Evidence, 'AUTOMATIC')
const integrationDir = path.join(r3Evidence, 'INTEGRATION')
const performanceDir = path.join(r3Evidence, 'PERFORMANCE')
const manualDir = path.join(r3Evidence, 'MANUAL_BROWSER')
const mutationDir = path.join(r3Evidence, 'MUTATIONS')
const readerDir = path.join(r3Evidence, 'PACKAGE_READERS')
const repairBaselineShort = 'e6419ce'
const repairBaselineCommit = spawnSync('git', ['rev-parse', repairBaselineShort + '^{commit}'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'

for (const directory of [automaticDir, integrationDir, performanceDir, manualDir, mutationDir, readerDir]) fs.mkdirSync(directory, { recursive: true })

function git(args) {
  return spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 }).stdout.trim()
}

const sourceCommit = git(['rev-parse', 'HEAD'])
const statusBefore = git(['status', '--porcelain', '--untracked-files=all'])
const commands = {}

function run(name, command, args, extraEnv = {}) {
  const startedAt = new Date().toISOString()
  const started = Date.now()
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || '/Users/mac/Library/Caches/moheng-product-ux-cargo-target',
      ...extraEnv,
    },
    maxBuffer: 256 * 1024 * 1024,
  })
  const output = (result.stdout || '') + (result.stderr || '')
  const record = {
    name,
    command: [command, ...args].join(' '),
    startedAt,
    finishedAt: new Date().toISOString(),
    durationMs: Date.now() - started,
    exitCode: result.status === null ? 1 : result.status,
    status: result.status === 0 ? 'PASS' : 'FAIL',
    logPath: 'B13_R3/AUTOMATIC/' + name + '.log',
    sourceCommit,
  }
  fs.writeFileSync(path.join(evidenceDir, record.logPath), output)
  commands[name] = record
  return { ...record, output }
}

function writeRoot(name, value) {
  fs.writeFileSync(path.join(evidenceDir, name), JSON.stringify(value, null, 2) + '\n')
}

function writeEvidence(name, value, command = null) {
  writeRoot(name, {
    ...value,
    sourceCommit,
    repairBaselineCommit,
    command: command?.command || null,
    startedAt: command?.startedAt || null,
    finishedAt: command?.finishedAt || null,
    exitCode: command?.exitCode ?? 0,
    generatedAt: new Date().toISOString(),
  })
}

function readJson(file) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return null }
}

function parseJsonOutput(output) {
  try { return JSON.parse(output.trim()) } catch { return null }
}

function marker(output, prefix) {
  return output.split('\n').find(line => line.startsWith(prefix)) || null
}

function field(line, name) {
  const match = line?.match(new RegExp('(?:^|\\s)' + name + '=([^\\s]+)'))
  return match ? match[1].replace(/^"|"$/g, '') : null
}

function boolField(line, name) {
  return field(line, name) === 'true'
}

function numberField(line, name) {
  const value = field(line, name)
  return value === null ? null : Number(value)
}

function writeQueryPlanLogs(plans) {
  const files = {
    formal_route_source: 'formal-route.explain.txt',
    planning_state_projection_lookup: 'planning-state.explain.txt',
    volume_plan_payload_lookup: 'volume-plan-payload.explain.txt',
    task_state_chunk_le_100: 'task-state-chunk.explain.txt',
  }
  for (const plan of plans) {
    const file = files[plan.queryName]
    if (!file) continue
    fs.writeFileSync(path.join(performanceDir, file), [
      'sourceCommit=' + sourceCommit,
      'repairBaselineCommit=' + repairBaselineCommit,
      'queryName=' + plan.queryName,
      'sqlDigest=' + plan.sqlDigest,
      'fixture=' + plan.fixture,
      'sql=' + plan.sql,
      'rawExplainRows=' + JSON.stringify(plan.rawExplainRows),
      '',
    ].join('\n'))
  }
}

function mapChangedFile(file) {
  if (/src-tauri\/src\/(?:repositories\/narrative|repositories\/health|volume_lifecycle)|tests\/b13_r3/.test(file)) return 'B13-R3-01'
  if (/VolumeManager\.vue|verify-product-ux|product-ux|tests\/product-ux/.test(file)) return 'B13-R3-02'
  if (/b13-r3|package|verify-package-consistency|generate-b13-r3|run-b13-r3|assemble-b13-r3/.test(file)) return 'B13-R3-03'
  return 'OUT_OF_SCOPE'
}

function copyFileIfPresent(source, destination) {
  if (!fs.existsSync(source)) return false
  fs.mkdirSync(path.dirname(destination), { recursive: true })
  fs.copyFileSync(source, destination)
  return true
}

function copyReaderEvidence(stageRoot) {
  const stageReader = path.join(stageRoot, 'EVIDENCE', 'B13_R3', 'PACKAGE_READERS')
  if (!fs.existsSync(stageReader)) return
  for (const name of fs.readdirSync(stageReader)) {
    copyFileIfPresent(path.join(stageReader, name), path.join(readerDir, name))
  }
}

function main() {
  if (!repairBaselineCommit || repairBaselineCommit.length !== 40) throw new Error('B13_R3_REPAIR_BASELINE_RESOLUTION_FAILED')
  writeEvidence('B13_R3_PREBATCH_FREEZE.json', {
    schema: 'b13-r3-prebatch-freeze',
    batch: 'B13-R3',
    repairBaselineShort,
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    b01ToB13Frozen: true,
    doNotStart: ['B14', 'B15', 'RC1'],
    status: statusBefore === '' && sourceCommit !== repairBaselineCommit ? 'PASS' : 'FAIL',
  })

  const architecture = run('architecture', 'node', ['scripts/verify-b13-r3-architecture.mjs', repairBaselineCommit])
  const architectureReport = parseJsonOutput(architecture.output) || inspectB13R3Architecture(root, repairBaselineCommit)
  const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
  const productUx = run('product_ux', 'node', ['scripts/verify-product-ux.mjs'])
  const r3Node = run('r3_node', 'node', ['--test', 'tests/b13-r3-acceptance.test.mjs'])
  const node = run('node', 'npm', ['test'])
  const vitest = run('vitest', 'npm', ['run', 'test:store'])
  const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
  const build = run('build', 'npm', ['run', 'build'])
  const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
  const verify = run('verify', 'npm', ['run', 'verify'])
  const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
  const productUxFormal = run('product_ux_formal', 'npm', ['run', 'verify:product-ux'])
  const b10Boundary = run('b10_boundary', 'node', ['scripts/verify-b10-architecture.mjs'])
  const b12Boundary = run('b12_boundary', 'node', ['scripts/verify-b12-architecture.mjs'])

  const r3Targeted = run('r3_targeted', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_r3', '--', '--nocapture'])
  const taskMarker = marker(r3Targeted.output, 'B13_R3_TASK_REALITY_RESULT')
  const integrationMarker = marker(r3Targeted.output, 'B13_R3_501_FORMAL_INTEGRATION_RESULT')
  const b09Marker = marker(r3Targeted.output, 'B13_R3_B09_SEMANTICS_RESULT')
  const plans = r3Targeted.output.split('\n')
    .filter(line => line.startsWith('B13_R3_QUERY_PLAN '))
    .map(line => parseJsonOutput(line.slice('B13_R3_QUERY_PLAN '.length)))
    .filter(Boolean)
  writeQueryPlanLogs(plans)
  writeEvidence('B13_R3_TASK_REALITY_COMPLETE_READ.json', {
    schema: 'b13-r3-task-reality-complete-read',
    requestedTaskIds: numberField(taskMarker, 'requestedTaskIds'),
    uniqueTaskIds: numberField(taskMarker, 'uniqueTaskIds'),
    chunkSize: numberField(taskMarker, 'chunkSize'),
    chunkCount: numberField(taskMarker, 'chunkCount'),
    legacyReturnedStates: numberField(taskMarker, 'legacyReturnedStates'),
    legacyIncludesTask501: boolField(taskMarker, 'legacyIncludesTask501'),
    returnedTaskStates: numberField(taskMarker, 'returnedTaskStates'),
    blockedRequiredTasks: numberField(taskMarker, 'blockedRequiredTasks'),
    h04Severity: field(taskMarker, 'h04Severity'),
    b09BlockedRequiredTasks: numberField(taskMarker, 'b09BlockedRequiredTasks'),
    task100Chunks: numberField(taskMarker, 'task100Chunks'),
    task101Chunks: numberField(taskMarker, 'task101Chunks'),
    duplicateReturnedStates: numberField(taskMarker, 'duplicateReturnedStates'),
    missingStatePreserved: boolField(taskMarker, 'missingStatePreserved'),
    pageIndependent: field(taskMarker, 'pageIndependent') === 'PASS',
    sharedReader: 'task_states_by_ids_complete',
    status: r3Targeted.status === 'PASS' && Boolean(taskMarker) ? 'PASS' : 'FAIL',
  }, r3Targeted)
  writeEvidence('B13_R3_501_FORMAL_INTEGRATION.json', {
    schema: 'b13-r3-501-formal-integration',
    persistence: field(integrationMarker, 'persistence'),
    stateRows: numberField(integrationMarker, 'stateRows'),
    b09: field(integrationMarker, 'b09'),
    queryBookHealth: field(integrationMarker, 'queryBookHealth'),
    h04: field(integrationMarker, 'h04'),
    pageIndependent: field(integrationMarker, 'pageIndependent'),
    status: r3Targeted.status === 'PASS' && Boolean(integrationMarker) ? 'PASS' : 'FAIL',
  }, r3Targeted)
  writeEvidence('B13_R3_B09_SEMANTICS_UNCHANGED.json', {
    schema: 'b13-r3-b09-semantics-unchanged',
    owner: field(b09Marker, 'owner'),
    sharedReader: field(b09Marker, 'sharedReader'),
    blockedRequiredTasks: numberField(b09Marker, 'blockedRequiredTasks'),
    semanticsUnchanged: boolField(b09Marker, 'semanticsUnchanged'),
    status: r3Targeted.status === 'PASS' && Boolean(b09Marker) ? 'PASS' : 'FAIL',
  }, r3Targeted)
  writeEvidence('B13_R3_CURRENT_QUERY_PLANS.json', {
    schema: 'b13-r3-current-query-plans',
    plans,
    requiredQueryNames: ['formal_route_source', 'planning_state_projection_lookup', 'volume_plan_payload_lookup', 'task_state_chunk_le_100', 'health_source_time_aggregate'],
    status: r3Targeted.status === 'PASS' && plans.length === 5 ? 'PASS' : 'FAIL',
  }, r3Targeted)

  const browserScratch = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-browser-'))
  const browser = run('manual_browser', 'node', ['scripts/capture-b13-r1-browser-evidence.mjs'], {
    B13_R1_EVIDENCE_DIR: browserScratch,
    B13_R1_SOURCE_COMMIT: sourceCommit,
  })
  const walkthrough = readJson(path.join(browserScratch, 'B13_R1_MANUAL_WALKTHROUGH.json'))
  const m11 = walkthrough?.items?.find(item => item.id === 'M11')
  const m11Text = m11?.actualVisibleText || ''
  const bookPlanVisibleCount = (m11Text.match(/\bBookPlan\b/g) || []).length
  const volumePlanVisibleCount = (m11Text.match(/\bVolumePlan\b/g) || []).length
  copyFileIfPresent(path.join(browserScratch, 'MANUAL_BROWSER', 'M11.png'), path.join(manualDir, 'M11.png'))
  fs.writeFileSync(path.join(manualDir, 'M11.json'), JSON.stringify({
    schema: 'b13-r3-manual-browser-m11',
    sourceCommit,
    repairBaselineCommit,
    route: m11?.route || null,
    actualVisibleText: m11Text,
    actualVisibleTextSource: m11?.actualVisibleTextSource || null,
    domTextSha256: m11?.domTextSha256 || null,
    bookPlanVisibleCount,
    volumePlanVisibleCount,
    internalLeakageCheck: m11?.internalLeakageCheck === true && bookPlanVisibleCount === 0 && volumePlanVisibleCount === 0,
    screenshotPath: 'B13_R3/MANUAL_BROWSER/M11.png',
    result: m11 && m11.internalLeakageCheck === true && bookPlanVisibleCount === 0 && volumePlanVisibleCount === 0 ? 'PASS' : 'FAIL',
  }, null, 2) + '\n')
  writeEvidence('B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json', {
    schema: 'b13-r3-product-ux-bookplan-volumeplan',
    beforeText: 'BookPlan 只保存全书战略；详细 VolumePlan 按当前卷创建，不一次生成整书章纲。',
    afterText: '全书规划只保存整体战略；详细卷计划按当前卷逐步建立，不会一次生成整本书的章节规划。',
    actualVisibleText: m11Text,
    bookPlanVisibleCount,
    volumePlanVisibleCount,
    m11Route: m11?.route || null,
    m11Result: m11?.result || 'FAIL',
    internalLeakageCheck: bookPlanVisibleCount === 0 && volumePlanVisibleCount === 0,
    productUxGate: productUxFormal.status === 'PASS' && productUx.status === 'PASS' ? 'PASS' : 'FAIL',
    mutationIntroducedExitCode: null,
    mutationRevertedExitCode: null,
    screenshotPath: 'B13_R3/MANUAL_BROWSER/M11.png',
    status: browser.status === 'PASS' && m11?.result === 'PASS' && bookPlanVisibleCount === 0 && volumePlanVisibleCount === 0 ? 'PASS' : 'FAIL',
  }, browser)

  const r2EvidenceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-r2-regression-'))
  const r2Regression = run('r2_regression', 'npm', ['run', 'verify:b13-r2'], { B13_R2_EVIDENCE_DIR: r2EvidenceDir })
  const r2Authority = readJson(path.join(r2EvidenceDir, 'B13_R2_NO_AUTHORITY_MUTATION.json'))
  const r2Remote = readJson(path.join(r2EvidenceDir, 'B13_R2_NO_REMOTE_CALL.json'))
  const historicalEvidenceFiles = ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']
  const historicalRestoration = historicalEvidenceFiles.map(file => {
    const changed = spawnSync('git', ['diff', '--quiet', '--', file], { cwd: root }).status !== 0
    if (!changed) return { file, changed: false, status: 'UNCHANGED' }
    const restored = spawnSync('git', ['restore', '--source', sourceCommit, '--', file], { cwd: root, encoding: 'utf8' })
    return { file, changed: true, status: restored.status === 0 ? 'RESTORED' : 'FAIL' }
  })
  writeRoot('B13_R3_HISTORICAL_EVIDENCE_RESTORATION.json', {
    schema: 'b13-r3-historical-evidence-restoration',
    files: historicalRestoration,
    status: historicalRestoration.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL',
  })
  writeRoot('B13_R3_NO_AUTHORITY_MUTATION.json', {
    schema: 'b13-r3-no-authority-mutation',
    sourceCommit,
    repairBaselineCommit,
    domains: r2Authority?.domains || [],
    projectionChanges: r2Authority?.projectionChanges ?? null,
    authorityFixtureNonEmpty: r2Authority?.authorityFixtureNonEmpty ?? false,
    status: r2Regression.status === 'PASS' && r2Authority?.status === 'PASS' ? 'PASS' : 'FAIL',
  })
  writeRoot('B13_R3_NO_REMOTE_CALL.json', {
    schema: 'b13-r3-no-remote-call',
    sourceCommit,
    repairBaselineCommit,
    modelRunsBefore: r2Remote?.modelRunsBefore ?? null,
    modelRunsAfter: r2Remote?.modelRunsAfter ?? null,
    status: r2Regression.status === 'PASS' && r2Remote?.status === 'PASS' ? 'PASS' : 'FAIL',
  })
  if (fs.existsSync(r2EvidenceDir)) fs.cpSync(r2EvidenceDir, path.join(evidenceDir, 'HISTORICAL', 'B13_R2'), { recursive: true })

  const changed = git(['diff', '--name-status', repairBaselineCommit + '..' + sourceCommit])
    .split('\n')
    .filter(Boolean)
    .map(line => {
      const [status, ...parts] = line.split('\t')
      const file = parts.at(-1)
      return { status, file, task: mapChangedFile(file || '') }
    })
  const outOfScope = changed.filter(item => item.task === 'OUT_OF_SCOPE')
  writeRoot('B13_R3_CHANGED_FILES.json', {
    schema: 'b13-r3-changed-files',
    sourceCommit,
    repairBaselineCommit,
    paths: changed,
    outOfScope,
    status: outOfScope.length === 0 ? 'PASS' : 'FAIL',
  })
  const forbidden = evaluateForbiddenScopeDiff(root, repairBaselineCommit)
  writeRoot('B13_R3_FORBIDDEN_SCOPE.json', {
    ...forbidden,
    sourceCommit,
    repairBaselineCommit,
    status: forbidden.status === 'PASS' ? 'PASS' : 'FAIL',
  })
  writeEvidence('B13_R3_ARCHITECTURE.json', {
    ...architectureReport,
    schema: 'b13-r3-architecture-evidence',
    status: architecture.status === 'PASS' && architectureReport.status === 'PASS' ? 'PASS' : 'FAIL',
  }, architecture)

  const cleanBeforePackage = git(['status', '--porcelain', '--untracked-files=all'])
  writeRoot('B13_R3_SOURCE_BINDING.json', {
    schema: 'b13-r3-source-binding',
    batch: 'B13-R3',
    startCommit: sourceCommit,
    repairBaselineShort,
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    evidenceSourceCommit: sourceCommit,
    sourceCommitAfter: git(['rev-parse', 'HEAD']),
    workingTreeDirtyBefore: statusBefore !== '',
    workingTreeDirtyAfter: cleanBeforePackage !== '',
    excludedUncommittedPaths: [],
    status: statusBefore === '' && cleanBeforePackage === '' && git(['rev-parse', 'HEAD']) === sourceCommit ? 'PASS' : 'FAIL',
  })

  const preflight = run('package_preflight', 'node', ['scripts/assemble-b13-r3-package.mjs', '--preflight'], { B13_R3_EVIDENCE_DIR: evidenceDir })
  const preflightJson = parseJsonOutput(preflight.output)
  if (preflightJson?.stageRoot) copyReaderEvidence(preflightJson.stageRoot)
  const dualRaw = readJson(path.join(evidenceDir, 'B13_R3_DUAL_READER_UTF8.json'))
  if (dualRaw) writeRoot('B13_R3_DUAL_READER_UTF8.json', { ...dualRaw, sourceCommit, repairBaselineCommit })
  const packageConsistency = readJson(path.join(evidenceDir, 'B13_R3_PACKAGE_CONSISTENCY.json'))
  const packageSecrets = preflightJson?.stageRoot
    ? readJson(path.join(preflightJson.stageRoot, 'PACKAGE_MANIFEST', 'SECRET_SCAN.json'))
    : null
  const mutations = run('real_mutations', 'node', ['scripts/run-b13-r3-mutations.mjs'], {
    B13_R3_EVIDENCE_DIR: evidenceDir,
    B13_R3_SOURCE_COMMIT: sourceCommit,
    B13_R3_REPAIR_BASELINE_COMMIT: repairBaselineCommit,
  })
  const mutationReport = readJson(path.join(evidenceDir, 'B13_R3_MUTATION_GATES.json')) || { mutations: [], status: 'FAIL' }
  const uxMutation = mutationReport.mutations.find(item => item.id === 'MUT-R3-02')
  const productEvidence = readJson(path.join(evidenceDir, 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json')) || {}
  writeEvidence('B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json', {
    ...productEvidence,
    mutationIntroducedExitCode: uxMutation?.introducedExitCode ?? null,
    mutationRevertedExitCode: uxMutation?.revertedExitCode ?? null,
    status: productEvidence.status === 'PASS' && uxMutation?.status === 'PASS' ? 'PASS' : 'FAIL',
  })

  const b09Boundary = run('b09_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b09_h04', '--', '--nocapture'])
  const rustAll = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
  const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
  const regressionStatus = [
    r2Regression, r3Targeted, b09Boundary, node, vitest, typecheck, build, buildDemo,
    verify, contracts, productUxFormal, rustAll, rustFmt, rustCheck, rustClippy,
  ].every(item => item.status === 'PASS')
  const postRegressionRestoration = historicalEvidenceFiles.map(file => {
    const changed = spawnSync('git', ['diff', '--quiet', '--', file], { cwd: root }).status !== 0
    if (!changed) return { file, changed: false, status: 'UNCHANGED' }
    const restored = spawnSync('git', ['restore', '--source', sourceCommit, '--', file], { cwd: root, encoding: 'utf8' })
    return { file, changed: true, status: restored.status === 0 ? 'RESTORED' : 'FAIL' }
  })
  writeRoot('B13_R3_HISTORICAL_EVIDENCE_RESTORATION.json', {
    schema: 'b13-r3-historical-evidence-restoration',
    files: postRegressionRestoration,
    status: postRegressionRestoration.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL',
  })
  writeEvidence('B13_R3_REGRESSION.json', {
    schema: 'b13-r3-regression',
    r2Regression: r2Regression.status,
    r3Targeted: r3Targeted.status,
    b09Regression: b09Boundary.status,
    rustAllTargets: rustAll.status,
    rustFmt: rustFmt.status,
    rustCheck: rustCheck.status,
    rustClippy: rustClippy.status,
    node: node.status,
    vitest: vitest.status,
    typecheck: typecheck.status,
    build: build.status === 'PASS' && buildDemo.status === 'PASS' ? 'PASS' : 'FAIL',
    contracts: contracts.status,
    status: regressionStatus ? 'PASS' : 'FAIL',
  })

  const dual = readJson(path.join(evidenceDir, 'B13_R3_DUAL_READER_UTF8.json'))
  const product = readJson(path.join(evidenceDir, 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json'))
  const truthfulness = evaluateR3EvidenceTruthfulness(evidenceDir, sourceCommit, repairBaselineCommit)
  writeRoot('B13_R3_EVIDENCE_TRUTHFULNESS.json', truthfulness)

  const plansByName = new Map((readJson(path.join(evidenceDir, 'B13_R3_CURRENT_QUERY_PLANS.json'))?.plans || []).map(plan => [plan.queryName, plan]))
  const mutation = id => mutationReport.mutations.find(item => item.id === id)
  const gate = (condition, source, actual = {}) => ({ status: condition ? 'PASS' : 'FAIL', blocking: true, source, ...actual })
  const gates = {
    b13_r3_01_task_reality_complete_read: gate(truthfulness.checks.taskReality && truthfulness.checks.integration && truthfulness.checks.b09Semantics && mutation('MUT-R3-01')?.status === 'PASS', 'B13_R3_TASK_REALITY_COMPLETE_READ.json'),
    b13_r3_02_product_ux_bookplan_volumeplan: gate(truthfulness.checks.productUx && mutation('MUT-R3-02')?.status === 'PASS', 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json'),
    b13_r3_03_current_performance_dual_reader: gate(truthfulness.checks.queryPlans && truthfulness.checks.dualReader && mutation('MUT-R3-03A')?.status === 'PASS' && mutation('MUT-R3-03B')?.status === 'PASS', 'B13_R3_CURRENT_QUERY_PLANS.json'),
    shared_chunked_task_reader: gate(truthfulness.checks.taskReality && architectureReport.checks.sharedChunkedTaskReader === 'PASS', 'B13_R3_TASK_REALITY_COMPLETE_READ.json'),
    global_task_limit_unchanged: gate(architectureReport.checks.globalTaskLimitUnchanged === 'PASS', 'B13_R3_ARCHITECTURE.json'),
    task_100_boundary: gate(Number(taskMarker && numberField(taskMarker, 'task100Chunks')) === 1, 'B13_R3_TASK_REALITY_COMPLETE_READ.json'),
    task_101_boundary: gate(Number(taskMarker && numberField(taskMarker, 'task101Chunks')) === 2, 'B13_R3_TASK_REALITY_COMPLETE_READ.json'),
    task_501_formal_reality: gate(truthfulness.checks.integration && Number(taskMarker && numberField(taskMarker, 'returnedTaskStates')) === 501, 'B13_R3_501_FORMAL_INTEGRATION.json'),
    b09_task_reality_complete: gate(truthfulness.checks.b09Semantics, 'B13_R3_B09_SEMANTICS_UNCHANGED.json'),
    h04_task_blockage: gate(truthfulness.checks.taskReality && Number(taskMarker && numberField(taskMarker, 'blockedRequiredTasks')) >= 1, 'B13_R3_TASK_REALITY_COMPLETE_READ.json'),
    page_independent_truth: gate(truthfulness.checks.integration && field(integrationMarker, 'pageIndependent') === 'PASS', 'B13_R3_501_FORMAL_INTEGRATION.json'),
    bookplan_user_visible: gate(product?.bookPlanVisibleCount === 0, 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json'),
    volumeplan_user_visible: gate(product?.volumePlanVisibleCount === 0, 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json'),
    m11_browser: gate(truthfulness.checks.productUx && m11?.result === 'PASS', 'B13_R3/MANUAL_BROWSER/M11.json'),
    product_ux_gate: gate(productUxFormal.status === 'PASS' && productUx.status === 'PASS', 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json'),
    product_ux_mutation: gate(mutation('MUT-R3-02')?.status === 'PASS', 'B13_R3/MUTATIONS/MUT-R3-02/result.json'),
    current_h03_query_plan: gate(truthfulness.checks.currentH03, 'B13_R3_CURRENT_QUERY_PLANS.json', { queryName: 'formal_route_source' }),
    current_h04_query_plan: gate(truthfulness.checks.currentH04, 'B13_R3_CURRENT_QUERY_PLANS.json', { queryName: 'task_state_chunk_le_100' }),
    task_chunk_query_plan: gate(truthfulness.checks.taskChunk, 'B13_R3_CURRENT_QUERY_PLANS.json'),
    reader_a: gate(dual?.readerAOk === true, 'B13_R3_DUAL_READER_UTF8.json'),
    reader_b: gate(dual?.readerBOk === true && dual?.readerB && dual.readerB !== 'python-zipfile', 'B13_R3_DUAL_READER_UTF8.json'),
    dual_reader_entry_set: gate(dual?.entrySetsEqual === true && dual?.entryCountA === dual?.entryCountB, 'B13_R3_DUAL_READER_UTF8.json'),
    dual_reader_content_hash: gate(dual?.contentHashesEqual === true && dual?.extractFileCountA === dual?.extractFileCountB, 'B13_R3_DUAL_READER_UTF8.json'),
    no_authority_mutation: gate(truthfulness.checks.noAuthority, 'B13_R3_NO_AUTHORITY_MUTATION.json'),
    no_remote_model_call: gate(truthfulness.checks.noRemote, 'B13_R3_NO_REMOTE_CALL.json'),
    architecture: gate(architecture.status === 'PASS' && architectureReport.status === 'PASS', 'B13_R3_ARCHITECTURE.json'),
    module_boundary: gate(moduleBoundary.status === 'PASS', 'B13_R3/AUTOMATIC/module_boundary.log'),
    b09_regression: gate(b09Boundary.status === 'PASS' && r2Regression.status === 'PASS', 'B13_R3_REGRESSION.json'),
    b10_boundary: gate(b10Boundary.status === 'PASS', 'B13_R3/AUTOMATIC/b10_boundary.log'),
    b12_boundary: gate(b12Boundary.status === 'PASS', 'B13_R3/AUTOMATIC/b12_boundary.log'),
    b14_boundary: gate(!changed.some(item => /B14|Endgame|Completion/.test(item.file || '')), 'B13_R3_PREBATCH_FREEZE.json'),
    b15_boundary: gate(!changed.some(item => /B15|ExportRelease/.test(item.file || '')), 'B13_R3_PREBATCH_FREEZE.json'),
    rc1_boundary: gate(!changed.some(item => /RC1/.test(item.file || '')), 'B13_R3_PREBATCH_FREEZE.json'),
    rust_fmt: gate(rustFmt.status === 'PASS', 'B13_R3_REGRESSION.json'),
    rust_check: gate(rustCheck.status === 'PASS', 'B13_R3_REGRESSION.json'),
    rust_clippy: gate(rustClippy.status === 'PASS', 'B13_R3_REGRESSION.json'),
    rust_tests: gate(rustAll.status === 'PASS' && r3Targeted.status === 'PASS' && b09Boundary.status === 'PASS', 'B13_R3_REGRESSION.json'),
    node_tests: gate(node.status === 'PASS' && r3Node.status === 'PASS', 'B13_R3_REGRESSION.json'),
    vitest: gate(vitest.status === 'PASS', 'B13_R3_REGRESSION.json'),
    typecheck: gate(typecheck.status === 'PASS', 'B13_R3_REGRESSION.json'),
    build: gate(build.status === 'PASS' && buildDemo.status === 'PASS', 'B13_R3_REGRESSION.json'),
    contracts: gate(contracts.status === 'PASS', 'B13_R3_REGRESSION.json'),
    package_integrity: gate(packageConsistency?.status === 'PASS' && preflight.status === 'PASS', 'B13_R3_PACKAGE_CONSISTENCY.json'),
    package_inventory: gate(packageConsistency?.status === 'PASS', 'B13_R3_PACKAGE_CONSISTENCY.json'),
    files_sha256: gate(packageConsistency?.status === 'PASS', 'B13_R3_PACKAGE_CONSISTENCY.json'),
    package_utf8: gate(packageConsistency?.status === 'PASS' && dual?.status === 'PASS', 'B13_R3_DUAL_READER_UTF8.json'),
    secret_scan: gate(packageSecrets?.status === 'PASS' && Array.isArray(packageSecrets.realSecretFindings) && packageSecrets.realSecretFindings.length === 0, 'B13_R3_ARCHITECTURE.json'),
  }

  const sourceAfterCommit = git(['rev-parse', 'HEAD'])
  const statusAfter = git(['status', '--porcelain', '--untracked-files=all'])
  const allStatus = Object.values(gates).every(item => item.status === 'PASS')
    && truthfulness.status === 'PASS'
    && sourceAfterCommit === sourceCommit
    && statusBefore === ''
    && statusAfter === ''
  const gateResults = {
    schema: 'b13-r3-gate-results',
    batch: 'B13-R3',
    repairBaselineShort,
    repairBaselineCommit,
    postB12FrozenCommit,
    sourceCommit,
    evidenceSourceCommit: sourceCommit,
    sourceCommitAfter: sourceAfterCommit,
    workingTreeDirtyBefore: statusBefore !== '',
    workingTreeDirtyAfter: statusAfter !== '',
    excludedUncommittedPaths: [],
    commands,
    gates,
    status: allStatus ? 'PASS' : 'FAIL',
    deliveryStatus: allStatus ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    nextBatchAllowed: false,
  }
  writeRoot('gate-results.json', gateResults)
  writeRoot('COMMANDS_EXECUTED.json', { schema: 'b13-r3-commands-executed', sourceCommit, repairBaselineCommit, commands })
  const acceptance = generateR3Acceptance(evidenceDir)
  writeRoot('B13_R3_FINAL_DELIVERY_STATUS.json', {
    schema: 'b13-r3-final-delivery-status',
    sourceCommit,
    repairBaselineCommit,
    deliveryStatus: acceptance.deliveryStatus,
    nextBatchAllowed: false,
    status: acceptance.status,
  })
  console.log('B13_R3_EVIDENCE_RESULT sourceCommit=' + sourceCommit + ' gates=' + Object.values(gates).filter(item => item.status === 'PASS').length + '/' + Object.keys(gates).length + ' status=' + acceptance.status + ' evidenceDir=' + evidenceDir)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}

try { main() } catch (error) { console.error(error instanceof Error ? error.stack : String(error)); process.exitCode = 1 }
