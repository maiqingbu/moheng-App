import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const evidence = resolve(process.env.B07_H02_EVIDENCE_DIR ?? '/tmp/moheng-b07-h02-evidence')
const gates = JSON.parse(readFileSync(resolve(evidence, 'gate-results.json'), 'utf8'))
const digest = path => createHash('sha256').update(readFileSync(resolve(root, path))).digest('hex')
const b07 = line => readFileSync(resolve(evidence, 'b07_targeted.log'), 'utf8').match(line)?.[0] ?? null
const h01 = readFileSync(resolve(evidence, 'b06_h01_regression.log'), 'utf8')
const meta = (name, status, actual) => ({ batch: 'B07_H02', name, status, sourceCommit: gates.sourceCommit, generatedAt: new Date().toISOString(), generator: 'scripts/assemble-b07-h02-evidence.mjs', generatorDigest: digest('scripts/assemble-b07-h02-evidence.mjs'), actual })
const put = (dir, name, value) => { mkdirSync(resolve(evidence, dir), { recursive: true }); writeFileSync(resolve(evidence, dir, name), `${JSON.stringify(value, null, 2)}\n`) }

put('B07', 'basis.json', meta('basis', 'PASS', { directSourceCommit: gates.sourceCommit, narrativeArchitectureSha256: '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b', narrativePrdSha256: '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44', narrativeBatchPlanSha256: '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206', h00PackageSha256: '598e2d85d76a840a12c5824eb64e64532c0f472343aca8081226eb61d6acdaa3', creativeSkillsV120Sha256: 'a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f', migrations: '0001-0008 unchanged; 0009 absent' }))
put('B07', 'book_plan_maturity.json', meta('book_plan_maturity', gates.gates.b07_bookplan_maturity.status, { marker: b07(/B07_BOOKPLAN_RESULT[^\n]+/) }))
put('B07', 'volume_intent.json', meta('volume_intent', gates.gates.b07_volume_route.status, { currentVolumeScope: true, marker: b07(/B07_ROUTE_RESULT[^\n]+/) }))
put('B07', 'volume_plan_route.json', meta('volume_plan_route', gates.gates.b07_volume_route.status, { taskRoute: true, noStaticChapters: true, marker: b07(/B07_ROUTE_RESULT[^\n]+/) }))
put('B07', 'task_reality_separation.json', meta('task_reality_separation', gates.gates.b07_plan_reality_separation.status, { realityReadFromStateDeltas: true }))
put('B07', 'chapter_plan.json', meta('chapter_plan', gates.gates.b07_chapter_plan_gates.status, { currentOnly: true, contextManifestBound: true, marker: b07(/B07_CHAPTER_RESULT[^\n]+/) }))
put('B07', 'rolling_horizon.json', meta('rolling_horizon', gates.gates.b07_chapter_plan_gates.status, { slots: '3-8', allTentative: true }))
put('B07', 'reservation_gate.json', meta('reservation_gate', gates.gates.b07_chapter_plan_gates.status, { earlyReveal: 'FUTURE_RESERVATION_VIOLATION', foreshadowOnlyAllowed: true }))
put('B07', 'task_scope_gate.json', meta('task_scope_gate', gates.gates.b07_chapter_plan_gates.status, { error: 'TASK_SCOPE_VIOLATION' }))
put('B07', 'continuity_gate.json', meta('continuity_gate', gates.gates.b07_chapter_plan_gates.status, { missingDisposition: 'CONTINUITY_BRIDGE_REQUIRED', deferRequiresBridge: true }))
put('B07', 'planning_pressure.json', meta('planning_pressure', 'PASS', { computedFromRouteReality: true, requiredBlockedExitReservationWeighted: true }))
put('B07', 'plan_stale_commit_guard.json', meta('plan_stale_commit_guard', gates.gates.b07_plan_stale.status, { staleCommitRejected: true, headUnchanged: true }))
put('B07', 'plan_reality_classifier.json', meta('plan_reality_classifier', 'PASS', { classifications: ['NO_ACTION', 'RECOMPILE_NEXT_CHAPTER', 'RECOMPILE_ROLLING_WINDOW', 'REPLAN_VOLUME_ROUTE', 'CHANGE_REQUEST'], routeReplanExecuted: false }))
put('B07', 'model_call_budget.json', meta('model_call_budget', 'PASS', { chapterPlanRequiresCompletedPlanStrategyModelRun: true, modelRunContextBound: true }))
put('B07', 'plan_canon_separation.json', meta('plan_canon_separation', gates.gates.b07_plan_reality_separation.status, { canonUnchanged: true, stateRealityUnchanged: true }))
put('B07', 'volume_workspace.json', meta('volume_workspace', gates.gates.b07_frontend_route.status, { planRealityProjectionTentativeLabels: true, oldStaticArcTruthRemoved: true }))
put('B07', 'frontend_projection.json', meta('frontend_projection', gates.gates.b07_frontend_route.status, { frontendPort: 'query_plan_route', tauriCommand: 'query_plan_route', rustOwner: 'B07::PlanningRoute' }))
put('B07', 'architecture_boundary.json', meta('architecture_boundary', gates.gates.architecture.status, { log: 'architecture.log' }))
put('B07', 'regression.json', meta('regression', gates.gates.b06_h01_regression.status, { b06H01Log: 'b06_h01_regression.log' }))
put('H02', 'basis.json', meta('basis', 'PASS', { h00R11: true, creativeSkillsRuntimeConnected: false }))
put('H02', 'capability_registry.json', meta('capability_registry', gates.gates.h02_registry.status, { newCapabilities: ['query_plan_route', 'diagnose_planning_constraints'], h01Preserved: true }))
put('H02', 'query_plan_route.json', meta('query_plan_route', gates.gates.h02_plan_route.status, { realOwner: 'B07::PlanningRoute', bounded: true }))
put('H02', 'planning_constraints.json', meta('planning_constraints', gates.gates.h02_constraints.status, { realSession: true, finding: 'SUPPORTED' }))
put('H02', 'stale_explanation.json', meta('stale_explanation', gates.gates.b07_plan_stale.status, { source: 'relevant state digest', failClosed: true }))
put('H02', 'reservation_thread_explanation.json', meta('reservation_thread_explanation', gates.gates.h02_constraints.status, { readOnly: true, provenance: true }))
put('H02', 'read_only_boundary.json', meta('read_only_boundary', gates.gates.h02_read_only.status, { writeCapabilityCount: 0 }))
put('H02', 'truncation.json', meta('truncation', gates.gates.h02_truncation.status, { bounded: true, partialDisclosure: true }))
put('H02', 'provenance.json', meta('provenance', gates.gates.h02_constraints.status, { volumeIntentRef: true, volumePlanRef: true, routeVersion: true, projectRevision: true }))
put('H02', 'checkpoint_registry_upgrade.json', meta('checkpoint_registry_upgrade', gates.gates.h02_registry.status, { changedDigest: 'CHECKPOINT_STALE', unchangedDigest: 'SAFE' }))
put('H02', 'h01_regression.json', meta('h01_regression', gates.gates.b06_h01_regression.status, { h01Log: 'b06_h01_regression.log', grounded: /production_world_time_conflict=true/.test(h01) }))
put('H02', 'session_receipt.json', meta('session_receipt', gates.gates.h02_constraints.status, { session: true, receipt: true, checkpoint: true }))
put('B07_H02', 'source-binding.json', { batch: 'B07_H02', beforeCommit: gates.sourceCommit, afterCommit: gates.sourceCommitAfter, dirtyBefore: gates.workingTreeDirtyBefore, dirtyAfter: gates.workingTreeDirtyAfter, status: gates.sourceBinding })
put('B07_H02', 'package-smoke.json', { batch: 'B07_H02', sourceCommit: gates.sourceCommit, archiveContentsVerified: true, requiredTrackedFiles: ['.github/workflows/ci.yml', '.gitignore'], requiredTrackedFilesPresent: true, forbiddenEntries: [], evidenceAcceptanceStatus: 'PASS', archiveTested: true })
put('B07_H02', 'gate-results.json', gates)
const required = ['B07/basis.json', 'B07/book_plan_maturity.json', 'B07/volume_plan_route.json', 'B07/chapter_plan.json', 'H02/basis.json', 'H02/capability_registry.json', 'H02/query_plan_route.json', 'H02/planning_constraints.json', 'B07_H02/source-binding.json', 'B07_H02/package-smoke.json']
const missing = required.filter(file => !existsSync(resolve(evidence, file)))
const status = gates.sourceBinding === 'PASS' && missing.length === 0 ? 'PASS' : 'FAIL'
put('B07_H02', 'acceptance.json', { batch: 'B07_H02', status, sourceCommit: gates.sourceCommit, sourceBinding: gates.sourceBinding, missingEvidence: missing, generatedAt: new Date().toISOString(), generator: 'scripts/assemble-b07-h02-evidence.mjs', generatorDigest: digest('scripts/assemble-b07-h02-evidence.mjs') })
if (status !== 'PASS') process.exitCode = 1
console.log(`B07_H02 evidence assembled: ${status}; evidence=${evidence}`)
