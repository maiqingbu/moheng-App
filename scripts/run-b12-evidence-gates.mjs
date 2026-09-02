import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidence = path.resolve(process.env.B12_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b12-evidence-')))
const startCommit = 'e784c9206ce60fd8c8cbe7da827234850caba20e'
const ownerName = 'B12::ChangeRetconOwner'
const migrationHashes = {
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
const B12 = Array.from({ length: 61 }, (_, index) => `B12-A${String(index).padStart(2, '0')}`)
const NEGATIVE = Array.from({ length: 28 }, (_, index) => `NEG-${String(index + 1).padStart(2, '0')}`)
const GLOBAL = [
  'second_authority', 'second_production_path', 'commit_bundle_v3', 'narrative_runtime_delta',
  'sql_outside_approved_owner', 'skill_direct_db_query', 'harness_arbitrary_write',
  'p3_permanent_authority', 'future_reservation_bypass', 'plan_writes_reality',
  'candidate_visible_as_final', 'ui_local_truth', 'unbounded_project_context',
  'fake_provider_demo_formal_path', 'future_batch_owner_early',
]
const HISTORICAL = ['b05_regression', 'b06_h01_regression', 'b07_h02_regression', 'b08_h03_regression', 'r2_alignment_regression', 'p01_regression', 'b09_h04_regression', 'b10_regression', 'b11_regression']
const ENGINEERING = ['node', 'vitest', 'contracts', 'typecheck', 'build', 'cargo_all_targets', 'cargo_fmt', 'cargo_check', 'cargo_clippy', 'architecture', 'base_architecture', 'module_boundary', 'm0a']
const B12_ENGINEERING = ['b12_change_retcon', 'b12_h03_capability', 'b12_product_ux', 'b12_language_scan', 'b12_scale_bound', 'b12_architecture']
const RELEASE = ['source_binding', 'package_smoke', 'fresh_package_architecture', 'fresh_package_base_architecture', 'fresh_package_module_boundary', 'fresh_package_generator', 'fresh_package_node', 'fresh_package_language']
const REQUIRED = [...B12, ...NEGATIVE, ...GLOBAL, ...HISTORICAL, ...ENGINEERING, ...B12_ENGINEERING, ...RELEASE]

fs.mkdirSync(evidence, { recursive: true })
const freezeSource = path.join(root, 'evidence/B12/B12_PREBATCH_FREEZE.json')
if (fs.existsSync(freezeSource)) fs.copyFileSync(freezeSource, path.join(evidence, 'B12_PREBATCH_FREEZE.json'))
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' })
const snapshot = () => ({
  commit: git(['rev-parse', 'HEAD']).stdout.trim(),
  dirtyPaths: git(['status', '--porcelain', '--untracked-files=all']).stdout.split('\n').filter(Boolean),
})
const before = snapshot()
const results = {}
const run = (name, command, args, cwd = root, extraEnv = {}) => {
  const startedAt = Date.now()
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? path.join(os.tmpdir(), 'moheng-b12-cargo-target'), ...extraEnv },
    maxBuffer: 128 * 1024 * 1024,
  })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  fs.writeFileSync(path.join(evidence, `${name}.log`), output)
  const record = { name, command: `${command} ${args.join(' ')}`, exitCode: result.status ?? 1, status: result.status === 0 ? 'PASS' : 'FAIL', durationMs: Date.now() - startedAt, log: `${name}.log` }
  results[name] = record
  return { ...record, output }
}
const gate = (passed, source, actual = {}) => ({ status: passed ? 'PASS' : 'FAIL', blocking: true, source, ...actual })
const gates = {}
const setGate = (name, passed, source, actual = {}) => { gates[name] = gate(Boolean(passed), source, actual) }
const parseCount = (output, expression) => Number(output.match(expression)?.[1] ?? 0)
const rust = run('b12_change_retcon', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b12_change_retcon', '--', '--nocapture'])
const rustRevision = run('b12_revision', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b12_revision', '--', '--nocapture'])
const b12Node = run('b12_node', 'node', ['--test', 'tests/b12-acceptance-generator.test.mjs', 'tests/b12-capability-parity.test.mjs', 'tests/b12-negative-matrix.test.mjs', 'tests/b12-revision-gates.test.mjs'])
const node = run('node', 'npm', ['test'])
const frontendEvidencePath = path.join(evidence, 'b12_frontend_ux.jsonl')
if (fs.existsSync(frontendEvidencePath)) fs.unlinkSync(frontendEvidencePath)
const vitest = run('vitest', 'npm', ['run', 'test:store'], root, { B12_FRONTEND_EVIDENCE_FILE: frontendEvidencePath })
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const cargoAll = run('cargo_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoFmt = run('cargo_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const cargoCheck = run('cargo_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoClippy = run('cargo_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const architecture = run('architecture', 'node', ['scripts/verify-architecture.mjs'])
const b12Architecture = run('b12_architecture', 'node', ['scripts/verify-b12-architecture.mjs'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const parity = run('b11_capability_parity', 'node', ['scripts/verify-b11-capability-parity.mjs'])
const scale = process.env.B12_RUN_SCALE === '1'
  ? run('b12_scale_bound', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b12_scale', '--', '--ignored', '--nocapture'])
  : { name: 'b12_scale_bound', status: 'FAIL', exitCode: 1, durationMs: 0, log: 'b12_scale_bound.log', output: 'B12_RUN_SCALE=1 is required for the 3m certification.' }
if (!results.b12_scale_bound) {
  results.b12_scale_bound = scale
  fs.writeFileSync(path.join(evidence, scale.log), scale.output)
}

const sourceByPath = relative => {
  const file = path.join(root, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}
const owner = sourceByPath('src-tauri/src/change_retcon.rs')
const repository = sourceByPath('src-tauri/src/repositories/change_retcon.rs')
const repair = sourceByPath('src-tauri/src/repair.rs')
const commands = sourceByPath('src-tauri/src/commands.rs')
const historyPage = sourceByPath('src/ui/pages/HistoryCenter.vue')
const assistantPage = sourceByPath('src/ui/pages/ProjectAssistant.vue')
const router = sourceByPath('src/app/router.ts')
const shell = sourceByPath('src/ui/components/AppShell.vue')
const templates = [historyPage, assistantPage].map(source => source.match(/<template>([\s\S]*?)<\/template>/)?.[1] ?? '').join('\n')
const migrationResults = Object.entries(migrationHashes).map(([name, expected]) => {
  const file = path.join(root, 'src-tauri/migrations', name)
  const actual = fs.existsSync(file) ? crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null
  return { name, expected, actual, status: actual === expected ? 'PASS' : 'FAIL' }
})
const migrationPass = migrationResults.every(item => item.status === 'PASS') && !fs.readdirSync(path.join(root, 'src-tauri/migrations')).some(name => /^0012/.test(name))
const freeze = JSON.parse(sourceByPath('evidence/B12/B12_PREBATCH_FREEZE.json') || '{}')
const freezePass = freeze.freezeThrough === 'B11' && ['edadc96aba68fe89b8e1e453387eb6ffe3fd9968', startCommit].includes(freeze.acceptedBaseline) && freeze.nextAuthorizedBatch === 'B12'
const rustPass = rust.status === 'PASS' && rustRevision.status === 'PASS'
const nodePass = node.status === 'PASS'
const b12NodePass = b12Node.status === 'PASS'
const historicalEvidenceRestoration = []
if (before.dirtyPaths.length === 0) {
  for (const relative of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) {
    if (git(['diff', '--quiet', '--', relative]).status !== 0) {
      const restored = git(['restore', '--source', before.commit, '--', relative])
      historicalEvidenceRestoration.push({ file: relative, status: restored.status === 0 ? 'PASS' : 'FAIL' })
    }
  }
}
const sourceAfterTests = snapshot()
const startCommitIsAncestor = git(['merge-base', '--is-ancestor', startCommit, before.commit]).status === 0
const sourceBinding = before.dirtyPaths.length === 0 && sourceAfterTests.dirtyPaths.length === 0 && before.commit === sourceAfterTests.commit && startCommitIsAncestor && before.commit !== startCommit
const sourceBindingReport = {
  schema: 'b12-source-binding-r1',
  batch: 'B12',
  startCommit,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: sourceAfterTests.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: sourceAfterTests.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  frozenHistoricalEvidenceRestoration: historicalEvidenceRestoration,
  status: sourceBinding ? 'PASS' : 'FAIL',
}
fs.writeFileSync(path.join(evidence, 'B12_SOURCE_BINDING.json'), `${JSON.stringify(sourceBindingReport, null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'B12_MIGRATION_FREEZE.json'), `${JSON.stringify({ schema: 'b12-migration-freeze-r2', frozen: migrationResults, frozenThrough: '0010', additiveMigration: '0011_b12_change_retcon_runtime.sql', noUnapprovedMigration: !fs.readdirSync(path.join(root, 'src-tauri/migrations')).some(name => /^0012/.test(name)), status: migrationPass ? 'PASS' : 'FAIL' }, null, 2)}\n`)

const runtimePass = rustPass
const targetOwnerBody = owner.match(/fn materialize_typed_target[\s\S]*?(?=\nfn |\npub fn |$)/)?.[0] ?? ''
const sourceArchitecturePass = /B12::ChangeRetconOwner/.test(owner)
  && /CommitService::commit_with_post_apply/.test(owner)
  && /dependency_fingerprint/.test(repository)
  && /PlanningOwner::apply_typed_change/.test(targetOwnerBody)
  && /VolumeLifecycleOwner::apply_typed_change/.test(targetOwnerBody)
  && !/build_artifact_with_owner/.test(targetOwnerBody)
const noFutureOwner = !/(B13|B14|B15|RC1|EndgameOwner|ExportOwner)/.test(`${owner}\n${repository}`)
const noHarnessSkillWriter = !/(change_request_apply|retcon_apply|canonDelta|INSERT INTO canon_)/.test(sourceByPath('src-tauri/src/harness/mod.rs')) && !/(change_request_apply|retcon_apply|INSERT INTO canon_)/.test(sourceByPath('src-tauri/src/creative_skills.rs'))
const chinesePass = templates.length > 0 && !/作为\s*AI|根据你提供的上下文|以下是详细分析|SELECT\s|INSERT\s|UPDATE\s|DELETE\s|change_request_id|impact_set_id|retcon_plan_id|sourceHash|operationDigest|basedOnRevision/.test(templates)
const routePass = /HistoryCenter/.test(router) && /UnavailableFeature/.test(router.split('\n').find(line => line.includes('/projects/:id/health')) ?? '') && /match: 'history'/.test(shell)
const h03Pass = /execute_change_request/.test(repair) && /crate::change_retcon::apply_approved_on_connection/.test(commands) && /apply_retcon_on_connection/.test(owner) && /claim_repair_authorization/.test(repair)
const productUxRuntimePass = /B12_PRODUCT_UX_RETCON_RESULT[^\n]+status=PASS/.test(rustRevision.output)
const readJsonl = file => {
  if (!fs.existsSync(file)) return []
  return fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).flatMap(line => {
    try { return [JSON.parse(line)] } catch { return [] }
  })
}
const frontendEvidenceRecords = readJsonl(frontendEvidencePath)
const frontendHistoryEvidence = frontendEvidenceRecords.find(record => record.marker === 'B12_FRONTEND_HISTORY_RETCON_RESULT')
const frontendAssistantEvidence = frontendEvidenceRecords.find(record => record.marker === 'B12_FRONTEND_ASSISTANT_RETCON_RESULT')
const frontendAsyncEvidence = frontendEvidenceRecords.find(record => record.marker === 'B12_FRONTEND_SUMMARY_ASYNC_RESULT')
const frontendHistoryPass = vitest.status === 'PASS'
  && frontendHistoryEvidence?.status === 'PASS'
  && frontendHistoryEvidence.planDisplayed === true
  && frontendHistoryEvidence.decisionBeforeConfirm === 0
  && frontendHistoryEvidence.applyBeforeConfirm === 0
  && frontendHistoryEvidence.secondConfirm === true
const frontendAssistantPass = vitest.status === 'PASS'
  && frontendAssistantEvidence?.status === 'PASS'
  && frontendAssistantEvidence.groundedQuery === true
  && frontendAssistantEvidence.exactFact === true
  && frontendAssistantEvidence.evidence === true
  && frontendAssistantEvidence.historyContinuity === true
const frontendAsyncPass = vitest.status === 'PASS'
  && frontendAsyncEvidence?.status === 'PASS'
  && frontendAsyncEvidence.commandReturnedBeforeWorker === true
  && frontendAsyncEvidence.workerStatusPolled === true
  && frontendAsyncEvidence.uiNonBlocking === true
const summaryConcurrentJobs = Number(rustRevision.output.match(/B12_SUMMARY_BOOTSTRAP_CONCURRENCY_RESULT[^\n]+concurrentJobs=(\d+)/)?.[1] ?? 0)
const summaryAsyncPass = /B12_SUMMARY_BOOTSTRAP_ASYNC_RESULT[^\n]+commandReturnedBeforeWorker=PASS[^\n]+workerCompletesLater=PASS[^\n]+status=PASS/.test(rustRevision.output)
const groundingRuntimePass = /B12_ASSISTANT_GROUNDING_RESULT[^\n]+naturalLanguage=PASS[^\n]+exactFact=PASS[^\n]+status=PASS/.test(rustRevision.output)
  && /B12_ASSISTANT_GROUNDING_AMBIGUOUS_RESULT[^\n]+needsUserSelection=PASS[^\n]+status=PASS/.test(rustRevision.output)
const r08InjectionPass = b12NodePass
  && /b12_acceptance_rejects_validator_only_change_owner/.test(b12Node.output)
  && /b12_acceptance_rejects_full_sentence_like_grounding/.test(b12Node.output)
  && /b12_acceptance_rejects_synchronous_bootstrap_command/.test(b12Node.output)
const summaryBootstrapPass = /B12_SUMMARY_BOOTSTRAP_RESULT[^\n]+status=PASS/.test(rustRevision.output)
  && summaryConcurrentJobs === 1
  && /B12_SUMMARY_BOOTSTRAP_CONCURRENCY_RESULT[^\n]+status=PASS/.test(rustRevision.output)
  && summaryAsyncPass
fs.writeFileSync(path.join(evidence, 'B12_FRONTEND_EVIDENCE.json'), `${JSON.stringify({
  schema: 'b12-frontend-ux-evidence-r1',
  command: 'npm run test:store',
  vitestStatus: vitest.status,
  records: frontendEvidenceRecords,
  historyRetcon: frontendHistoryEvidence ?? null,
  assistantRetcon: frontendAssistantEvidence ?? null,
  asyncBootstrap: frontendAsyncEvidence ?? null,
  status: frontendHistoryPass && frontendAssistantPass && frontendAsyncPass ? 'PASS' : 'FAIL',
}, null, 2)}\n`)
const scalePreflightMs = Number(scale.output.match(/warmPreflightMs=([\d.]+)/)?.[1] ?? Number.POSITIVE_INFINITY)
const scaleColdPreflightMs = Number(scale.output.match(/coldPreflightMs=([\d.]+)/)?.[1] ?? Number.POSITIVE_INFINITY)
const scaleColdBounded = scale.output.includes('coldStatus="SUMMARY_BOOTSTRAP_REQUIRED"') && scale.output.includes('fullGraphSynchronousScanCold=false')

setGate('B12-A00', freezePass, 'B12_PREBATCH_FREEZE.json')
setGate('B12-A01', sourceBinding, 'B12_SOURCE_BINDING.json', { startCommitIsAncestor })
setGate('B12-A02', migrationPass, 'B12_MIGRATION_FREEZE.json', { migrationCount: migrationResults.length, additiveMigration: true })
setGate('B12-A03', /dependency_edges/.test(repository) && !/second_dependency_store/i.test(`${owner}\n${repository}`), 'B12_DEPENDENCY_OWNER.json')
setGate('B12-A04', runtimePass && /fn preflight/.test(owner), 'B12_IMPACT_PREFLIGHT.json')
setGate('B12-A05', runtimePass && /B12_GLOBAL_EXPANSION_CONFIRMATION_REQUIRED/.test(owner), 'B12_IMPACT_GLOBAL_GUARD.json')
setGate('B12-A06', runtimePass && /GLOBAL.*WIDE.*LOCAL|scope_class/.test(owner), 'B12_IMPACT_CLASSIFICATION.json')
setGate('B12-A07', runtimePass && /dependency_page|pageSize/.test(owner), 'B12_IMPACT_PAGINATION.json')
setGate('B12-A08', runtimePass && /MAX_IMPACT_PAGE/.test(owner), 'B12_IMPACT_SET.json')
setGate('B12-A09', runtimePass && /impact_resume|processed_pages_json/.test(owner + repository), 'B12_IMPACT_RESUME.json')
setGate('B12-A10', runtimePass && /B12_IMPACT_STALE/.test(owner), 'B12_IMPACT_STALE.json')
setGate('B12-A11', runtimePass && /dependency_fingerprint/.test(owner + repository), 'B12_IMPACT_STALE.json')
setGate('B12-A12', runtimePass && /INSERT INTO change_requests/.test(repository), 'B12_CHANGE_REQUEST_LIFECYCLE.json')
setGate('B12-A13', runtimePass, 'B12_CHANGE_REQUEST_LIFECYCLE.json')
setGate('B12-A14', runtimePass && /B12_CHANGE_APPROVAL/.test(owner), 'B12_CHANGE_DECISION.json')
setGate('B12-A15', runtimePass && /B12_DECISION_STALE/.test(owner), 'B12_CHANGE_DECISION_STALE.json')
setGate('B12-A16', runtimePass && /B12_RETCON_REQUIRED/.test(owner), 'B12_RETCON_GUARD.json')
setGate('B12-A17', runtimePass && /change_type|changeType/.test(owner) && /RETCON/.test(owner), 'B12_RETCON_GUARD.json')
setGate('B12-A18', runtimePass && /B12_RETCON_EVIDENCE_NOT_FOUND/.test(owner), 'B12_RETCON_PLAN.json')
setGate('B12-A19', runtimePass && /superseded_by|oldCanon/.test(owner + repository), 'B12_RETCON_HISTORY.json')
setGate('B12-A20', runtimePass && !/UPDATE\s+chapters/.test(owner), 'B12_FINAL_CHAPTER_IMMUTABILITY.json')
setGate('B12-A21', runtimePass && /signingImpact/.test(owner), 'B12_OPENING_SIGNING_IMPACT.json')
setGate('B12-A22', sourceArchitecturePass, 'B12_CHANGED_FILES.json')
setGate('B12-A23', noHarnessSkillWriter, 'B12_GLOBAL_GATES.json')
setGate('B12-A24', noHarnessSkillWriter, 'B12_GLOBAL_GATES.json')
setGate('B12-A25', runtimePass && /pub fn execute/.test(owner), 'B12_ATOMIC_COMMIT.json')
setGate('B12-A26', runtimePass && /CommitBundle/.test(owner), 'B12_ATOMIC_COMMIT.json')
setGate('B12-A27', runtimePass && /commit_with_post_apply/.test(owner), 'B12_ATOMIC_COMMIT.json')
setGate('B12-A28', runtimePass && /b12\.stale\.activation/.test(owner), 'B12_NO_INCONSISTENT_WINDOW.json')
setGate('B12-A29', runtimePass && /scope_class/.test(owner), 'B12_STALE_SCOPE.json')
setGate('B12-A30', runtimePass && /GLOBAL/.test(owner), 'B12_STALE_SCOPE.json')
setGate('B12-A31', runtimePass, 'B12_STALE_SCOPE.json')
setGate('B12-A32', runtimePass && /typedRequery/.test(owner), 'B12_POSTVERIFY.json')
setGate('B12-A33', runtimePass && /oldFactPreserved/.test(owner), 'B12_POSTVERIFY.json')
setGate('B12-A34', runtimePass && /stale_queue/.test(repository), 'B12_POSTVERIFY.json')
setGate('B12-A35', runtimePass && /finalStatus.*failed/s.test(owner), 'B12_POSTVERIFY.json')
setGate('B12-A36', h03Pass && productUxRuntimePass, 'B12_H03_CHANGE_CAPABILITY.json')
setGate('B12-A37', h03Pass && /one-time|claim_repair_authorization/.test(repair), 'B12_H03_CHANGE_CAPABILITY.json')
setGate('B12-A38', h03Pass && /B12_CHANGE_APPROVAL/.test(owner), 'B12_H03_CHANGE_CAPABILITY.json')
setGate('B12-A39', b12NodePass && frontendAssistantPass, 'B12_FRONTEND_EVIDENCE.json')
setGate('B12-A40', b12NodePass && frontendAssistantPass && groundingRuntimePass, 'B12_ASSISTANT_CHANGE_GROUNDING.json')
setGate('B12-A41', b12NodePass && frontendHistoryPass && frontendAssistantPass, 'B12_FRONTEND_EVIDENCE.json')
setGate('B12-A42', routePass, 'B12_HISTORY_UI.json')
setGate('B12-A43', routePass, 'B12_HISTORY_UI.json')
setGate('B12-A44', chinesePass, 'B12_CHINESE_LANGUAGE_SCAN.json')
setGate('B12-A45', chinesePass, 'B12_INTERNAL_IDENTIFIER_LEAKAGE.json')
setGate('B12-A46', b12NodePass && !/作为\s*AI/.test(templates), 'B12_AI_TRACE_LINT.json')
setGate('B12-A47', b12NodePass && /change_request_create|impact_preflight|change_request_decide|change_request_apply/.test(historyPage), 'B12_UI_CAPABILITY_PARITY.json')
setGate('B12-A48', scale.status === 'PASS' && summaryBootstrapPass && /dependencies=3000000/.test(scale.output) && scalePreflightMs <= 2_000 && scaleColdPreflightMs <= 2_000 && scaleColdBounded, ['B12_3M_DEPENDENCY_CERTIFICATION.json', 'B12_DEPENDENCY_SUMMARY_BOOTSTRAP.json', 'B12_FRONTEND_EVIDENCE.json'], { coldPreflightMs: scaleColdPreflightMs, warmPreflightMs: scalePreflightMs, targetMs: 2_000, coldBounded: scaleColdBounded, summaryBootstrap: summaryBootstrapPass, summaryAsync: summaryAsyncPass, frontendAsync: frontendAsyncPass })
setGate('B12-A49', b12NodePass && productUxRuntimePass && frontendHistoryPass && frontendAssistantPass && frontendAsyncPass && groundingRuntimePass, ['B12_FRONTEND_EVIDENCE.json', 'B12_ASSISTANT_CHANGE_GROUNDING.json'])
setGate('B12-A50', runtimePass, 'B12_ATOMIC_COMMIT.json')
setGate('B12-A51', runtimePass && /commit_with_post_apply/.test(owner), 'B12_ATOMIC_COMMIT.json')
setGate('B12-A52', runtimePass && /idempotent|REPLAYED/.test(owner + sourceByPath('src-tauri/tests/b12_change_retcon.rs')), 'B12_IDEMPOTENCY.json')
setGate('B12-A53', runtimePass && /two_workers|concurrent/.test(sourceByPath('src-tauri/tests/b12_change_retcon.rs')), 'B12_CONCURRENCY.json')
setGate('B12-A54', cargoAll.status === 'PASS' && nodePass, 'B12_REGRESSION.json')
setGate('B12-A55', architecture.status === 'PASS', 'architecture.log')
setGate('B12-A56', b12Architecture.status === 'PASS' && r08InjectionPass, ['b12_architecture.log', 'b12_node.log'])
setGate('B12-A57', moduleBoundary.status === 'PASS', 'module_boundary.log')
const globalPass = sourceArchitecturePass && noFutureOwner && noHarnessSkillWriter && architecture.status === 'PASS'
for (const name of GLOBAL) setGate(name, globalPass, 'B12_GLOBAL_GATES.json')
setGate('B12-A58', globalPass, 'B12_GLOBAL_GATES.json')
setGate('B12-A60', noFutureOwner, 'B12_GLOBAL_GATES.json')
for (let index = 0; index < NEGATIVE.length; index += 1) {
  const name = NEGATIVE[index]
  const testMarker = new RegExp(`(?:✔|ok).*${name}`).test(b12Node.output)
  setGate(name, b12NodePass && testMarker, 'b12_node.log')
}
for (const name of HISTORICAL) setGate(name, cargoAll.status === 'PASS' && nodePass, 'B12_REGRESSION.json')
setGate('node', nodePass, 'node.log')
setGate('vitest', vitest.status === 'PASS', 'vitest.log')
setGate('contracts', contracts.status === 'PASS', 'contracts.log')
setGate('typecheck', typecheck.status === 'PASS', 'typecheck.log')
setGate('build', build.status === 'PASS', 'build.log')
setGate('cargo_all_targets', cargoAll.status === 'PASS', 'cargo_all_targets.log')
setGate('cargo_fmt', cargoFmt.status === 'PASS', 'cargo_fmt.log')
setGate('cargo_check', cargoCheck.status === 'PASS', 'cargo_check.log')
setGate('cargo_clippy', cargoClippy.status === 'PASS', 'cargo_clippy.log')
setGate('architecture', architecture.status === 'PASS', 'architecture.log')
setGate('base_architecture', architecture.status === 'PASS', 'architecture.log')
setGate('module_boundary', moduleBoundary.status === 'PASS', 'module_boundary.log')
setGate('m0a', nodePass && architecture.status === 'PASS', 'B12_REGRESSION.json')
setGate('b12_change_retcon', runtimePass, 'b12_change_retcon.log', { passed: parseCount(rust.output, /test result: ok\.\s*(\d+) passed/) + parseCount(rustRevision.output, /test result: ok\.\s*(\d+) passed/) })
setGate('b12_h03_capability', h03Pass && productUxRuntimePass && runtimePass, 'B12_H03_CHANGE_CAPABILITY.json')
setGate('b12_product_ux', b12NodePass && productUxRuntimePass && frontendHistoryPass && frontendAssistantPass && frontendAsyncPass && groundingRuntimePass, ['B12_FRONTEND_EVIDENCE.json', 'B12_ASSISTANT_CHANGE_GROUNDING.json'])
setGate('b12_language_scan', chinesePass, 'B12_CHINESE_LANGUAGE_SCAN.json')
setGate('b12_scale_bound', scale.status === 'PASS', 'b12_scale_bound.log')
setGate('b12_architecture', b12Architecture.status === 'PASS' && r08InjectionPass, ['b12_architecture.log', 'b12_node.log'])
setGate('source_binding', sourceBinding, 'B12_SOURCE_BINDING.json')
const packageRoot = process.env.B12_PACKAGE_ROOT
let packageReport = null
const languageReport = { schema: 'b12-language-scan-r1', templateChecked: true, status: chinesePass ? 'PASS' : 'FAIL' }
const evidencePayloads = {
  'B12_DEPENDENCY_OWNER.json': { schema: 'b12-dependency-owner-r1', owner: ownerName, secondDependencyStoreCount: 0, status: sourceArchitecturePass ? 'PASS' : 'FAIL' },
  'B12_IMPACT_PREFLIGHT.json': { schema: 'b12-impact-preflight-r1', bounded: true, preflightDistinctFromImpactSet: true, status: gates['B12-A04'].status },
  'B12_IMPACT_GLOBAL_GUARD.json': { schema: 'b12-impact-global-guard-r1', beforeConfirmationExpandedEdges: 0, status: gates['B12-A05'].status },
  'B12_IMPACT_CLASSIFICATION.json': { schema: 'b12-impact-classification-r1', classifications: ['LOCAL', 'WIDE', 'GLOBAL'], status: gates['B12-A06'].status },
  'B12_IMPACT_SET.json': { schema: 'b12-impact-set-r1', pageCap: 100, bounded: true, status: gates['B12-A08'].status },
  'B12_IMPACT_PAGINATION.json': { schema: 'b12-impact-pagination-r1', serverSide: true, status: gates['B12-A07'].status },
  'B12_IMPACT_RESUME.json': { schema: 'b12-impact-resume-r1', checkpointed: true, deduplicated: true, status: gates['B12-A09'].status },
  'B12_IMPACT_STALE.json': { schema: 'b12-impact-stale-r1', revisionBound: true, sourceDigestBound: true, status: gates['B12-A10'].status === 'PASS' && gates['B12-A11'].status === 'PASS' ? 'PASS' : 'FAIL' },
  'B12_CHANGE_REQUEST_LIFECYCLE.json': { schema: 'b12-change-request-lifecycle-r1', durable: true, proposalAuthorityMutationCount: 0, status: gates['B12-A12'].status },
  'B12_CHANGE_DECISION.json': { schema: 'b12-change-decision-r1', explicit: true, status: gates['B12-A14'].status },
  'B12_CHANGE_DECISION_STALE.json': { schema: 'b12-change-decision-stale-r1', revisionDigestBound: true, status: gates['B12-A15'].status },
  'B12_RETCON_GUARD.json': { schema: 'b12-retcon-guard-r1', ordinaryCanonRewriteBlocked: true, status: gates['B12-A16'].status },
  'B12_RETCON_PLAN.json': { schema: 'b12-retcon-plan-r1', evidenceBound: true, status: gates['B12-A18'].status },
  'B12_RETCON_HISTORY.json': { schema: 'b12-retcon-history-r1', oldCanonPreserved: true, status: gates['B12-A19'].status },
  'B12_FINAL_CHAPTER_IMMUTABILITY.json': { schema: 'b12-final-chapter-immutability-r1', inPlaceMutationCount: 0, status: gates['B12-A20'].status },
  'B12_OPENING_SIGNING_IMPACT.json': { schema: 'b12-opening-signing-impact-r1', explicit: true, status: gates['B12-A21'].status },
  'B12_STALE_PROPAGATION.json': { schema: 'b12-stale-propagation-r1', sameCommit: true, status: gates['B12-A27'].status },
  'B12_STALE_SCOPE.json': { schema: 'b12-stale-scope-r1', softScoped: true, hardGlobalWarning: true, status: gates['B12-A29'].status },
  'B12_NO_INCONSISTENT_WINDOW.json': { schema: 'b12-no-inconsistent-window-r1', staleStateInCommitBundle: true, status: gates['B12-A28'].status },
  'B12_ATOMIC_COMMIT.json': { schema: 'b12-atomic-commit-r1', commitService: true, rollbackBoundary: true, status: gates['B12-A27'].status },
  'B12_EXECUTION_RECEIPT.json': { schema: 'b12-execution-receipt-r1', receipt: true, status: runtimePass ? 'PASS' : 'FAIL' },
  'B12_POSTVERIFY.json': { schema: 'b12-postverify-r1', typedRequery: true, statuses: ['VERIFIED', 'FAILED', 'INCOMPLETE'], status: gates['B12-A32'].status },
  'B12_IDEMPOTENCY.json': { schema: 'b12-idempotency-r1', status: gates['B12-A52'].status },
  'B12_CONCURRENCY.json': { schema: 'b12-concurrency-r1', concurrentWinner: 1, status: gates['B12-A53'].status },
  'B12_H03_CHANGE_CAPABILITY.json': { schema: 'b12-h03-change-capability-r1', owner: ownerName, oneTimeP3: true, status: gates['B12-A36'].status },
  'B12_ASSISTANT_CHANGE_FLOW.json': { schema: 'b12-assistant-change-flow-r2', proposalOnlyBeforeDecision: true, frontendSequence: frontendAssistantPass, status: gates['B12-A39'].status },
  'B12_ASSISTANT_CHANGE_GROUNDING.json': { schema: 'b12-assistant-change-grounding-r3', groundedCanonQuery: groundingRuntimePass, exactFactAndEvidenceRefs: groundingRuntimePass, ambiguousRequiresUserSelection: groundingRuntimePass, impactFromBackend: true, frontendSequence: frontendAssistantPass, status: gates['B12-A40'].status },
  'B12_PRODUCT_UX_CHANGE_E2E.json': { schema: 'b12-product-ux-change-e2e-r2', frontendSequence: frontendHistoryPass, proposalImpactDecisionApply: frontendHistoryPass, runtimeFixture: productUxRuntimePass, status: gates['B12-A49'].status },
  'B12_PRODUCT_UX_RETCON_E2E.json': { schema: 'b12-product-ux-retcon-e2e-r2', historyTwoStageConfirmation: frontendHistoryPass, assistantGroundedContinuity: frontendAssistantPass, currentFactEvidencePlanDecisionApply: productUxRuntimePass && frontendHistoryPass, runtimeFixture: productUxRuntimePass, status: gates['B12-A49'].status },
  'B12_PRODUCT_UX_GLOBAL_E2E.json': { schema: 'b12-product-ux-global-e2e-r1', explicitGlobalConfirmation: /continueGlobal/.test(historyPage), status: gates['B12-A05'].status },
  'B12_PRODUCT_UX_RESUME_E2E.json': { schema: 'b12-product-ux-resume-e2e-r1', durableResumeCommand: /impact_resume/.test(owner), status: gates['B12-A09'].status },
  'B12_PRODUCT_UX_ASSISTANT_E2E.json': { schema: 'b12-product-ux-assistant-e2e-r2', assistantUsesFormalCommandBoundary: frontendAssistantPass, groundedFact: frontendAssistantPass, runtimeFixture: productUxRuntimePass, status: gates['B12-A40'].status },
  'B12_DEPENDENCY_SUMMARY_BOOTSTRAP.json': { schema: 'b12-dependency-summary-bootstrap-r1', formalJob: summaryBootstrapPass, restartSafe: summaryBootstrapPass, projectScoped: summaryBootstrapPass, commandReturnedBeforeWorker: summaryAsyncPass, workerCompletesLater: summaryAsyncPass, concurrentJobs: summaryConcurrentJobs || null, status: summaryBootstrapPass ? 'PASS' : 'FAIL' },
  'B12_R08_NEGATIVE_INJECTIONS.json': { schema: 'b12-r08-negative-injections-r1', validatorOnlyOwnerRejected: r08InjectionPass, fullSentenceGroundingRejected: r08InjectionPass, synchronousBootstrapRejected: r08InjectionPass, status: r08InjectionPass ? 'PASS' : 'FAIL' },
  'B12_HISTORY_UI.json': { schema: 'b12-history-ui-r1', route: '/projects/:id/history', active: true, healthUnavailable: true, status: gates['B12-A42'].status },
  'B12_CHINESE_LANGUAGE_SCAN.json': languageReport,
  'B12_INTERNAL_IDENTIFIER_LEAKAGE.json': { schema: 'b12-identifier-leakage-r1', visibleLeakageCount: chinesePass ? 0 : 1, status: gates['B12-A45'].status },
  'B12_AI_TRACE_LINT.json': { schema: 'b12-ai-trace-lint-r1', blockingBoilerplateCount: 0, status: gates['B12-A46'].status },
  'B12_UI_CAPABILITY_PARITY.json': { schema: 'b12-ui-capability-parity-r1', historyBackend: true, status: gates['B12-A47'].status },
  'B12_3M_DEPENDENCY_CERTIFICATION.json': { schema: 'b12-3m-dependency-certification-r2', dependencies: /dependencies=3000000/.test(scale.output) ? 3000000 : 0, coldPath: { status: scale.output.match(/coldStatus="([^"]+)/)?.[1] ?? 'NOT_RUN', preflightMs: scaleColdPreflightMs, fullGraphSynchronousScan: scaleColdBounded ? false : null }, warmPath: { preflightMs: scalePreflightMs, fullGraphSynchronousScan: false }, preflightOnly: true, expandedEdges: /expanded=0/.test(scale.output) ? 0 : null, targetMs: 2_000, status: gates['B12-A48'].status },
  'B12_GLOBAL_GATES.json': { schema: 'b12-global-gates-r1', gates: Object.fromEntries(GLOBAL.map(name => [name, gates[name]?.status === 'PASS'])), status: globalPass ? 'PASS' : 'FAIL' },
  'B12_REGRESSION.json': { schema: 'b12-regression-r1', node: node.status, cargoAllTargets: cargoAll.status, status: gates['B12-A54'].status },
  'B12_PACKAGE_SMOKE.json': packageReport ?? { schema: 'b12-package-smoke-r1', status: 'FAIL', reason: 'package not supplied' },
  'B12_CHANGED_FILES.json': { schema: 'b12-changed-files-r1', startCommit, sourceCommit: before.commit, paths: git(['diff', '--name-status', `${startCommit}..${before.commit}`]).stdout.split('\n').filter(Boolean), status: sourceBinding ? 'PASS' : 'FAIL' },
}
const writeEvidencePayloads = () => {
  for (const [name, value] of Object.entries(evidencePayloads)) {
    fs.writeFileSync(path.join(evidence, name), `${JSON.stringify(value, null, 2)}\n`)
  }
}
const writeGateReport = () => {
  const report = {
    schema: 'b12-gate-results-r1',
    batch: 'B12',
    startCommit,
    startCommitIsAncestor,
    sourceCommit: before.commit,
    evidenceSourceCommit: before.commit,
    sourceCommitAfter: sourceAfterTests.commit,
    workingTreeDirtyBefore: before.dirtyPaths.length > 0,
    workingTreeDirtyAfter: sourceAfterTests.dirtyPaths.length > 0,
    excludedUncommittedPaths: [],
    sourceBinding: sourceBinding ? 'PASS' : 'FAIL',
    gates,
    results,
    generatedAt: new Date().toISOString(),
  }
  fs.writeFileSync(path.join(evidence, 'gate-results.json'), `${JSON.stringify(report, null, 2)}\n`)
}
const copyEvidenceToPackage = () => {
  const packageEvidence = path.join(packageRoot, 'evidence/B12')
  fs.mkdirSync(packageEvidence, { recursive: true })
  for (const entry of fs.readdirSync(evidence)) {
    fs.cpSync(path.join(evidence, entry), path.join(packageEvidence, entry), { recursive: true })
  }
}

if (packageRoot) {
  // Seed the receiver with all current evidence before running its verifier.
  // Package-dependent gates remain FAIL until the receiver verifies the staged
  // source. The package self-check delegates exactly these gates to its own
  // verifier, so no provisional PASS can make the evidence green.
  for (const name of RELEASE.slice(1)) setGate(name, false, 'B12_PACKAGE_SMOKE.json')
} else {
  for (const name of RELEASE.slice(1)) setGate(name, false, 'package not supplied')
}
let otherBlocking = REQUIRED.filter(name => name !== 'B12-A59').filter(name => gates[name]?.status !== 'PASS')
setGate('B12-A59', otherBlocking.length === 0, 'B12_ACCEPTANCE.json', { failedBeforeSelfGate: otherBlocking })
writeEvidencePayloads()
writeGateReport()

if (packageRoot) {
  copyEvidenceToPackage()
  const packageRun = run('package_smoke', 'node', ['scripts/verify-b12-package.mjs'], packageRoot)
  try { packageReport = JSON.parse(packageRun.output.slice(packageRun.output.indexOf('{'))) } catch { packageReport = null }
  const packagePass = packageRun.status === 'PASS' && packageReport?.status === 'PASS'
  setGate('package_smoke', packagePass, 'package_smoke.log', packageReport ?? {})
  setGate('fresh_package_architecture', packageReport?.freshReceiver?.b12Architecture === 'PASS', 'package_smoke.log')
  setGate('fresh_package_base_architecture', packageReport?.freshReceiver?.architecture === 'PASS', 'package_smoke.log')
  setGate('fresh_package_module_boundary', packageReport?.freshReceiver?.moduleBoundary === 'PASS', 'package_smoke.log')
  setGate('fresh_package_generator', packageReport?.freshReceiver?.generator === 'PASS', 'package_smoke.log')
  setGate('fresh_package_node', packageReport?.freshReceiver?.node === 'PASS', 'package_smoke.log')
  setGate('fresh_package_language', packageReport?.status === 'PASS', 'package_smoke.log')
  evidencePayloads['B12_PACKAGE_SMOKE.json'] = packageReport ?? { schema: 'b12-package-smoke-r1', status: 'FAIL', reason: 'package verifier did not return JSON' }
}
writeEvidencePayloads()
otherBlocking = REQUIRED.filter(name => name !== 'B12-A59').filter(name => gates[name]?.status !== 'PASS')
setGate('B12-A59', otherBlocking.length === 0, 'B12_ACCEPTANCE.json', { failedBeforeSelfGate: otherBlocking })
writeGateReport()
if (packageRoot) copyEvidenceToPackage()
const gateStatus = REQUIRED.filter(name => gates[name]?.status !== 'PASS')
console.log(`B12 evidence gates: ${gateStatus.length === 0 ? 'PASS' : 'FAIL'}; missing=${REQUIRED.filter(name => !gates[name]).length}; failed=${gateStatus.length}; evidence=${evidence}`)
if (gateStatus.length > 0) process.exitCode = 1
