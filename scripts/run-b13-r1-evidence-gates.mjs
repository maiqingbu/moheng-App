import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { inspectB13Architecture } from './verify-b13-architecture.mjs'
import { evaluateProductUx } from './verify-product-ux.mjs'
import { evaluateEvidenceTruthfulness } from './verify-b13-evidence-truthfulness.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R1_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r1-evidence-')))
const automaticDir = path.join(evidenceDir, 'AUTOMATIC')
const performanceDir = path.join(evidenceDir, 'PERFORMANCE')
const repairBaselineCommit = '66638c4e7917fc233d084f0142a6a3a07bc0fb41'
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const originalInstructionPath = '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_长篇健康治理_开发与正式验收打包执行指令_R1_2026-09-04.md'
const repairInstructionPath = '/Users/mac/Downloads/墨衡_V2.1_B13_LongFormHealth_R1_定点修复与重新打包执行指令_2026-09-04.md'
const frozenMigrations = [
  '0001_catalog.sql', '0002_project_authority.sql', '0003_project_command.sql',
  '0004_project_longterm.sql', '0005_m0c_commit_delta.sql', '0006_m0e_ops.sql',
  '0007_b05_narrative_foundation.sql', '0008_h01_support_checkpoint.sql',
  '0009_h03_repair_workflow.sql', '0010_b09_h04_volume_lifecycle.sql',
  '0011_b12_change_retcon_runtime.sql', '0012_b13_longform_health_indexes.sql',
]

fs.mkdirSync(automaticDir, { recursive: true })
fs.mkdirSync(path.join(evidenceDir, 'MANUAL_BROWSER'), { recursive: true })
fs.mkdirSync(path.join(evidenceDir, 'MUTATIONS'), { recursive: true })
fs.mkdirSync(performanceDir, { recursive: true })

const gitResult = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' })
const git = args => gitResult(args)
const gitText = args => git(args).stdout.trim()
const sourceCommit = gitText(['rev-parse', 'HEAD'])
const statusBefore = gitText(['status', '--porcelain', '--untracked-files=all'])
const commands = {}

function run(name, command, args, extraEnv = {}) {
  const startedAt = new Date().toISOString()
  const started = Date.now()
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? '/Users/mac/Library/Caches/moheng-product-ux-cargo-target',
      ...extraEnv,
    },
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
    output,
  }
  commands[name] = record
  return record
}

function recordFor(command, fallback = 'derived from source-bound assertions') {
  return command ?? {
    command: fallback,
    startedAt: new Date().toISOString(),
    finishedAt: new Date().toISOString(),
    exitCode: 0,
    status: 'PASS',
    logPath: null,
  }
}

function writeEvidence(name, data, command = null) {
  const record = recordFor(command)
  const payload = {
    ...data,
    sourceCommit,
    baselineCommit: repairBaselineCommit,
    command: record.command,
    startedAt: record.startedAt,
    finishedAt: record.finishedAt,
    exitCode: record.exitCode,
    logPath: record.logPath,
    generatedAt: new Date().toISOString(),
  }
  fs.writeFileSync(path.join(evidenceDir, name), `${JSON.stringify(payload, null, 2)}\n`)
  return payload
}

function gate(status, source, actual = {}) {
  return { status: status ? 'PASS' : 'FAIL', blocking: true, source, ...actual }
}

function numberMarker(line, key) {
  const match = line?.match(new RegExp(`${key}=([0-9]+)`))
  return match ? Number(match[1]) : null
}

function tupleMarker(line, key) {
  const match = line?.match(new RegExp(`${key}=\\(([^)]+)\\)`))
  return match ? match[1].split(',').map(value => Number(value.trim())) : null
}

function testFiles() {
  const result = []
  const walk = directory => {
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      if (fs.statSync(file).isDirectory()) walk(file)
      else if (name.endsWith('.test.mjs')) result.push(path.relative(root, file))
    }
  }
  walk(path.join(root, 'tests'))
  return result.sort()
}

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex')
}

function gitShowBytes(commit, relative) {
  const result = spawnSync('git', ['show', `${commit}:${relative}`], { cwd: root })
  return result.status === 0 ? result.stdout : null
}

