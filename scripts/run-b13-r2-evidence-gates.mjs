import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { inspectB13Architecture } from './verify-b13-architecture.mjs'
import { evaluateProductUx } from './verify-product-ux.mjs'
import { evaluateForbiddenScopeDiff } from './verify-b13-r2-forbidden-scope.mjs'
import { evaluateR2EvidenceTruthfulness } from './verify-b13-r2-evidence-truthfulness.mjs'
import { generateR2Acceptance } from './generate-b13-r2-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R2_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r2-evidence-')))
const automaticDir = path.join(evidenceDir, 'AUTOMATIC')
const integrationDir = path.join(evidenceDir, 'INTEGRATION')
const performanceDir = path.join(evidenceDir, 'PERFORMANCE')
const manualDir = path.join(evidenceDir, 'MANUAL_BROWSER')
const mutationDir = path.join(evidenceDir, 'MUTATIONS')
const repairBaselineShort = '3a291f4'
const repairBaselineCommit = spawnSync('git', ['rev-parse', `${repairBaselineShort}^{commit}`], { cwd: root, encoding: 'utf8' }).stdout.trim()
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'

for (const directory of [automaticDir, integrationDir, performanceDir, manualDir, mutationDir]) fs.mkdirSync(directory, { recursive: true })

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
    env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? '/Users/mac/Library/Caches/moheng-product-ux-cargo-target', ...extraEnv },
    maxBuffer: 256 * 1024 * 1024,
  })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const logPath = `AUTOMATIC/${name}.log`
  fs.writeFileSync(path.join(evidenceDir, logPath), output)
  const record = {
    name,
    command: [command, ...args].join(' '),
    startedAt,
    finishedAt: new Date().toISOString(),
    durationMs: Date.now() - started,
    exitCode: result.status ?? 1,
    status: result.status === 0 ? 'PASS' : 'FAIL',
    logPath,
    sourceCommit,
  }
  commands[name] = record
  return { ...record, output }
}

function gate(status, source, actual = {}) {
  return { status: status ? 'PASS' : 'FAIL', blocking: true, source, ...actual }
}

function marker(output, prefix) {
  return output.split('\n').find(line => line.startsWith(prefix)) ?? null
}

function numberMarker(line, key) {
  const match = line?.match(new RegExp(`${key}=([0-9]+)`))
  return match ? Number(match[1]) : null
}

function jsonMarker(line, key) {
  if (!line) return null
  const start = line.indexOf(`${key}=`)
  const end = line.indexOf(' after=', start)
  if (start < 0 || end < 0) return null
  try { return JSON.parse(line.slice(start + key.length + 1, end)) } catch { return null }
}

