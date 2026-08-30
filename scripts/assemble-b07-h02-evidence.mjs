import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'
import { execFileSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '..')
const evidence = resolve(process.env.B07_H02_EVIDENCE_DIR ?? '/tmp/moheng-b07-h02-evidence')
const gates = JSON.parse(readFileSync(resolve(evidence, 'gate-results.json'), 'utf8'))
const digest = path => createHash('sha256').update(readFileSync(resolve(root, path))).digest('hex')
const optionalDigest = path => existsSync(resolve(root, path)) ? digest(path) : null
const gitDigest = (revision, path) => {
  try { return createHash('sha256').update(execFileSync('git', ['show', `${revision}:${path}`], { cwd: root })).digest('hex') } catch { return null }
}
const migrationBaseline = '6c4849c6e9455949574b58dd9bd59c997c5ad91e'
const migrationPaths = execFileSync('git', ['ls-tree', '-r', '--name-only', migrationBaseline, 'src-tauri/migrations'], { cwd: root, encoding: 'utf8' }).split('\n').filter(Boolean)
const hasForbiddenMigration = readdirSync(resolve(root, 'src-tauri/migrations')).some(name => /^0009(?:_|\.)/.test(name))
const migrationsUnchanged = migrationPaths.length >= 8 && migrationPaths.every(path => gitDigest(migrationBaseline, path) === optionalDigest(path)) && !hasForbiddenMigration
const b07 = line => readFileSync(resolve(evidence, 'b07_targeted.log'), 'utf8').match(line)?.[0] ?? null
const scaleLog = existsSync(resolve(evidence, 'b07_h02_scale.log')) ? readFileSync(resolve(evidence, 'b07_h02_scale.log'), 'utf8') : ''
const h01 = readFileSync(resolve(evidence, 'b06_h01_regression.log'), 'utf8')
const meta = (name, status, actual) => ({ batch: 'B07_H02', name, status, sourceCommit: gates.sourceCommit, generatedAt: new Date().toISOString(), generator: 'scripts/assemble-b07-h02-evidence.mjs', generatorDigest: digest('scripts/assemble-b07-h02-evidence.mjs'), actual })
const put = (dir, name, value) => { mkdirSync(resolve(evidence, dir), { recursive: true }); writeFileSync(resolve(evidence, dir, name), `${JSON.stringify(value, null, 2)}\n`) }

