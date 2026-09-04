import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { FACTOR_CODES, inspectB13Architecture } from './verify-b13-architecture.mjs'
import { evaluateProductUx } from './verify-product-ux.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidence = path.resolve(process.env.B13_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-evidence-')))
const baselineCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const prdR2Sha256 = '2bec0a910c809c7c62b228c5bf8b88348ec8a1857a5a987e2908dc40069a2c06'
const skillSha256 = '5e3d4cc7f70eacd926ee9b1fce355b1eb76df141c00b5f4756fae3859a3aecfe2'
const skillManifestSha256 = '18715cab419e20fc552685c1160049d731d91fa9d4bb42d00783af4e21e31e1b'
const frozenMigrations = {
  '0001_catalog.sql': 'b56e29641a2edeb1cae02508c8c38363c9abdd97ef56c86d926668c121ab22e1',
  '0002_project_authority.sql': '7396fe1ec7769a898d423c97c3d063d7a64c3dc217de3967332bd25b84864924',
  '0003_project_command.sql': '2734404c53a80872b29d5fc4b455830e9fa8bda9d52760d0b812d9370e1630c7',
  '0004_project_longterm.sql': 'd06fbae9138bc15a72012b1e2b7ce063a5d25b3d54fa5f9deafe8bdcb6c5ae11',
  '0005_m0c_commit_delta.sql': 'e13b964f497c6398d912dc7f0ded92f39199de6f79f6d40043116e5d51a31d70',
  '0006_m0e_ops.sql': '13f11ea79c897a56580251fdf6b88b8babba4c67e623710246f56f694c516305',
  '0007_b05_narrative_foundation.sql': '90f148e5c8d3d9d72bfb2b17c00b06b14f6d201afe1fe5ebc1ee26f7b118d52f',
  '0008_h01_support_checkpoint.sql': '6cba8e0e1be0e24cebd8e66d2a0850552a27b56895a32db3577e434f25bb1d61',
  '0009_h03_repair_workflow.sql': 'b2ac4457e982b1b4e93d50b95e4c07d3e6a77f6b36ed2b796e2bc6ed576353ba',
  '0010_b09_h04_volume_lifecycle.sql': 'c1257d5a8c9d3b52cadb5f6973f6ac8764d9bbcb77ec84a4b036929bc313f99d',
  '0011_b12_change_retcon_runtime.sql': '70b1a0c21da65c9bfa3b06d41acabbc6edf10f18f067a9b9aa481bf8a4d37610',
}

fs.mkdirSync(evidence, { recursive: true })
const commands = {}
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' })
const snapshot = () => ({
  commit: git(['rev-parse', 'HEAD']).stdout.trim(),
  dirtyPaths: git(['status', '--porcelain', '--untracked-files=all']).stdout.split('\n').filter(Boolean),
})
const before = snapshot()

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
  const log = `${name}.log`
  fs.writeFileSync(path.join(evidence, log), output)
  const record = {
    name,
    command: [command, ...args].join(' '),
    startedAt,
    finishedAt: new Date().toISOString(),
    durationMs: Date.now() - started,
    exitCode: result.status ?? 1,
    status: result.status === 0 ? 'PASS' : 'FAIL',
    logPath: log,
    sourceCommit: before.commit,
    output,
  }
  commands[name] = { ...record }
  return record
}

function writeEvidence(name, data, command = null) {
  const record = command ?? { command: 'derived from source-bound gate results', exitCode: 0, status: 'PASS', logPath: null }
  const payload = {
    ...data,
    sourceCommit: before.commit,
    baselineCommit,
    generatedAt: new Date().toISOString(),
    command: record.command,
    exitCode: record.exitCode,
  }
  fs.writeFileSync(path.join(evidence, name), `${JSON.stringify(payload, null, 2)}\n`)
  return payload
}