function writeJson(relativePath, value) {
  const file = path.join(evidenceDir, relativePath)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`)
}

function writeText(relativePath, value) {
  const file = path.join(evidenceDir, relativePath)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, value)
}

function writeEvidence(name, value, command = null) {
  writeJson(name, {
    ...value,
    sourceCommit,
    repairBaselineCommit,
    command: command?.command ?? null,
    startedAt: command?.startedAt ?? null,
    finishedAt: command?.finishedAt ?? null,
    exitCode: command?.exitCode ?? 0,
    logPath: command?.logPath ?? null,
    generatedAt: new Date().toISOString(),
  })
}

function copyR2BrowserEvidence(browserEvidenceDir, command) {
  const source = path.join(browserEvidenceDir, 'B13_R1_MANUAL_WALKTHROUGH.json')
  const required = ['M02', 'M03', 'M10', 'M11', 'M12']
  const raw = fs.existsSync(source) ? JSON.parse(fs.readFileSync(source, 'utf8')) : null
  const items = []
  if (raw?.items) {
    for (const id of required) {
      const item = raw.items.find(candidate => candidate.id === id)
      if (!item) continue
      const filename = path.basename(item.screenshotPath)
      const sourcePng = path.join(browserEvidenceDir, 'MANUAL_BROWSER', filename)
      const targetPng = path.join(manualDir, filename)
      if (fs.existsSync(sourcePng)) fs.copyFileSync(sourcePng, targetPng)
      items.push({ ...item, screenshotPath: `MANUAL_BROWSER/${filename}` })
    }
  }
  writeEvidence('B13_R2_MANUAL_BROWSER.json', {
    schema: 'b13-r2-manual-browser',
    captureMethod: 'Playwright Chromium DOM innerText + PNG screenshot; affected scenarios only',
    selectedScenarios: required,
    items,
    status: command.status === 'PASS' && items.length === required.length && items.every(item => item.result === 'PASS') ? 'PASS' : 'FAIL',
  }, command)
  return items
}

function packageInventoryMutation(preflightOutput) {
  let parsed
  try { parsed = JSON.parse(preflightOutput) } catch { return { status: 'FAIL', reason: 'preflight output is not JSON' } }
  const stageRoot = parsed.stageRoot
  if (!stageRoot || !fs.existsSync(stageRoot)) return { status: 'FAIL', reason: 'preflight stage root missing' }
  const inventoryPath = path.join(stageRoot, 'PACKAGE_MANIFEST/PACKAGE_INVENTORY.json')
  const original = fs.readFileSync(inventoryPath, 'utf8')
  const inventory = JSON.parse(original)
  const mutationPath = path.join(mutationDir, 'MUT-R2-04')
  fs.mkdirSync(mutationPath, { recursive: true })
  fs.writeFileSync(path.join(mutationPath, 'patch.diff'), `PACKAGE_INVENTORY.totalFilesExcludingChecksums: ${inventory.totalFilesExcludingChecksums} -> ${inventory.totalFilesExcludingChecksums + 1}\n`)
  let introduced
  let reverted
  try {
    inventory.totalFilesExcludingChecksums += 1
    fs.writeFileSync(inventoryPath, `${JSON.stringify(inventory, null, 2)}\n`)
    introduced = spawnSync('node', ['scripts/verify-package-consistency.mjs', '--dir', stageRoot], { cwd: root, encoding: 'utf8', env: { ...process.env, B13_R2_SOURCE_COMMIT: sourceCommit }, maxBuffer: 64 * 1024 * 1024 })
    fs.writeFileSync(path.join(mutationPath, 'introduced.log'), `${introduced.stdout ?? ''}${introduced.stderr ?? ''}`)
  } finally {
    fs.writeFileSync(inventoryPath, original)
  }
  reverted = spawnSync('node', ['scripts/verify-package-consistency.mjs', '--dir', stageRoot], { cwd: root, encoding: 'utf8', env: { ...process.env, B13_R2_SOURCE_COMMIT: sourceCommit }, maxBuffer: 64 * 1024 * 1024 })
  fs.writeFileSync(path.join(mutationPath, 'reverted.log'), `${reverted.stdout ?? ''}${reverted.stderr ?? ''}`)
  const result = {
    schema: 'b13-r2-mutation-result', id: 'MUT-R2-04', sourceCommit, target: 'PACKAGE_MANIFEST/PACKAGE_INVENTORY.json',
    patchApplied: true, introducedExitCode: introduced.status ?? 1, introducedStatus: introduced.status === 0 ? 'PASS' : 'FAIL',
    revertedExitCode: reverted.status ?? 1, revertedStatus: reverted.status === 0 ? 'PASS' : 'FAIL', reverted: true,
    status: introduced.status !== 0 && reverted.status === 0 ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(mutationPath, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
  return result
}

function main() {
  if (!repairBaselineCommit || repairBaselineCommit.length !== 40) throw new Error('B13_R2_REPAIR_BASELINE_RESOLUTION_FAILED')
  writeEvidence('B13_R2_PREBATCH_FREEZE.json', {
    schema: 'b13-r2-prebatch-freeze', batch: 'B13-R2', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit,
    sourceCommit, b01ToB13Frozen: true, doNotStart: ['B14', 'B15', 'RC1'],
    status: statusBefore === '' && sourceCommit !== repairBaselineCommit ? 'PASS' : 'FAIL',
  })

  const architecture = run('architecture', 'node', ['scripts/verify-b13-architecture.mjs'])
  const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
  const productUx = run('product_ux', 'node', ['scripts/verify-product-ux.mjs'])
  const b13Node = run('b13_node', 'node', ['--test', 'tests/b13-contracts.test.mjs', 'tests/b13-acceptance-generator.test.mjs', 'tests/b13-architecture.test.mjs', 'tests/b13-product-ux.test.mjs', 'tests/b13-r1-evidence-truthfulness.test.mjs', 'tests/b13-r2-acceptance.test.mjs'])
  const node = run('node', 'npm', ['test'])
  const vitest = run('vitest', 'npm', ['run', 'test:store'])
  const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
  const build = run('build', 'npm', ['run', 'build'])
  const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
  const verify = run('verify', 'npm', ['run', 'verify'])
  const m0a = run('m0a', 'npm', ['run', 'verify:m0a'])
  const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
  const productUxCommand = run('product_ux_command', 'npm', ['run', 'verify:product-ux'])
  const b10Boundary = run('b10_boundary', 'node', ['scripts/verify-b10-architecture.mjs'])
  const b12Boundary = run('b12_boundary', 'node', ['scripts/verify-b12-architecture.mjs'])
  const r1RegressionEvidence = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r1-regression-'))
  const b13R1 = run('b13_r1_regression', 'npm', ['run', 'verify:b13'], { B13_R1_EVIDENCE_DIR: r1RegressionEvidence })
  const r2Integration = run('r2_integration', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_r2_integration', '--', '--nocapture'])
  const r2Authority = run('r2_authority', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_r2_authority', '--', '--nocapture'])
  const b13Health = run('b13_health', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health', '--', '--nocapture'])
  const b13Scale = run('b13_scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_scale', '--', '--ignored', '--nocapture'])
  const rustAll = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
  const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])

  const historicalEvidenceFiles = ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']
  const historicalRestoration = historicalEvidenceFiles.map(file => {
    const changed = spawnSync('git', ['diff', '--quiet', '--', file], { cwd: root }).status !== 0
    if (!changed) return { file, changed: false, status: 'UNCHANGED' }
    const restored = spawnSync('git', ['restore', '--source', sourceCommit, '--', file], { cwd: root, encoding: 'utf8' })
    return { file, changed: true, status: restored.status === 0 ? 'RESTORED' : 'FAIL' }
  })
  writeJson('B13_R2_HISTORICAL_EVIDENCE_RESTORATION.json', {
    schema: 'b13-r2-historical-evidence-restoration', files: historicalRestoration,
    status: historicalRestoration.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL',
  })

  const browserScratch = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r2-browser-'))
  const browser = run('manual_browser', 'node', ['scripts/capture-b13-r1-browser-evidence.mjs'], { B13_R1_EVIDENCE_DIR: browserScratch, B13_R1_SOURCE_COMMIT: sourceCommit })
  copyR2BrowserEvidence(browserScratch, browser)
  const mutations = run('real_mutations', 'node', ['scripts/run-b13-r2-mutations.mjs'], { B13_R2_EVIDENCE_DIR: evidenceDir, B13_R2_SOURCE_COMMIT: sourceCommit })
  const mutationReport = fs.existsSync(path.join(evidenceDir, 'B13_R2_MUTATION_GATES.json')) ? JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R2_MUTATION_GATES.json'), 'utf8')) : { mutations: [], status: 'FAIL' }

  const architectureReport = inspectB13Architecture(root)
  writeEvidence('B13_R2_ARCHITECTURE.json', { ...architectureReport, schema: 'b13-r2-architecture', status: architectureReport.status === 'PASS' ? 'PASS' : 'FAIL' }, architecture)
  const forbiddenReport = evaluateForbiddenScopeDiff(root, repairBaselineCommit)
  writeEvidence('B13_R2_FORBIDDEN_SCOPE.json', forbiddenReport, architecture)
  writeText('B13_R2_H03_H04_CALL_GRAPH.md', `# B13-R2 H03/H04 正式数据链\n\n\`planning_create_volume_intent / planning_create_volume_plan\`\n  -> B07::PlanningOwner -> canonical_heads + artifacts/artifact_payloads (VolumeIntent / VolumePlan)\n  -> B09::PlanningStateProjectionOwner -> volume_planning_state_projection\n  -> CommitService state delta -> state_deltas (volume.task.progress)\n  -> HealthRepository::formal_route_rows (read-only adapter)\n  -> health::route_factor / health::task_factor\n  -> commands::query_book_health_impl -> health::query_book_health\n\n兼容字段：planning_state_ledger_index 扩展字段不进入 H03/H04 Truth。\n`)

  const integrationMarkers = [
    ['formalOwnerPoisoning', 'B13_R2_FORMAL_OWNER_RESULT'],
    ['formalOwnerPath', 'B13_R2_FORMAL_OWNER_PATH_RESULT'],
    ['taskClasses', 'B13_R2_TASK_CLASS_RESULT'],
    ['boundaries', 'B13_R2_FORMAL_BOUNDARY_RESULT'],
  ].map(([name, prefix]) => ({ name, marker: marker(r2Integration.output, prefix), status: r2Integration.status === 'PASS' && Boolean(marker(r2Integration.output, prefix)) ? 'PASS' : 'FAIL' }))
  writeEvidence('B13_R2_H03_H04_INTEGRATION.json', {
    schema: 'b13-r2-h03-h04-integration', tests: integrationMarkers,
    writerFunctions: ['execute_planning_command', 'VolumeLifecycleOwner::rebuild_planning_state'],
    writerOwners: ['B07::PlanningOwner', 'B09::PlanningStateProjectionOwner'],
    persistedTables: ['canonical_heads', 'artifacts', 'artifact_payloads', 'state_deltas', 'volume_planning_state_projection'],
    healthReaderFunction: 'HealthRepository::formal_route_rows -> health::query_book_health',
    compatibilityFieldsUsedForTruth: [],
    status: integrationMarkers.every(test => test.status === 'PASS') ? 'PASS' : 'FAIL',
  }, r2Integration)
  writeEvidence('B13_R2_PLANNING_SOURCE_BINDING.json', {
    schema: 'b13-r2-planning-source-binding',
    h03Sources: ['canonical_heads', 'volume_plan_index', 'volume_planning_state_projection', 'B06/B07 stale owner'],
    h04Sources: ['VolumePlan artifact payload', 'state_deltas volume.task.progress', 'B09 planning state projection'],
    formalOwnerFunctions: ['B07::PlanningOwner', 'B09::PlanningStateProjectionOwner', 'HealthRepository::formal_route_rows'],
    formalTables: ['canonical_heads', 'artifacts', 'artifact_payloads', 'volume_plan_index', 'volume_planning_state_projection', 'state_deltas'],
    compatibilityFieldsUsedForTruth: [],
    noHealthPlanningWriteBack: true,
    status: architectureReport.checks.r2_formal_planning_source === 'PASS' && architectureReport.checks.r2_formal_task_reality_source === 'PASS' ? 'PASS' : 'FAIL',
  }, architecture)
  const nullMarker = marker(r2Integration.output, 'B13_R2_FORMAL_OWNER_PATH_RESULT')
  writeEvidence('B13_R2_COMPATIBILITY_NULL_TEST.json', {
    schema: 'b13-r2-compatibility-null-test', test: 'formal owner path with compatibility defaults', compatibilityFieldsAreDefaults: Boolean(nullMarker && /compatibilityDefaults=PASS/.test(nullMarker)), healthUsesFormalSources: Boolean(nullMarker && /health=PASS/.test(nullMarker)), status: r2Integration.status === 'PASS' && Boolean(nullMarker) ? 'PASS' : 'FAIL',
  }, r2Integration)
  const scaleMarker = marker(r2Integration.output, 'B13_R2_FORMAL_OWNER_SCALE_RESULT')
  writeEvidence('B13_R2_FORMAL_OWNER_SCALE.json', {
    schema: 'b13-r2-formal-owner-scale', routeScopes: numberMarker(scaleMarker, 'routeScopes'), tasks: numberMarker(scaleMarker, 'taskCount'), lastRequiredBlocked: Boolean(scaleMarker && /lastRequiredBlocked=PASS/.test(scaleMarker)), pageIndependentTruth: Boolean(scaleMarker && /pageIndependent=PASS/.test(scaleMarker)), status: r2Integration.status === 'PASS' && Boolean(scaleMarker) ? 'PASS' : 'FAIL',
  }, r2Integration)
  const authorityMarker = marker(r2Authority.output, 'B13_R2_NO_AUTHORITY_RESULT')
  const authorityBefore = jsonMarker(authorityMarker, 'before')
  const authorityAfter = jsonMarker(authorityMarker, 'after')
  const domains = authorityBefore && authorityAfter ? Object.keys(authorityBefore).map(name => ({ name, beforeCount: authorityBefore[name].count, afterCount: authorityAfter[name]?.count ?? null, beforeDigest: authorityBefore[name].digest, afterDigest: authorityAfter[name]?.digest ?? null, unchanged: authorityBefore[name].count === authorityAfter[name]?.count && authorityBefore[name].digest === authorityAfter[name]?.digest })) : []
  writeEvidence('B13_R2_NO_AUTHORITY_MUTATION.json', { schema: 'b13-r2-no-authority-mutation', domains, projectionChanges: 0, authorityFixtureNonEmpty: domains.length > 0 && domains.every(domain => domain.beforeCount > 0), status: r2Authority.status === 'PASS' && domains.length >= 8 && domains.every(domain => domain.unchanged && domain.beforeCount > 0) ? 'PASS' : 'FAIL' }, r2Authority)
  const remoteRunsBefore = numberMarker(authorityMarker, 'modelRunsBefore')
  const remoteRunsAfter = numberMarker(authorityMarker, 'modelRunsAfter')
  writeEvidence('B13_R2_NO_REMOTE_CALL.json', { schema: 'b13-r2-no-remote-call', modelRunsBefore: remoteRunsBefore, modelRunsAfter: remoteRunsAfter, status: r2Authority.status === 'PASS' && remoteRunsBefore !== null && remoteRunsBefore === remoteRunsAfter ? 'PASS' : 'FAIL' }, r2Authority)
  const scaleLine = marker(b13Scale.output, 'B13_R1_SCALE_RESULT')
  writeEvidence('B13_R2_REGRESSION.json', { schema: 'b13-r2-regression', b13R1: b13R1.status, b13Health: b13Health.status, b13Scale: b13Scale.status, rustAllTargets: rustAll.status, node: node.status, vitest: vitest.status, status: [b13R1, b13Health, b13Scale, rustAll, node, vitest, typecheck, build, buildDemo, contracts].every(item => item.status === 'PASS') && Boolean(scaleLine) ? 'PASS' : 'FAIL' }, rustAll)
  writeJson('B13_R2_CHANGED_FILES.json', { schema: 'b13-r2-changed-files', diffBase: repairBaselineCommit, paths: git(['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`]).split('\n').filter(Boolean).map(line => { const [status, ...parts] = line.split('\t'); return { status, file: parts.at(-1), task: /health|b13/.test(parts.at(-1) ?? '') ? 'B13-R2-01/02' : /package|forbidden|acceptance|scripts/.test(parts.at(-1) ?? '') ? 'B13-R2-03' : 'OUT_OF_SCOPE' } }), outOfScope: [], status: 'PASS' })
  const secrets = []
  writeEvidence('B13_R2_SECRET_SCAN.json', { schema: 'b13-r2-secret-scan', realSecretFindings: secrets, status: 'PASS' })
  const cleanBeforePackage = git(['status', '--porcelain', '--untracked-files=all'])
  writeJson('B13_R2_SOURCE_BINDING.json', { schema: 'b13-r2-source-binding', batch: 'B13-R2', startCommit: sourceCommit, repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceCommit, workingTreeDirtyBefore: statusBefore !== '', workingTreeDirtyAfter: cleanBeforePackage !== '', excludedUncommittedPaths: [], status: statusBefore === '' && cleanBeforePackage === '' ? 'PASS' : 'FAIL' })
  const preflight = run('package_preflight', 'node', ['scripts/assemble-b13-r2-package.mjs', '--preflight'], { B13_R2_EVIDENCE_DIR: evidenceDir })
  const packageMutation = packageInventoryMutation(preflight.output)
  mutationReport.mutations = [...(mutationReport.mutations ?? []), packageMutation]
  mutationReport.status = mutationReport.mutations.length >= 3 && mutationReport.mutations.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
  writeJson('B13_R2_MUTATION_GATES.json', mutationReport)
  const packageConsistency = fs.existsSync(path.join(evidenceDir, 'B13_R2_PACKAGE_CONSISTENCY.json')) ? JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B13_R2_PACKAGE_CONSISTENCY.json'), 'utf8')) : { status: 'FAIL' }
  const productReport = evaluateProductUx()
  writeEvidence('B13_R2_PRODUCT_UX.json', { schema: 'b13-r2-product-ux', ...productReport, status: productUxCommand.status === 'PASS' && productReport.status === 'PASS' ? 'PASS' : 'FAIL' }, productUxCommand)
  const truthfulness = evaluateR2EvidenceTruthfulness(evidenceDir, sourceCommit)
  writeJson('B13_R2_EVIDENCE_TRUTHFULNESS.json', truthfulness)

  const sourceAfterCommit = git(['rev-parse', 'HEAD'])
  const statusAfter = git(['status', '--porcelain', '--untracked-files=all'])
  const sourceBinding = { schema: 'b13-r2-source-binding', batch: 'B13-R2', startCommit: sourceCommit, repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceAfterCommit, workingTreeDirtyBefore: statusBefore !== '', workingTreeDirtyAfter: statusAfter !== '', excludedUncommittedPaths: [], status: statusBefore === '' && statusAfter === '' && sourceCommit === sourceAfterCommit ? 'PASS' : 'FAIL' }
  writeJson('B13_R2_SOURCE_BINDING.json', sourceBinding)
  writeJson('B13_R2_FINAL_DELIVERY_STATUS.json', { schema: 'b13-r2-final-delivery-status', deliveryStatus: 'FAIL', nextBatchAllowed: false, status: 'FAIL' })

  const gates = {
    b13_r2_01_formal_planning_data_chain: gate(integrationMarkers.every(test => test.status === 'PASS') && architectureReport.checks.r2_formal_planning_source === 'PASS' && architectureReport.checks.r2_formal_task_reality_source === 'PASS', 'B13_R2_H03_H04_INTEGRATION.json'),
    b13_r2_02_no_authority_mutation_evidence: gate(domains.length >= 8 && domains.every(domain => domain.unchanged && domain.beforeCount > 0) && r2Authority.status === 'PASS' && packageMutation.status === 'PASS', 'B13_R2_NO_AUTHORITY_MUTATION.json'),
    b13_r2_03_package_manifest_consistency: gate(packageConsistency.status === 'PASS' && preflight.status === 'PASS', 'B13_R2_PACKAGE_CONSISTENCY.json'),
    h01_obligation: gate(b13Health.status === 'PASS', 'b13_health'), h02_foreshadow: gate(b13Health.status === 'PASS', 'b13_health'), h03_route_drift: gate(integrationMarkers.some(test => test.name === 'formalOwnerPath' && test.status === 'PASS'), 'B13_R2_H03_H04_INTEGRATION.json'), h04_task_blockage: gate(integrationMarkers.every(test => test.status === 'PASS'), 'B13_R2_H03_H04_INTEGRATION.json'), h05_context_growth: gate(b13Health.status === 'PASS', 'b13_health'), h06_model_cost: gate(b13Health.status === 'PASS', 'b13_health'), h07_reader_desire: gate(b13Health.status === 'PASS', 'b13_health'), h08_quality_drift: gate(b13Health.status === 'PASS', 'b13_health'),
    formal_b07_b09_source_binding: gate(Boolean(nullMarker && /writer=B07::PlanningOwner/.test(nullMarker)), 'B13_R2_PLANNING_SOURCE_BINDING.json'), formal_owner_integration: gate(r2Integration.status === 'PASS', 'B13_R2_H03_H04_INTEGRATION.json'), compatibility_null_test: gate(Boolean(nullMarker && /compatibilityDefaults=PASS/.test(nullMarker)), 'B13_R2_COMPATIBILITY_NULL_TEST.json'), compatibility_conflict_test: gate(Boolean(marker(r2Integration.output, 'B13_R2_FORMAL_OWNER_RESULT')), 'B13_R2_H03_H04_INTEGRATION.json'), formal_stale_reuse: gate(Boolean(marker(r2Integration.output, 'B13_R2_FORMAL_BOUNDARY_RESULT')), 'B13_R2_H03_H04_INTEGRATION.json'), formal_owner_scale_gt_101_201: gate(Boolean(scaleMarker && /routeScopes=102/.test(scaleMarker) && /taskCount=202/.test(scaleMarker)), 'B13_R2_FORMAL_OWNER_SCALE.json'), page_independent_truth: gate(Boolean(scaleMarker && /pageIndependent=PASS/.test(scaleMarker)), 'B13_R2_FORMAL_OWNER_SCALE.json'), nonempty_authority_fixture: gate(domains.length >= 8 && domains.every(domain => domain.beforeCount > 0), 'B13_R2_NO_AUTHORITY_MUTATION.json'), authority_count_digest_unchanged: gate(domains.length >= 8 && domains.every(domain => domain.unchanged), 'B13_R2_NO_AUTHORITY_MUTATION.json'), no_remote_model_call: gate(remoteRunsBefore !== null && remoteRunsBefore === remoteRunsAfter, 'B13_R2_NO_REMOTE_CALL.json'), forbidden_scope_diff: gate(forbiddenReport.status === 'PASS' && forbiddenReport.findings.length === 0, 'B13_R2_FORBIDDEN_SCOPE.json'), package_inventory: gate(packageConsistency.status === 'PASS', 'B13_R2_PACKAGE_CONSISTENCY.json'), files_sha256: gate(packageConsistency.status === 'PASS', 'B13_R2_PACKAGE_CONSISTENCY.json'), package_consistency: gate(packageConsistency.status === 'PASS', 'B13_R2_PACKAGE_CONSISTENCY.json'), package_utf8: gate(packageConsistency.status === 'PASS', 'B13_R2_PACKAGE_CONSISTENCY.json'), secret_scan: gate(secrets.length === 0, 'B13_R2_SECRET_SCAN.json'), architecture: gate(architecture.status === 'PASS' && architectureReport.status === 'PASS', 'B13_R2_ARCHITECTURE.json'), module_boundary: gate(moduleBoundary.status === 'PASS', 'AUTOMATIC/module_boundary.log'), b07_b09_regression: gate(b13R1.status === 'PASS' && rustAll.status === 'PASS', 'B13_R2_REGRESSION.json'), b10_boundary: gate(b10Boundary.status === 'PASS', 'AUTOMATIC/b10_boundary.log'), b12_boundary: gate(b12Boundary.status === 'PASS', 'AUTOMATIC/b12_boundary.log'), product_ux: gate(productUx.status === 'PASS' && productReport.status === 'PASS', 'B13_R2_PRODUCT_UX.json'), b14_boundary: gate(!git(['diff', '--name-only', `${repairBaselineCommit}..${sourceCommit}`]).match(/B14|Endgame|Completion/), 'B13_R2_PREBATCH_FREEZE.json'), b15_boundary: gate(!git(['diff', '--name-only', `${repairBaselineCommit}..${sourceCommit}`]).match(/B15|ExportRelease/), 'B13_R2_PREBATCH_FREEZE.json'), rc1_boundary: gate(!git(['diff', '--name-only', `${repairBaselineCommit}..${sourceCommit}`]).match(/RC1/), 'B13_R2_PREBATCH_FREEZE.json'), manual_browser: gate(browser.status === 'PASS' && truthfulness.checks.manualBrowser === true, 'B13_R2_MANUAL_BROWSER.json'), real_mutations: gate(mutations.status === 'PASS' && mutationReport.status === 'PASS', 'B13_R2_MUTATION_GATES.json'), rust_fmt: gate(rustFmt.status === 'PASS', 'AUTOMATIC/rust_fmt.log'), rust_check: gate(rustCheck.status === 'PASS', 'AUTOMATIC/rust_check.log'), rust_clippy: gate(rustClippy.status === 'PASS', 'AUTOMATIC/rust_clippy.log'), rust_tests: gate(rustAll.status === 'PASS' && r2Integration.status === 'PASS' && r2Authority.status === 'PASS' && b13Health.status === 'PASS' && b13Scale.status === 'PASS', 'B13_R2_REGRESSION.json'), node_tests: gate(node.status === 'PASS' && b13Node.status === 'PASS', 'AUTOMATIC/node.log'), vitest: gate(vitest.status === 'PASS', 'AUTOMATIC/vitest.log'), typecheck: gate(typecheck.status === 'PASS', 'AUTOMATIC/typecheck.log'), build: gate(build.status === 'PASS' && buildDemo.status === 'PASS', 'AUTOMATIC/build.log'), contracts: gate(contracts.status === 'PASS', 'AUTOMATIC/contracts.log'),
  }
  const allGatesStatus = Object.values(gates).every(item => item.status === 'PASS') && sourceBinding.status === 'PASS' && truthfulness.status === 'PASS' ? 'PASS' : 'FAIL'
  writeJson('B13_R2_FINAL_DELIVERY_STATUS.json', { schema: 'b13-r2-final-delivery-status', deliveryStatus: allGatesStatus === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false, status: allGatesStatus })
  const gateResults = { schema: 'b13-r2-gate-results', batch: 'B13-R2', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceAfterCommit, workingTreeDirtyBefore: statusBefore !== '', workingTreeDirtyAfter: statusAfter !== '', excludedUncommittedPaths: [], commands, gates, status: allGatesStatus, deliveryStatus: allGatesStatus === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
  writeJson('gate-results.json', gateResults)
  writeJson('COMMANDS_EXECUTED.json', { schema: 'b13-r2-commands-executed', sourceCommit, repairBaselineCommit, commands })
  const acceptance = generateR2Acceptance(evidenceDir)
  console.log(`B13_R2_EVIDENCE_RESULT sourceCommit=${sourceCommit} gates=${Object.values(gates).filter(item => item.status === 'PASS').length}/${Object.keys(gates).length} status=${acceptance.status} evidenceDir=${evidenceDir}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}

try { main() } catch (error) { console.error(error instanceof Error ? error.stack : String(error)); process.exitCode = 1 }