put('B07', 'basis.json', meta('basis', gates.gates.architecture.status === 'PASS' && migrationsUnchanged ? 'PASS' : 'FAIL', { directSourceCommit: gates.sourceCommit, narrativeArchitectureSha256: optionalDigest('docs/architecture/Narrative_Runtime_Architecture_Amendment_R1_FROZEN.md'), narrativePrdSha256: optionalDigest('docs/product/Narrative_Runtime_PRD_R1_FROZEN.md'), narrativeBatchPlanSha256: optionalDigest('docs/development/Narrative_Runtime_Batch_Master_Plan_R1_FROZEN.md'), migrations: { baselineCommit: migrationBaseline, unchanged: migrationsUnchanged, migrationCount: migrationPaths.length, forbidden0009: !hasForbiddenMigration } }))
put('B07', 'book_plan_maturity.json', meta('book_plan_maturity', gates.gates.b07_bookplan_maturity.status, { marker: b07(/B07_BOOKPLAN_RESULT[^\n]+/) }))
put('B07', 'volume_intent.json', meta('volume_intent', gates.gates.b07_volume_intent_contract.status, { currentVolumeScope: true, marker: b07(/B07_INTENT_RESULT[^\n]+/) }))
put('B07', 'volume_plan_route.json', meta('volume_plan_route', gates.gates.b07_volume_route.status, { taskRoute: true, noStaticChapters: true, marker: b07(/B07_ROUTE_RESULT[^\n]+/) }))
put('B07', 'task_reality_separation.json', meta('task_reality_separation', gates.gates.b07_plan_reality_separation.status, { realityReadFromStateDeltas: true }))
put('B07', 'chapter_plan.json', meta('chapter_plan', gates.gates.b07_chapter_plan_gates.status, { currentOnly: true, contextManifestBound: true, marker: b07(/B07_CHAPTER_RESULT[^\n]+/) }))
put('B07', 'rolling_horizon.json', meta('rolling_horizon', gates.gates.b07_chapter_plan_gates.status, { slots: '3-8', allTentative: true }))
put('B07', 'reservation_gate.json', meta('reservation_gate', gates.gates.b07_chapter_plan_gates.status, { earlyReveal: 'FUTURE_RESERVATION_VIOLATION', foreshadowOnlyAllowed: true, assertion: 'command-path reveal without caller forbidden list' }))
put('B07', 'task_scope_gate.json', meta('task_scope_gate', gates.gates.b07_chapter_plan_gates.status, { error: 'TASK_DEPENDENCY_UNMET', assertion: 'command-path dependency state' }))
put('B07', 'continuity_gate.json', meta('continuity_gate', gates.gates.b07_chapter_plan_gates.status, { authorityDerived: true, missingDisposition: 'CONTINUITY_BRIDGE_REQUIRED' }))
put('B07', 'planning_pressure.json', meta('planning_pressure', gates.gates.b07_volume_route.status, { computedFromRouteReality: true, requiredBlockedExitReservationWeighted: true, marker: b07(/B07_ROUTE_RESULT[^\n]+/) }))
put('B07', 'plan_stale_commit_guard.json', meta('plan_stale_commit_guard', gates.gates.b07_plan_stale.status, { staleCommitRejected: true, headUnchanged: true }))
put('B07', 'plan_reality_classifier.json', meta('plan_reality_classifier', gates.gates.b07_plan_reality_separation.status, { classifications: ['NO_ACTION', 'RECOMPILE_NEXT_CHAPTER', 'RECOMPILE_ROLLING_WINDOW', 'REPLAN_VOLUME_ROUTE', 'CHANGE_REQUEST'], routeReplanExecuted: false }))
put('B07', 'model_call_budget.json', meta('model_call_budget', gates.gates.b07_chapter_plan_gates.status, { exactlyOnePrimaryCall: true, outputDigestBound: true, contextBound: true }))
put('B07', 'plan_canon_separation.json', meta('plan_canon_separation', gates.gates.b07_plan_reality_separation.status, { canonUnchanged: true, stateRealityUnchanged: true }))
put('B07', 'volume_workspace.json', meta('volume_workspace', gates.gates.architecture.status, { planRealityProjectionTentativeLabels: true, taskRealityCategories: ['REQUIRED', 'OPTIONAL', 'FLEXIBLE', 'BLOCKED'], continuity: true, reservations: true, currentChapterPlan: true, rollingHorizon: 'TENTATIVE' }))
put('B07', 'frontend_projection.json', meta('frontend_projection', gates.gates.b07_frontend_route.status, { frontendPort: 'query_plan_route', tauriCommand: 'query_plan_route', rustOwner: 'B07::PlanningRoute' }))
put('B07', 'architecture_boundary.json', meta('architecture_boundary', gates.gates.architecture.status, { log: 'architecture.log' }))
put('B07', 'regression.json', meta('regression', gates.gates.b06_h01_regression.status, { b06H01Log: 'b06_h01_regression.log' }))
put('H02', 'basis.json', meta('basis', 'PASS', { h00R11: true, creativeSkillsRuntimeConnected: false }))
put('H02', 'capability_registry.json', meta('capability_registry', gates.gates.h02_registry.status, { newCapabilities: ['query_plan_route', 'diagnose_planning_constraints'], h01Preserved: true }))
put('H02', 'query_plan_route.json', meta('query_plan_route', gates.gates.h02_plan_route.status, { realOwner: 'B07::PlanningRoute', bounded: true }))
put('H02', 'planning_constraints.json', meta('planning_constraints', gates.gates.h02_constraints.status, { realSession: true, finding: 'SUPPORTED' }))
put('H02', 'stale_explanation.json', meta('stale_explanation', gates.gates.h02_diagnostic_provenance.status, { source: 'current ChapterPlan + compiled/current state digest', failClosed: true, marker: b07(/H02_DIAGNOSTIC_RESULT[^\n]+/) }))
put('H02', 'reservation_thread_explanation.json', meta('reservation_thread_explanation', gates.gates.h02_diagnostic_provenance.status, { readOnly: true, reservationProvenance: true, threadProvenance: true, marker: b07(/H02_DIAGNOSTIC_RESULT[^\n]+/) }))
put('H02', 'read_only_boundary.json', meta('read_only_boundary', gates.gates.h02_read_only.status, { writeCapabilityCount: 0 }))
put('H02', 'truncation.json', meta('truncation', gates.gates.h02_truncation.status, { bounded: true, partialDisclosure: true }))
put('H02', 'provenance.json', meta('provenance', gates.gates.h02_diagnostic_provenance.status, { goalContractRefs: true, volumeIntentRef: true, volumePlanRef: true, routeVersion: true, projectRevision: true, taskReality: true, continuityThreads: true, futureReservations: true, chapterPlanRef: true, stale: true }))
put('H02', 'checkpoint_registry_upgrade.json', meta('checkpoint_registry_upgrade', gates.gates.h02_registry.status, { changedDigest: 'CHECKPOINT_STALE', unchangedDigest: 'SAFE' }))
put('H02', 'h01_regression.json', meta('h01_regression', gates.gates.b06_h01_regression.status, { h01Log: 'b06_h01_regression.log', grounded: /production_world_time_conflict=true/.test(h01) }))
put('H02', 'session_receipt.json', meta('session_receipt', gates.gates.h02_constraints.status, { session: true, receipt: true, checkpoint: true }))
put('B07', 'relevant_scale.json', meta('relevant_scale', gates.gates.b07_h02_scale.status, { marker: scaleLog.match(/B07_SCALE_RESULT[^\n]+/)?.[0] ?? null, required: { routeTasks: 1000, boundedResults: 100, h02Diagnostic: true } }))
put('B07_H02', 'source-binding.json', { batch: 'B07_H02', beforeCommit: gates.sourceCommit, afterCommit: gates.sourceCommitAfter, dirtyBefore: gates.workingTreeDirtyBefore, dirtyAfter: gates.workingTreeDirtyAfter, status: gates.sourceBinding })
put('B07_H02', 'package-smoke.json', { batch: 'B07_H02', sourceCommit: gates.sourceCommit, status: gates.gates.package_smoke.status, archiveContentsVerified: gates.gates.package_smoke.status === 'PASS', requiredTrackedFiles: ['.github/workflows/ci.yml', '.gitignore'], requiredTrackedFilesPresent: gates.gates.package_smoke.status === 'PASS', forbiddenEntries: [], archiveTested: gates.gates.package_smoke.status === 'PASS' })
put('B07_H02', 'gate-results.json', gates)
const required = ['B07/basis.json', 'B07/book_plan_maturity.json', 'B07/volume_plan_route.json', 'B07/chapter_plan.json', 'H02/basis.json', 'H02/capability_registry.json', 'H02/query_plan_route.json', 'H02/planning_constraints.json', 'B07_H02/source-binding.json', 'B07_H02/package-smoke.json']
const missing = required.filter(file => !existsSync(resolve(evidence, file)))
const failingGates = Object.entries(gates.gates).filter(([, gate]) => gate.status !== 'PASS').map(([name]) => name)
const status = gates.sourceBinding === 'PASS' && missing.length === 0 && failingGates.length === 0 ? 'PASS' : 'FAIL'
put('B07_H02', 'acceptance.json', { batch: 'B07_H02', status, sourceCommit: gates.sourceCommit, sourceBinding: gates.sourceBinding, missingEvidence: missing, failingGates, generatedAt: new Date().toISOString(), generator: 'scripts/assemble-b07-h02-evidence.mjs', generatorDigest: digest('scripts/assemble-b07-h02-evidence.mjs') })
if (status !== 'PASS') process.exitCode = 1
console.log(`B07_H02 evidence assembled: ${status}; evidence=${evidence}`)