const architecture = run('architecture', 'node', ['scripts/verify-b13-architecture.mjs'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const productUx = run('product_ux', 'node', ['scripts/verify-product-ux.mjs'])
const b13Node = run('b13_node', 'node', [
  '--test',
  'tests/b13-contracts.test.mjs',
  'tests/b13-acceptance-generator.test.mjs',
  'tests/b13-architecture.test.mjs',
  'tests/b13-product-ux.test.mjs',
  'tests/b13-r1-evidence-truthfulness.test.mjs',
])
const node = run('node', 'node', ['--test', ...testFiles()])
const vitest = run('vitest', 'npm', ['run', 'test:store'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
const verify = run('verify', 'npm', ['run', 'verify'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const productUxCommand = run('product_ux_command', 'npm', ['run', 'verify:product-ux'])
const rustHealth = run('rust_health', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health', '--', '--nocapture'])
const rustScale = run('rust_scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_scale', '--', '--ignored', '--nocapture'])
const rustAll = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const packagePreflight = run('package_preflight', 'node', ['scripts/assemble-b13-r1-package.mjs', '--preflight'])

const browser = run('browser_manual', 'node', ['scripts/capture-b13-r1-browser-evidence.mjs'], {
  B13_R1_EVIDENCE_DIR: evidenceDir,
  B13_R1_SOURCE_COMMIT: sourceCommit,
})
const mutations = run('real_mutations', 'node', ['scripts/run-b13-r1-mutations.mjs'], {
  B13_R1_EVIDENCE_DIR: evidenceDir,
  B13_R1_SOURCE_COMMIT: sourceCommit,
})

const architectureReport = inspectB13Architecture(root)
const productUxReport = evaluateProductUx()
const scaleLine = rustScale.output.match(/B13_R1_SCALE_RESULT[^\n]*/)?.[0] ?? null
const scalePassed = rustScale.status === 'PASS'
  && Boolean(scaleLine)
  && numberMarker(scaleLine, 'words') >= 10_000_000
  && numberMarker(scaleLine, 'chapters') >= 4000
  && numberMarker(scaleLine, 'obligations') > 51
  && numberMarker(scaleLine, 'routeScopes') > 101
  && numberMarker(scaleLine, 'tasks') > 201
  && numberMarker(scaleLine, 'readerDesires') > 50
  && numberMarker(scaleLine, 'contextRuns') >= 16
  && numberMarker(scaleLine, 'costRuns') >= 8
  && numberMarker(scaleLine, 'qualitySamples') >= 20
  && scaleLine.includes('pageIndependentTruth=PASS')
  && scaleLine.includes('noFullChapterBody=PASS')
const scaleData = {
  schema: 'b13-r1-scale-certification',
  words: numberMarker(scaleLine, 'words'),
  chapters: numberMarker(scaleLine, 'chapters'),
  obligations: numberMarker(scaleLine, 'obligations'),
  routeScopes: numberMarker(scaleLine, 'routeScopes'),
  tasks: numberMarker(scaleLine, 'tasks'),
  readerDesires: numberMarker(scaleLine, 'readerDesires'),
  contextRuns: numberMarker(scaleLine, 'contextRuns'),
  costRuns: numberMarker(scaleLine, 'costRuns'),
  qualitySamples: numberMarker(scaleLine, 'qualitySamples'),
  pageIndependentTruth: scaleLine?.includes('pageIndependentTruth=PASS') ?? false,
  noFullChapterBody: scaleLine?.includes('noFullChapterBody=PASS') ?? false,
  status: scalePassed ? 'PASS' : 'FAIL',
}
writeEvidence('B13_R1_SCALE.json', scaleData, rustScale)

for (const line of rustScale.output.split('\n')) {
  const match = line.match(/^B13_R1_EXPLAIN_([^=]+)=(.*)$/)
  if (!match) continue
  fs.writeFileSync(path.join(performanceDir, `${match[1]}.explain.txt`), `EXPLAIN QUERY PLAN ${match[1]}: ${match[2]}\n`)
}
const queryPlanNames = ['H01', 'H02', 'H03', 'H04', 'H07', 'ISSUES']
const queryPlansValid = queryPlanNames.every(name => fs.existsSync(path.join(performanceDir, `${name}.explain.txt`)))
writeEvidence('B13_R1_QUERY_PLANS.json', {
  schema: 'b13-r1-query-plans',
  plans: queryPlanNames.map(name => ({ name, path: `PERFORMANCE/${name}.explain.txt`, status: fs.existsSync(path.join(performanceDir, `${name}.explain.txt`)) ? 'PASS' : 'FAIL' })),
  status: rustScale.status === 'PASS' && queryPlansValid ? 'PASS' : 'FAIL',
}, rustScale)

const noRemoteLine = rustHealth.output.match(/B13_R1_NO_REMOTE_RESULT[^\n]*/)?.[0] ?? null
const noAuthorityLine = rustHealth.output.match(/B13_R1_NO_AUTHORITY_RESULT[^\n]*/)?.[0] ?? null
const planStaleLine = rustHealth.output.match(/B13_R1_PLAN_STALE_RESULT[^\n]*/)?.[0] ?? null
const modelRunsBefore = numberMarker(noRemoteLine, 'modelRunsBefore')
const modelRunsAfter = numberMarker(noRemoteLine, 'modelRunsAfter')
writeEvidence('B13_R1_NO_REMOTE_CALL.json', {
  schema: 'b13-r1-no-remote-call',
  modelRunsBefore,
  modelRunsAfter,
  modelRunsUnchanged: modelRunsBefore !== null && modelRunsBefore === modelRunsAfter,
  transportCounter: 'NOT_APPLICABLE',
  status: rustHealth.status === 'PASS' && Boolean(noRemoteLine) && modelRunsBefore === modelRunsAfter ? 'PASS' : 'FAIL',
}, rustHealth)
const authorityBefore = tupleMarker(noAuthorityLine, 'before')
const authorityAfter = tupleMarker(noAuthorityLine, 'after')
writeEvidence('B13_R1_NO_AUTHORITY_MUTATION.json', {
  schema: 'b13-r1-no-authority-mutation',
  before: authorityBefore,
  after: authorityAfter,
  unchanged: authorityBefore !== null && JSON.stringify(authorityBefore) === JSON.stringify(authorityAfter),
  status: rustHealth.status === 'PASS' && Boolean(noAuthorityLine) && JSON.stringify(authorityBefore) === JSON.stringify(authorityAfter) ? 'PASS' : 'FAIL',
}, rustHealth)
writeEvidence('B13_R1_STALE_CACHE.json', {
  schema: 'b13-r1-stale-cache',
  cacheMode: 'ON_DEMAND',
  staleCacheTest: 'NOT_APPLICABLE',
  reason: 'formal health truth does not read persisted cache',
  status: rustHealth.status === 'PASS' ? 'PASS' : 'FAIL',
}, rustHealth)
const projectionRebuildLine = rustHealth.output.match(/B13_PROJECTION_REBUILD_RESULT[^\n]*/)?.[0] ?? null
writeEvidence('B13_R1_PROJECTION_REBUILD.json', {
  schema: 'b13-r1-projection-rebuild',
  cacheMode: 'PROJECTION_ONLY',
  rebuildMode: 'ON_DEMAND',
  observed: projectionRebuildLine,
  deletingProjectionRowsDoesNotChangeTruth: Boolean(projectionRebuildLine && /sameOutput=PASS/.test(projectionRebuildLine)),
  status: rustHealth.status === 'PASS' && Boolean(projectionRebuildLine) ? 'PASS' : 'FAIL',
}, rustHealth)

const changedFiles = gitText(['diff', '--name-status', `${repairBaselineCommit}..${sourceCommit}`])
  .split('\n').filter(Boolean)
  .map(line => {
    const [status, ...parts] = line.split('\t')
    const file = parts.at(-1)
    const task = /health|b13_health|b13_scale/.test(file) ? 'B13-R1-01/03'
      : /LongFormHealth|healthLanguage|b13-r1-health-presentation/.test(file) ? 'B13-R1-04'
        : 'B13-R1-05'
    return { status, file, task }
  })
const changedOutOfScope = changedFiles.filter(item => !item.file || item.file.includes('OUT_OF_SCOPE'))
const secretLeaks = []
const secretTextExtensions = /\.(?:rs|ts|vue|mjs|json|md|toml|sql)$/
const scanSecrets = directory => {
  for (const name of fs.readdirSync(directory)) {
    const file = path.join(directory, name)
    if (fs.statSync(file).isDirectory()) scanSecrets(file)
    else if (secretTextExtensions.test(file) && /sk-[A-Za-z0-9]{20,}|Bearer\s+[A-Za-z0-9._-]{20,}|OPENAI_API_KEY\s*=/.test(fs.readFileSync(file, 'utf8'))) {
      secretLeaks.push(path.relative(root, file))
    }
  }
}
for (const relative of ['src', 'src-tauri/src', 'contracts', 'scripts', 'tests']) scanSecrets(path.join(root, relative))
writeEvidence('B13_R1_CHANGED_FILES.json', {
  schema: 'b13-r1-changed-files',
  diffBase: repairBaselineCommit,
  paths: changedFiles,
  outOfScope: changedOutOfScope,
  status: changedOutOfScope.length === 0 ? 'PASS' : 'FAIL',
})
writeEvidence('B13_R1_SECRET_SCAN.json', {
  schema: 'b13-r1-secret-scan',
  realSecretFindings: secretLeaks,
  status: secretLeaks.length === 0 ? 'PASS' : 'FAIL',
})

const migrationDetails = frozenMigrations.map(name => {
  const relative = `src-tauri/migrations/${name}`
  const beforeBytes = gitShowBytes(repairBaselineCommit, relative)
  const afterBytes = fs.existsSync(path.join(root, relative)) ? fs.readFileSync(path.join(root, relative)) : null
  const beforeSha256 = beforeBytes ? sha256Bytes(beforeBytes) : null
  const afterSha256 = afterBytes ? sha256Bytes(afterBytes) : null
  return { name, beforeSha256, afterSha256, equal: beforeSha256 !== null && beforeSha256 === afterSha256, status: beforeSha256 !== null && beforeSha256 === afterSha256 ? 'PASS' : 'FAIL' }
})
writeEvidence('B13_R1_MIGRATION_FREEZE.json', {
  schema: 'b13-r1-migration-freeze',
  frozenThrough: '0012',
  migrations: migrationDetails,
  historicalMigrationsUnchanged: migrationDetails.every(item => item.equal),
  status: migrationDetails.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
}, architecture)

writeEvidence('B13_R1_PREBATCH_FREEZE.json', {
  schema: 'b13-r1-prebatch-freeze',
  repairBaselineCommit,
  postB12FrozenCommit,
  originalInstructionPath,
  repairInstructionPath,
  b01ToB12Frozen: true,
  doNotStart: ['B14', 'B15', 'RC1'],
  status: statusBefore === '' && sourceCommit !== repairBaselineCommit ? 'PASS' : 'FAIL',
}, architecture)
writeEvidence('B13_R1_ARCHITECTURE.json', architectureReport, architecture)
writeEvidence('B13_R1_CONTRACTS.json', {
  schema: 'b13-r1-contracts',
  contract: 'LongFormHealthProjectionR1',
  status: contracts.status === 'PASS' ? 'PASS' : 'FAIL',
}, contracts)
writeEvidence('B13_R1_PRODUCT_IDENTITY.json', {
  schema: 'b13-r1-product-identity',
  productionProjectionProvidesDisplayMetadata: architectureReport.checks.r1_volume_display_metadata === 'PASS',
  dynamicPresentationTest: 'tests/store/b13-r1-health-presentation.test.ts',
  productUxStatus: productUxReport.status,
  status: productUxCommand.status === 'PASS' && productUxReport.status === 'PASS' ? 'PASS' : 'FAIL',
}, productUxCommand)
writeEvidence('B13_R1_CHAPTER_IDENTITY.json', {
  schema: 'b13-r1-chapter-identity',
  formalResolution: 'JOIN chapters.chapter_id -> chapters.chapter_no',
  testNames: [
    'b13_r1_opaque_chapter_references_are_resolved_by_chapter_row',
    'b13_r1_foreshadow_and_reader_refs_use_joined_opaque_chapter_numbers',
  ],
  status: rustHealth.status === 'PASS' && architectureReport.checks.r1_chapter_identity === 'PASS' ? 'PASS' : 'FAIL',
}, rustHealth)
writeEvidence('B13_R1_PLAN_STALE_SEMANTICS.json', {
  schema: 'b13-r1-plan-stale-semantics',
  owner: 'B06/B07::PlanStale',
  revisionOnlyChangeIsValid: Boolean(planStaleLine && /revisionOnly=PASS/.test(planStaleLine)),
  digestChangeIsStale: Boolean(planStaleLine && /digestChanged=PASS/.test(planStaleLine)),
  status: rustHealth.status === 'PASS' && architectureReport.checks.r1_plan_stale_owner === 'PASS' && Boolean(planStaleLine) ? 'PASS' : 'FAIL',
}, rustHealth)
writeEvidence('B13_R1_GLOBAL_TRUTH_SCALE.json', {
  schema: 'b13-r1-global-truth-scale',
  ...scaleData,
  queryPlans: queryPlansValid,
  status: scaleData.status === 'PASS' && queryPlansValid ? 'PASS' : 'FAIL',
}, rustScale)

const mutationResults = ['MUT-R1-01', 'MUT-R1-02', 'MUT-R1-03', 'MUT-R1-04'].map(id => {
  const file = path.join(evidenceDir, 'MUTATIONS', id, 'result.json')
  return fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : { id, status: 'FAIL' }
})
writeEvidence('B13_R1_MUTATION_GATES.json', {
  schema: 'b13-r1-mutation-gates',
  mutations: mutationResults,
  status: mutations.status === 'PASS' && mutationResults.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
}, mutations)

const manualEvidencePath = path.join(evidenceDir, 'B13_R1_MANUAL_WALKTHROUGH.json')
if (fs.existsSync(manualEvidencePath)) {
  const manualEvidence = JSON.parse(fs.readFileSync(manualEvidencePath, 'utf8'))
  manualEvidence.command = browser.command
  manualEvidence.startedAt = browser.startedAt
  manualEvidence.finishedAt = browser.finishedAt
  manualEvidence.exitCode = browser.exitCode
  manualEvidence.logPath = browser.logPath
  manualEvidence.status = browser.status === 'PASS' && manualEvidence.status === 'PASS' ? 'PASS' : 'FAIL'
  fs.writeFileSync(manualEvidencePath, `${JSON.stringify(manualEvidence, null, 2)}\n`)
}

const historicalEvidenceFiles = [
  'evidence/B01/registry_integrity.json',
  'evidence/B01/schema_inventory.json',
]
const historicalEvidenceRestoration = historicalEvidenceFiles.map(file => {
  const changed = gitResult(['diff', '--quiet', '--', file]).status !== 0
  if (!changed) return { file, changed: false, status: 'UNCHANGED' }
  const restored = gitResult(['restore', '--source', sourceCommit, '--', file])
  return { file, changed: true, status: restored.status === 0 ? 'RESTORED' : 'FAIL' }
})
writeEvidence('B13_R1_HISTORICAL_EVIDENCE_RESTORATION.json', {
  schema: 'b13-r1-historical-evidence-restoration',
  files: historicalEvidenceRestoration,
  noHistoricalEvidenceDiff: historicalEvidenceRestoration.every(item => gitResult(['diff', '--quiet', '--', item.file]).status === 0),
  status: historicalEvidenceRestoration.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL',
})

const sourceAfterCommit = gitText(['rev-parse', 'HEAD'])
const statusAfter = gitText(['status', '--porcelain', '--untracked-files=all'])
const sourceBinding = {
  schema: 'b13-r1-source-binding',
  startCommit: sourceCommit,
  repairBaselineCommit,
  sourceCommit,
  evidenceSourceCommit: sourceCommit,
  sourceCommitAfter: sourceAfterCommit,
  workingTreeDirtyBefore: statusBefore !== '',
  workingTreeDirtyAfter: statusAfter !== '',
  excludedUncommittedPaths: [],
  status: statusBefore === '' && statusAfter === '' && sourceCommit === sourceAfterCommit ? 'PASS' : 'FAIL',
}
writeEvidence('B13_R1_SOURCE_BINDING.json', sourceBinding)

const truthfulnessCommand = run('evidence_truthfulness', 'node', ['scripts/verify-b13-evidence-truthfulness.mjs'], {
  B13_R1_EVIDENCE_DIR: evidenceDir,
  B13_R1_SOURCE_COMMIT: sourceCommit,
})
const truthfulness = evaluateEvidenceTruthfulness(evidenceDir, sourceCommit)
writeEvidence('B13_R1_EVIDENCE_TRUTHFULNESS.json', {
  ...truthfulness,
  status: truthfulnessCommand.status === 'PASS' && truthfulness.status === 'PASS' ? 'PASS' : 'FAIL',
}, truthfulnessCommand)
writeEvidence('B13_R1_FINAL_DELIVERY_STATUS.json', {
  schema: 'b13-r1-final-delivery-status',
  deliveryStatus: 'FAIL',
  nextBatchAllowed: false,
  localEvidenceOnly: true,
})
writeEvidence('B13_R1_REGRESSION.json', {
  schema: 'b13-r1-regression',
  node: node.status,
  vitest: vitest.status,
  rustAllTargets: rustAll.status,
  B01_B12: rustAll.status,
  status: [node, vitest, rustAll, rustFmt, rustCheck, rustClippy, contracts, typecheck, build, buildDemo].every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
}, rustAll)
const gates = {
  b13_r1_01_chapter_identity: gate(rustHealth.status === 'PASS' && architectureReport.checks.r1_chapter_identity === 'PASS', 'B13_R1_CHAPTER_IDENTITY.json'),
  b13_r1_02_plan_stale_semantics: gate(rustHealth.status === 'PASS' && architectureReport.checks.r1_plan_stale_owner === 'PASS', 'B13_R1_PLAN_STALE_SEMANTICS.json'),
  b13_r1_03_global_truth_scale: gate(scaleData.status === 'PASS' && queryPlansValid && architectureReport.checks.r1_global_truth_aggregate === 'PASS', 'B13_R1_GLOBAL_TRUTH_SCALE.json'),
  b13_r1_04_product_identity: gate(productUxCommand.status === 'PASS' && productUxReport.status === 'PASS' && architectureReport.checks.r1_volume_display_metadata === 'PASS', 'B13_R1_PRODUCT_IDENTITY.json'),
  b13_r1_05_evidence_truthfulness: gate(truthfulness.status === 'PASS' && truthfulnessCommand.status === 'PASS', 'B13_R1_EVIDENCE_TRUTHFULNESS.json'),
  architecture: gate(architecture.status === 'PASS' && architectureReport.status === 'PASS', 'B13_R1_ARCHITECTURE.json'),
  module_boundary: gate(moduleBoundary.status === 'PASS', 'AUTOMATIC/module_boundary.log', { exitCode: moduleBoundary.exitCode }),
  product_ux: gate(productUx.status === 'PASS' && productUxReport.status === 'PASS', 'B13_R1_PRODUCT_IDENTITY.json'),
  B10_boundary: gate(!changedFiles.some(item => /B10|creative-skills/.test(item.file)), 'B13_R1_CHANGED_FILES.json'),
  B12_boundary: gate(!changedFiles.some(item => /B12|change_retcon/.test(item.file)), 'B13_R1_CHANGED_FILES.json'),
  B14_boundary: gate(!changedFiles.some(item => /B14|Endgame|Completion/.test(item.file)), 'B13_R1_PREBATCH_FREEZE.json'),
  B15_boundary: gate(!changedFiles.some(item => /B15|ExportRelease/.test(item.file)), 'B13_R1_PREBATCH_FREEZE.json'),
  RC1_boundary: gate(!changedFiles.some(item => /RC1/.test(item.file)), 'B13_R1_PREBATCH_FREEZE.json'),
  historical_evidence_restoration: gate(historicalEvidenceRestoration.every(item => item.status !== 'FAIL'), 'B13_R1_HISTORICAL_EVIDENCE_RESTORATION.json'),
  rust_fmt: gate(rustFmt.status === 'PASS', 'AUTOMATIC/rust_fmt.log', { exitCode: rustFmt.exitCode }),
  rust_check: gate(rustCheck.status === 'PASS', 'AUTOMATIC/rust_check.log', { exitCode: rustCheck.exitCode }),
  rust_clippy: gate(rustClippy.status === 'PASS', 'AUTOMATIC/rust_clippy.log', { exitCode: rustClippy.exitCode }),
  rust_tests: gate(rustAll.status === 'PASS' && rustHealth.status === 'PASS' && rustScale.status === 'PASS', 'AUTOMATIC/rust_all_targets.log', { exitCode: rustAll.exitCode }),
  node_tests: gate(node.status === 'PASS' && b13Node.status === 'PASS', 'AUTOMATIC/node.log+AUTOMATIC/b13_node.log'),
  vitest: gate(vitest.status === 'PASS', 'AUTOMATIC/vitest.log', { exitCode: vitest.exitCode }),
  typecheck: gate(typecheck.status === 'PASS', 'AUTOMATIC/typecheck.log', { exitCode: typecheck.exitCode }),
  build: gate(build.status === 'PASS' && buildDemo.status === 'PASS', 'AUTOMATIC/build.log+AUTOMATIC/build_demo.log'),
  contracts: gate(contracts.status === 'PASS', 'AUTOMATIC/contracts.log', { exitCode: contracts.exitCode }),
  package_preflight: gate(packagePreflight.status === 'PASS', 'AUTOMATIC/package_preflight.log', { exitCode: packagePreflight.exitCode }),
  secret_scan: gate(secretLeaks.length === 0, 'B13_R1_SECRET_SCAN.json', { findings: secretLeaks }),
  source_binding: gate(sourceBinding.status === 'PASS', 'B13_R1_SOURCE_BINDING.json'),
  manual_browser: gate(browser.status === 'PASS' && truthfulness.checks.manualBrowser === true, 'B13_R1_MANUAL_WALKTHROUGH.json'),
  real_mutations: gate(mutations.status === 'PASS' && truthfulness.checks.realMutations === true, 'B13_R1_MUTATION_GATES.json'),
  no_remote_model_call: gate(truthfulness.checks.noRemoteModelCall === true, 'B13_R1_NO_REMOTE_CALL.json'),
  no_authority_mutation: gate(truthfulness.checks.noAuthorityMutation === true, 'B13_R1_NO_AUTHORITY_MUTATION.json'),
  query_plans: gate(queryPlansValid, 'B13_R1_QUERY_PLANS.json'),
}
const gateStatus = Object.values(gates).every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
const finalStatus = gateStatus === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL'
const finalEvidencePath = path.join(evidenceDir, 'B13_R1_FINAL_DELIVERY_STATUS.json')
const finalEvidence = JSON.parse(fs.readFileSync(finalEvidencePath, 'utf8'))
finalEvidence.deliveryStatus = finalStatus
finalEvidence.nextBatchAllowed = false
finalEvidence.status = gateStatus
fs.writeFileSync(finalEvidencePath, `${JSON.stringify(finalEvidence, null, 2)}\n`)

const gateResults = {
  schema: 'b13-r1-gate-results',
  batch: 'B13-R1',
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
  status: gateStatus,
  deliveryStatus: finalStatus,
  nextBatchAllowed: false,
}
fs.writeFileSync(path.join(evidenceDir, 'gate-results.json'), `${JSON.stringify(gateResults, null, 2)}\n`)
fs.writeFileSync(path.join(evidenceDir, 'COMMANDS_EXECUTED.json'), `${JSON.stringify({ schema: 'b13-r1-commands-executed', sourceCommit, commands }, null, 2)}\n`)
console.log(`B13_R1_EVIDENCE_RESULT sourceCommit=${sourceCommit} gates=${Object.values(gates).filter(item => item.status === 'PASS').length}/${Object.keys(gates).length} status=${gateStatus} evidenceDir=${evidenceDir}`)
if (gateStatus !== 'PASS') process.exitCode = 1