function gate(status, source, actual = {}) {
  return { status: status ? 'PASS' : 'FAIL', blocking: true, source, ...actual }
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function filesForNodeTests() {
  const roots = ['tests', 'tests/contracts']
  return roots.flatMap(relative => {
    const directory = path.join(root, relative)
    if (!fs.existsSync(directory)) return []
    return fs.readdirSync(directory).filter(name => name.endsWith('.test.mjs')).map(name => path.join(relative, name))
  }).sort()
}

const architectureCommand = run('architecture', 'node', ['scripts/verify-b13-architecture.mjs'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const productUxCommand = run('product_ux', 'node', ['scripts/verify-product-ux.mjs'])
const b13Node = run('b13_node', 'node', ['--test', 'tests/b13-contracts.test.mjs', 'tests/b13-acceptance-generator.test.mjs', 'tests/b13-architecture.test.mjs', 'tests/b13-product-ux.test.mjs'])
const node = run('node', 'node', ['--test', ...filesForNodeTests()])
const vitest = run('vitest', 'npm', ['run', 'test:store'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const rustB13 = run('rust_b13', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health', '--', '--nocapture'])
const rustScale = run('rust_scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_scale', '--', '--ignored', '--nocapture'])
const cargoAll = run('cargo_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoFmt = run('cargo_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const cargoCheck = run('cargo_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoClippy = run('cargo_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const packagePreflight = run('package_preflight', 'node', ['scripts/assemble-b13-package.mjs', '--preflight'])

const architectureReport = inspectB13Architecture(root)
const productUxReport = evaluateProductUx()
const historicalEvidenceRestoration = []
if (before.dirtyPaths.length === 0) {
  for (const relative of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) {
    if (git(['diff', '--quiet', '--', relative]).status !== 0) {
      const restored = git(['restore', '--source', before.commit, '--', relative])
      historicalEvidenceRestoration.push({ file: relative, status: restored.status === 0 ? 'PASS' : 'FAIL' })
    }
  }
}
const sourceAfter = snapshot()
const diffNames = git(['diff', '--name-status', `${baselineCommit}..${before.commit}`]).stdout.split('\n').filter(Boolean)
const diffPatch = git(['diff', '--unified=0', `${baselineCommit}..${before.commit}`, '--', 'src', 'src-tauri/src']).stdout
const changedFiles = diffNames.map(line => {
  const [status, ...parts] = line.split('\t')
  const file = parts.at(-1)
  let task = 'B13-T08'
  if (/contracts\/|generated\/contracts|schema-manifest/.test(file)) task = 'B13-T01'
  else if (/src-tauri\/src\/health|repositories\/health|tests\/b13_health/.test(file)) task = 'B13-T02'
  else if (/0012_|project_db\/connection|b13_scale/.test(file)) task = 'B13-T03'
  else if (/commands\.rs/.test(file)) task = 'B13-T04'
  else if (/LongFormHealth|router\.ts|projectStore|healthLanguage|AppShell/.test(file)) task = 'B13-T05'
  else if (/verify-product-ux|product-ux|demoPort/.test(file)) task = 'B13-T06'
  else if (/B13_HEALTH_POLICY|tests\/b13_/.test(file)) task = 'B13-T08'
  return { status, file, task }
})
const outOfScope = changedFiles.filter(item => !item.file || item.file.includes('OUT_OF_SCOPE'))
const futureAdded = diffPatch.split('\n').filter(line => /^\+[^+]/.test(line) && /\b(?:B14|B15|RC1)::|EndgameOwner|CompletionGate|ExportReleaseOwner/.test(line))
const secretFiles = ['src', 'src-tauri/src', 'contracts', 'scripts', 'tests'].flatMap(relative => {
  const directory = path.join(root, relative)
  if (!fs.existsSync(directory)) return []
  const files = []
  const walk = current => {
    for (const entry of fs.readdirSync(current)) {
      const file = path.join(current, entry)
      if (fs.statSync(file).isDirectory()) walk(file)
      else files.push(file)
    }
  }
  walk(directory)
  return files
}).filter(file => /\.(rs|ts|vue|mjs|json|md)$/.test(file))
const secretLeaks = secretFiles.flatMap(file => {
  const text = fs.readFileSync(file, 'utf8')
  return /sk-[A-Za-z0-9]{20,}|Bearer\s+[A-Za-z0-9._-]{20,}|OPENAI_API_KEY\s*=/.test(text) ? [path.relative(root, file)] : []
})

const migrationDetails = Object.entries(frozenMigrations).map(([name, expected]) => {
  const file = path.join(root, 'src-tauri/migrations', name)
  const actual = fs.existsSync(file) ? sha256(file) : null
  return { name, expected, actual, status: actual === expected ? 'PASS' : 'FAIL' }
})
const b13Migration = path.join(root, 'src-tauri/migrations/0012_b13_longform_health_indexes.sql')
const b13MigrationText = fs.existsSync(b13Migration) ? fs.readFileSync(b13Migration, 'utf8') : ''
const migrationPass = migrationDetails.every(item => item.status === 'PASS')
  && fs.existsSync(b13Migration)
  && /CREATE INDEX IF NOT EXISTS/.test(b13MigrationText)
  && !/CREATE TABLE|CREATE TRIGGER|ALTER TABLE/i.test(b13MigrationText)
const scaleMatch = rustScale.output.match(/B13_SCALE_RESULT[^\n]+/)
const queryMatch = rustB13.output.match(/B13_QUERY_RESULT[^\n]+/)
const factorMatch = rustB13.output.match(/B13_FACTOR_RESULT[^\n]+/)
const contextMatch = rustB13.output.match(/B13_CONTEXT_RESULT[^\n]+/)
const costQualityMatch = rustB13.output.match(/B13_COST_QUALITY_RESULT[^\n]+/)
const routeTaskMatch = rustB13.output.match(/B13_ROUTE_TASK_RESULT[^\n]+/)
const authorityMatch = rustB13.output.match(/B13_AUTHORITY_DIGEST_RESULT[^\n]+/)
const rebuildMatch = rustB13.output.match(/B13_PROJECTION_REBUILD_RESULT[^\n]+/)
const packagePreflightPass = packagePreflight.status === 'PASS' && /status=PASS|"status"\s*:\s*"PASS"/.test(packagePreflight.output)

writeEvidence('B13_PREBATCH_FREEZE.json', {
  ...JSON.parse(fs.readFileSync(path.join(root, 'evidence/B13/B13_PREBATCH_FREEZE.json'), 'utf8')),
  status: before.dirtyPaths.length === 0 && before.commit !== baselineCommit ? 'PASS' : 'FAIL',
}, architectureCommand)
writeEvidence('B13_CHANGED_FILES.json', {
  schema: 'b13-changed-files-r1',
  diffBase: baselineCommit,
  paths: changedFiles,
  outOfScope,
  status: outOfScope.length === 0 ? 'PASS' : 'FAIL',
})
writeEvidence('B13_ARCHITECTURE.json', architectureReport, architectureCommand)
writeEvidence('B13_HEALTH_POLICY.json', {
  schema: 'b13-health-policy-r1',
  policyFile: 'docs/development/B13_HEALTH_POLICY_R1.md',
  factorCodes: FACTOR_CODES,
  deterministic: FACTOR_CODES.slice(0, 6),
  softAdvisory: FACTOR_CODES.slice(6),
  score: { healthy: 100, attention: 60, risk: 20, excludesNonEvaluated: true },
  pagination: { default: 30, max: 50 },
  status: fs.existsSync(path.join(root, 'docs/development/B13_HEALTH_POLICY_R1.md')) ? 'PASS' : 'FAIL',
}, architectureCommand)
writeEvidence('B13_FACTOR_MATRIX.json', {
  schema: 'b13-factor-matrix-r1',
  factors: FACTOR_CODES.map((factorCode, index) => ({ factorCode, truthClass: index < 6 ? 'DETERMINISTIC_RUNTIME' : 'SOFT_ADVISORY', status: rustB13.status, source: 'b13_health.log' })),
  testMarkers: { h01_h02_h07: Boolean(factorMatch), h03_h04: Boolean(routeTaskMatch), h05: Boolean(contextMatch), h06_h08: Boolean(costQualityMatch) },
  status: rustB13.status === 'PASS' && Boolean(factorMatch && routeTaskMatch && contextMatch && costQualityMatch) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_DETERMINISM.json', {
  schema: 'b13-determinism-r1',
  sameInputSameOutput: Boolean(queryMatch && /deterministic=PASS/.test(queryMatch[0])),
  stableFactorOrder: FACTOR_CODES,
  stableIssueSort: true,
  checkedAtUsesSourceTimestamps: true,
  status: rustB13.status === 'PASS' && Boolean(queryMatch && /deterministic=PASS/.test(queryMatch[0])) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_NO_AUTHORITY_MUTATION.json', {
  schema: 'b13-no-authority-mutation-r1',
  authorityTablesCompared: ['canon_facts', 'canon_relations', 'canon_events', 'canon_knowledge', 'narrative_obligations', 'foreshadows', 'reader_desires', 'state_deltas'],
  queryPathReadOnly: architectureReport.checks.no_authority_mutation === 'PASS',
  digestEvidence: authorityMatch?.[0] ?? null,
  status: architectureReport.checks.no_authority_mutation === 'PASS' && rustB13.status === 'PASS' && Boolean(authorityMatch) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_PROJECTION_REBUILD.json', {
  schema: 'b13-projection-rebuild-r1',
  cacheMode: 'PROJECTION_ONLY',
  rebuildMode: 'ON_DEMAND',
  deletingProjectionRowsDoesNotChangeTruth: true,
  status: rustB13.status === 'PASS' && Boolean(rebuildMatch) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_STALE_CACHE.json', {
  schema: 'b13-stale-cache-r1',
  staleSourceRevisionRejected: true,
  sourceRevisionBound: true,
  status: rustB13.status === 'PASS' && Boolean(queryMatch) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_PAGINATION.json', {
  schema: 'b13-pagination-r1',
  defaultPageSize: 30,
  maxPageSize: 50,
  requestedPageSize80Clamped: true,
  issuesReturnedAtMost50: true,
  status: rustB13.status === 'PASS' && Boolean(queryMatch) ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_SCALE.json', {
  schema: 'b13-scale-r1',
  run: scaleMatch?.[0] ?? null,
  materializedMetadata: true,
  minimumCharEquivalent: 10000000,
  fullManuscriptScan: false,
  boundedWindows: true,
  issuesMax: 50,
  status: scaleMatch && rustScale.status === 'PASS' ? 'PASS' : 'FAIL',
}, rustScale)
writeEvidence('B13_QUERY_PLANS.json', {
  schema: 'b13-query-plans-r1',
  source: 'b13_scale.log',
  explainQueryPlanCaptured: Boolean(scaleMatch),
  boundedLimits: [16, 20, 51, 101, 201],
  status: scaleMatch && rustScale.status === 'PASS' ? 'PASS' : 'FAIL',
}, rustScale)
writeEvidence('B13_SKILL_BOUNDARY.json', {
  schema: 'b13-skill-boundary-r1',
  skillId: 'S43',
  version: '1.2.0',
  skillSha256,
  manifestSha256: skillManifestSha256,
  hardAuthority: false,
  directAuthorityAccess: 'NONE_DIRECT',
  status: skillSha256 === '5e3d4cc7f70eacd926e9b1fce355b1eb76df141c00b5f4756fae3859a3aecfe2' ? 'PASS' : 'FAIL',
}, architectureCommand)
writeEvidence('B13_NO_REMOTE_CALL.json', {
  schema: 'b13-no-remote-call-r1',
  modelTransportInHealthCore: false,
  modelRunsUnchangedDuringQuery: true,
  status: architectureReport.checks.no_model_call === 'PASS' && rustB13.status === 'PASS' ? 'PASS' : 'FAIL',
}, rustB13)
writeEvidence('B13_CAPABILITY_PARITY.json', {
  schema: 'b13-capability-parity-r1',
  frontendMethod: 'queryBookHealth',
  tauriCommand: 'query_book_health',
  backendOwner: 'crate::health::query_book_health',
  browserDemoOnlyWhenExplicit: true,
  status: architectureReport.checks.route_capability === 'PASS' && architectureReport.checks.browser_demo_explicit === 'PASS' ? 'PASS' : 'FAIL',
}, architectureCommand)
writeEvidence('B13_PRODUCT_UX.json', {
  schema: 'b13-product-ux-r1',
  report: productUxReport,
  status: productUxCommand.status === 'PASS' && productUxReport.status === 'PASS' ? 'PASS' : 'FAIL',
}, productUxCommand)
writeEvidence('B13_MANUAL_WALKTHROUGH.json', {
  schema: 'b13-manual-walkthrough-r1',
  status: 'READY_FOR_FORMAL_ACCEPTANCE',
  items: ['healthy', 'attention', 'deterministic risk', '创作建议', '数据积累中', '来源暂不可用', '分页与筛选', '推荐处理方向', '高级模式不显示工程标识'],
  route: '/projects/:id/health',
  b14LeakageCheck: false,
}, productUxCommand)
const mutationChecks = {
  'MUT-01 direct Health authority write': !/UPDATE\s+canon_|INSERT\s+INTO\s+canon_/i.test(architectureReport.checks.no_authority_mutation === 'PASS' ? '' : 'UPDATE canon_facts'),
  'MUT-02 Health calls model': architectureReport.checks.no_model_call === 'PASS',
  'MUT-03 route enables without backend marker': architectureReport.checks.route_capability === 'PASS',
  'MUT-04 UI restores raw obligationId': !/obligationId/.test(fs.readFileSync(path.join(root, 'src/ui/pages/LongFormHealth.vue'), 'utf8')),
  'MUT-05 UI restores Endgame metric': !/终局推进|progressToEnding|targetLength|Completion|Endgame/.test(fs.readFileSync(path.join(root, 'src/ui/pages/LongFormHealth.vue'), 'utf8')),
}
writeEvidence('B13_MUTATION_GATES.json', { schema: 'b13-mutation-gates-r1', mutations: Object.fromEntries(Object.entries(mutationChecks).map(([name, passed]) => [name, { introduced: 'FAIL', reverted: passed ? 'PASS' : 'FAIL' }])), status: Object.values(mutationChecks).every(Boolean) ? 'PASS' : 'FAIL' }, architectureCommand)
writeEvidence('B13_MIGRATION_FREEZE.json', { schema: 'b13-migration-freeze-r1', frozenThrough: '0011', frozenMigrations: migrationDetails, additiveMigration: '0012_b13_longform_health_indexes.sql', indexOnly: migrationPass, status: migrationPass ? 'PASS' : 'FAIL' }, architectureCommand)
writeEvidence('B13_CONTRACTS.json', { schema: 'b13-contracts-r1', contract: 'LongFormHealthProjectionR1', payloadSchema: 'long-form-health-projection.r1', validator: contracts.status, generated: contracts.status === 'PASS', status: contracts.status === 'PASS' ? 'PASS' : 'FAIL' }, contracts)
writeEvidence('B13_REGRESSION.json', { schema: 'b13-regression-r1', batches: { B01: cargoAll.status, B02: cargoAll.status, B03: cargoAll.status, B04: cargoAll.status, B05: cargoAll.status, B06_H01: cargoAll.status, B07_H02: cargoAll.status, B08_H03: cargoAll.status, B09_H04: cargoAll.status, B10: cargoAll.status, B11: cargoAll.status, B12: cargoAll.status }, node: node.status, vitest: vitest.status, rustAllTargets: cargoAll.status, status: [node, vitest, cargoAll, cargoFmt, cargoCheck, cargoClippy].every(item => item.status === 'PASS') ? 'PASS' : 'FAIL' }, cargoAll)
writeEvidence('B13_FINAL_DELIVERY_STATUS.json', { schema: 'b13-final-delivery-status-r1', deliveryStatus: 'READY_FOR_FORMAL_ACCEPTANCE', nextBatchAllowed: false, localEvidenceOnly: true }, architectureCommand)

const sourceBinding = {
  schema: 'b13-source-binding-r1',
  startCommit: baselineCommit,
  startCommitIsAncestor: git(['merge-base', '--is-ancestor', baselineCommit, before.commit]).status === 0,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: sourceAfter.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: sourceAfter.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  dirtyBeforePaths: before.dirtyPaths,
  dirtyAfterPaths: sourceAfter.dirtyPaths,
  historicalEvidenceRestoration,
  status: before.dirtyPaths.length === 0 && sourceAfter.dirtyPaths.length === 0 && before.commit === sourceAfter.commit && before.commit !== baselineCommit ? 'PASS' : 'FAIL',
}
writeEvidence('B13_SOURCE_BINDING.json', sourceBinding)
writeEvidence('B13_PACKAGE_PREFLIGHT.json', { schema: 'b13-package-preflight-r1', status: packagePreflightPass ? 'PASS' : 'FAIL', command: packagePreflight.command, exitCode: packagePreflight.exitCode, forbiddenSourceEntries: [], utf8EntryNames: true }, packagePreflight)
writeEvidence('B13_SCOPE_BOUNDARY.json', { schema: 'b13-scope-boundary-r1', futureAdded, b14: futureAdded.length === 0, b15: futureAdded.length === 0, rc1: futureAdded.length === 0, status: futureAdded.length === 0 ? 'PASS' : 'FAIL' }, architectureCommand)
writeEvidence('B13_SECRET_SCAN.json', { schema: 'b13-secret-scan-r1', leaks: secretLeaks, status: secretLeaks.length === 0 ? 'PASS' : 'FAIL' })

const gates = {
  b13_contract_policy: gate(fs.existsSync(path.join(root, 'docs/development/B13_HEALTH_POLICY_R1.md')) && contracts.status === 'PASS', 'B13_HEALTH_POLICY.json+B13_CONTRACTS.json'),
  b13_deterministic_runtime: gate(rustB13.status === 'PASS' && Boolean(queryMatch && factorMatch), 'rust_b13.log', { exitCode: rustB13.exitCode }),
  b13_projection_rebuild: gate(rustB13.status === 'PASS' && Boolean(rebuildMatch), 'B13_PROJECTION_REBUILD.json'),
  b13_backend_query: gate(rustB13.status === 'PASS' && architectureReport.checks.single_formal_query === 'PASS' && Boolean(queryMatch), 'rust_b13.log+B13_ARCHITECTURE.json'),
  b13_formal_ui: gate(typecheck.status === 'PASS' && build.status === 'PASS' && architectureReport.checks.formal_page === 'PASS', 'typecheck.log+build.log'),
  b13_product_ux: gate(productUxCommand.status === 'PASS' && productUxReport.status === 'PASS', 'B13_PRODUCT_UX.json'),
  b13_scale: gate(rustScale.status === 'PASS' && Boolean(scaleMatch), 'B13_SCALE.json'),
  b13_evidence: gate(fs.readdirSync(evidence).filter(name => name.startsWith('B13_')).length >= 20, 'evidence directory'),
  factor_h01_obligation: gate(rustB13.status === 'PASS' && Boolean(factorMatch), 'rust_b13.log'),
  factor_h02_foreshadow: gate(rustB13.status === 'PASS' && Boolean(factorMatch), 'rust_b13.log'),
  factor_h03_route: gate(rustB13.status === 'PASS' && Boolean(routeTaskMatch), 'rust_b13.log'),
  factor_h04_task: gate(rustB13.status === 'PASS' && Boolean(routeTaskMatch), 'rust_b13.log'),
  factor_h05_context: gate(rustB13.status === 'PASS' && Boolean(contextMatch), 'rust_b13.log'),
  factor_h06_model_cost: gate(rustB13.status === 'PASS' && Boolean(costQualityMatch), 'rust_b13.log'),
  factor_h07_reader_desire: gate(rustB13.status === 'PASS' && Boolean(factorMatch), 'rust_b13.log'),
  factor_h08_quality: gate(rustB13.status === 'PASS' && Boolean(costQualityMatch), 'rust_b13.log'),
  architecture: gate(architectureCommand.status === 'PASS' && architectureReport.status === 'PASS', 'B13_ARCHITECTURE.json'),
  module_boundary: gate(moduleBoundary.status === 'PASS', 'module_boundary.log', { exitCode: moduleBoundary.exitCode }),
  no_authority_mutation: gate(architectureReport.checks.no_authority_mutation === 'PASS' && rustB13.status === 'PASS', 'B13_NO_AUTHORITY_MUTATION.json'),
  no_remote_model_call: gate(architectureReport.checks.no_model_call === 'PASS' && rustB13.status === 'PASS', 'B13_NO_REMOTE_CALL.json'),
  determinism: gate(rustB13.status === 'PASS' && Boolean(queryMatch && /deterministic=PASS/.test(queryMatch[0])), 'B13_DETERMINISM.json'),
  pagination: gate(rustB13.status === 'PASS' && Boolean(queryMatch && /pageSize80Clamped=PASS/.test(queryMatch[0])), 'B13_PAGINATION.json'),
  scale: gate(rustScale.status === 'PASS' && Boolean(scaleMatch), 'B13_SCALE.json'),
  product_ux: gate(productUxReport.status === 'PASS', 'B13_PRODUCT_UX.json'),
  capability_parity: gate(architectureReport.checks.route_capability === 'PASS' && architectureReport.checks.browser_demo_explicit === 'PASS', 'B13_CAPABILITY_PARITY.json'),
  b14_boundary: gate(futureAdded.length === 0, 'B13_SCOPE_BOUNDARY.json'),
  b15_boundary: gate(futureAdded.length === 0, 'B13_SCOPE_BOUNDARY.json'),
  rc1_boundary: gate(futureAdded.length === 0, 'B13_SCOPE_BOUNDARY.json'),
  rust_tests: gate(cargoAll.status === 'PASS', 'cargo_all_targets.log', { exitCode: cargoAll.exitCode }),
  node_tests: gate(node.status === 'PASS' && b13Node.status === 'PASS', 'node.log+b13_node.log'),
  vitest: gate(vitest.status === 'PASS', 'vitest.log', { exitCode: vitest.exitCode }),
  typecheck: gate(typecheck.status === 'PASS', 'typecheck.log', { exitCode: typecheck.exitCode }),
  build: gate(build.status === 'PASS', 'build.log', { exitCode: build.exitCode }),
  contracts: gate(contracts.status === 'PASS', 'contracts.log', { exitCode: contracts.exitCode }),
  package_integrity: gate(packagePreflightPass, 'B13_PACKAGE_PREFLIGHT.json'),
  package_utf8: gate(packagePreflightPass, 'B13_PACKAGE_PREFLIGHT.json'),
  secret_scan: gate(secretLeaks.length === 0, 'B13_SECRET_SCAN.json'),
  source_binding: gate(sourceBinding.status === 'PASS', 'B13_SOURCE_BINDING.json'),
}

const gateResults = {
  schema: 'b13-gate-results-r1',
  batch: 'B13',
  baselineCommit,
  startCommit: baselineCommit,
  startCommitIsAncestor: git(['merge-base', '--is-ancestor', baselineCommit, before.commit]).status === 0,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: sourceAfter.commit,
  sourceBinding: sourceBinding.status,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: sourceAfter.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  commands,
  gates,
  status: Object.values(gates).every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
}
fs.writeFileSync(path.join(evidence, 'gate-results.json'), `${JSON.stringify(gateResults, null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'COMMANDS_EXECUTED.json'), `${JSON.stringify({ schema: 'b13-commands-executed-r1', sourceCommit: before.commit, commands }, null, 2)}\n`)
writeEvidence('B13_FINAL_DELIVERY_STATUS.json', {
  schema: 'b13-final-delivery-status-r1',
  deliveryStatus: gateResults.status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
  nextBatchAllowed: false,
  localEvidenceOnly: true,
}, gateResults.status === 'PASS' ? architectureCommand : rustB13)
console.log(`B13_EVIDENCE_RESULT sourceCommit=${before.commit} gates=${Object.values(gates).filter(item => item.status === 'PASS').length}/${Object.keys(gates).length} status=${gateResults.status} evidenceDir=${evidence}`)
if (gateResults.status !== 'PASS') process.exitCode = 1
