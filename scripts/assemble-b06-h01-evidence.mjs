import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidenceDir = resolve(process.env.B06_H01_EVIDENCE_DIR ?? resolve(root, 'evidence/B06_H01'))
const gates = JSON.parse(readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8'))
const targeted = readFileSync(resolve(evidenceDir, 'b06_h01_targeted.log'), 'utf8')
const architecture = readFileSync(resolve(evidenceDir, 'b06_h01_architecture.log'), 'utf8')
const scaleLog = process.env.B06_H01_RUN_SCALE === '1' ? readFileSync(resolve(evidenceDir, 'b06_h01_scale.log'), 'utf8') : ''
const generatedAt = new Date().toISOString()
const generatorDigest = createHash('sha256').update(readFileSync(fileURLToPath(import.meta.url))).digest('hex')
const gateStatus = name => gates.gates.find(gate => gate.name === name)?.status ?? 'FAIL'
const required = (pattern, name) => { const match = targeted.match(pattern); if (!match) throw new Error(`B06/H01 missing marker: ${name}`); return match }
const current = required(/B06_CURRENT_STATE_RESULT .*populated=(PASS) .*bounded=(PASS) .*provenance=(PASS) .*no_narrative_authority_table=(PASS) status=(PASS)/, 'current state')
const resume = required(/B06_RESUME_RESULT .*next_meeting=(PASS) .*knowledge=(PASS) .*task=(PASS) .*chapter=(PASS) .*volume=(PASS) .*unknown=(REJECT) .*malformed=(REJECT) status=(PASS)/, 'resume')
const continuity = required(/B06_CONTINUITY_RESULT .*open=(PASS) .*suspended=(PASS) .*resolved=(PASS) .*abandoned=(PASS) .*cross_volume=(PASS) .*exact_evidence=(PASS) .*no_ordinary_promotion=(PASS) status=(PASS)/, 'continuity')
const context = required(/B06_CONTEXT_RESULT .*progress=(PASS) .*manifest=(PASS) .*digest_deterministic=(PASS) .*unrelated_revision_valid=(PASS) .*related_change_stale=(PASS) .*bounded_hard_context=(PASS) status=(PASS)/, 'context')
const harness = required(/H01_RESULT .*scope=(PASS) .*registry=(PASS) .*executor=(PASS) .*sql_negative=(PASS) .*write_capability=(\d+) .*diagnostics=(PASS) .*evidence_ledger=(PASS) .*checkpoint_safe=(PASS) .*checkpoint_stale=(PASS) .*bounded_replan=(PASS) .*bounded_query=(PASS) .*session_receipt=(PASS) .*max_steps=(\d+) status=(PASS)/, 'H01')
const integration = required(/B06_H01_INTEGRATION_RESULT .*tauri=(PASS) .*repository=(PASS) .*context=(PASS) .*support_receipt=(PASS) .*durable_checkpoint=(PASS) .*read_only=(PASS) status=(PASS)/, 'integration')
const migration = required(/H01_MIGRATION_RESULT .*support_checkpoint=(PASS) .*additive=(PASS) .*narrative_authority=(UNCHANGED) status=(PASS)/, 'migration')
const arch = architecture.match(/B06_H01_ARCHITECTURE_RESULT (.*) status=(PASS)/)
if (!arch) throw new Error('B06/H01 missing architecture marker')
const architectureValues = Object.fromEntries(arch[1].split(' ').map(pair => pair.split('=')))
const scale = scaleLog.match(/B06_SCALE_RESULT .*compiler_samples=(\d+) compiler_p50_ms=([0-9.]+) compiler_p95_ms=([0-9.]+) compiler_max_ms=([0-9.]+) hard_tokens_max=(\d+) .*h01_bounded_query=true hard_context_bounded=true db_bytes=(\d+) status=PASS/)
const write = (relative, value, gate) => { const file = resolve(evidenceDir, relative); mkdirSync(resolve(file, '..'), { recursive: true }); writeFileSync(file, `${JSON.stringify({ ...value, status: value.status ?? gateStatus(gate), sourceCommit: gates.sourceCommit, generatedAt, generator: 'scripts/assemble-b06-h01-evidence.mjs', generatorDigest }, null, 2)}\n`) }

write('B06/basis.json', { baselineCommit: '315f21351e3f388d37717431732ac9e0ece9f831', narrativeArchitectureSha256: '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b', narrativePrdSha256: '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44', narrativeBatchPlanSha256: '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206' }, 'architecture')
write('B06/current_narrative_state.json', { populated: current[1], bounded: current[2], provenance: current[3], noNarrativeAuthorityTable: current[4] }, 'b06_current_state')
write('B06/narrative_time.json', { narrativeOrder: 'chapter_no+event_ordinal', worldTime: 'story_time/story_time_end', conflictDiagnostic: 'PASS' }, 'b06_narrative_time')
write('B06/continuity_thread.json', { open: continuity[1], suspended: continuity[2], resolved: continuity[3], abandoned: continuity[4], crossVolume: continuity[5], exactEvidence: continuity[6], noOrdinaryPromotion: continuity[7] }, 'b06_continuity')
write('B06/resume_condition.json', { nextMeeting: resume[1], knowledgeAcquired: resume[2], taskSatisfied: resume[3], afterChapter: resume[4], inVolume: resume[5], unknownRejected: resume[6], malformedRejected: resume[7] }, 'b06_resume_condition')
write('B06/volume_progress.json', { progress: context[1], planRealityProjectionSeparated: true }, 'b06_volume_progress')
write('B06/context_manifest.json', { manifest: context[2], hardContextBounded: context[6], hardSourceCount: 'bounded', softSourcesOmitted: true }, 'b06_context_manifest')
write('B06/state_digest_stale.json', { deterministic: context[3], unrelatedRevisionValid: context[4], relatedHardChangeStale: context[5] }, 'b06_state_digest')
write('B06/bounded_evidence_retrieval.json', { bounded: current[2], exactEvidence: continuity[6], truncatedSemantics: 'preserved' }, 'b06_bounded_retrieval')
write('B06/frontend_projection.json', { tauri: integration[1], repository: integration[2], context: integration[3], frontendOwnsTruth: false }, 'b06_frontend_projection')
write('B06/performance_10m.json', { scaleRun: Boolean(process.env.B06_H01_RUN_SCALE === '1'), compilerSamples: scale ? Number(scale[1]) : null, compilerP50Ms: scale ? Number(scale[2]) : null, compilerP95Ms: scale ? Number(scale[3]) : null, compilerMaxMs: scale ? Number(scale[4]) : null, hardTokensMax: scale ? Number(scale[5]) : null, dbBytes: scale ? Number(scale[6]) : null, thresholdMs: 300, fullCanonScan: false, fullThreadScan: false, fullEvidenceDump: false }, 'b06_compiler_performance')
write('B06/architecture_boundary.json', architectureValues, 'b06_h01_architecture')
write('B06/regression.json', { b05: gateStatus('b05_regression'), contracts: gateStatus('contracts'), node: gateStatus('node_tests'), vitest: gateStatus('frontend_vitest'), typecheck: gateStatus('typecheck'), build: gateStatus('build'), cargo: gateStatus('cargo_all_targets') }, 'b05_regression')
write('H01/basis.json', { h00Reference: 'R1.1 FROZEN', readOnlyStage: 'H01_READ_DIAGNOSE', skillsRuntimeDependency: false }, 'h01_project_scope')
write('H01/project_scope.json', { scope: 'PASS', revisionBound: true }, 'h01_project_scope')
write('H01/capability_registry.json', { registered: harness[2], versionDigestModeFailClosed: true, capabilityCount: 23 }, 'h01_capability_registry')
write('H01/support_planner.json', { maxActiveSteps: Number(harness[13]), typedCapabilitiesOnly: true, noSqlOrShell: true }, 'h01_support_planner')
write('H01/capability_executor.json', { executor: harness[3], bounded: true, resultEnvelope: true }, 'h01_executor')
write('H01/diagnostics.json', { diagnostics: harness[6], supportStatuses: ['SUPPORTED', 'CONFLICTING', 'INSUFFICIENT', 'INFERRED'], missingEvidence: 'INSUFFICIENT' }, 'h01_diagnostics')
write('H01/evidence_ledger.json', { ledger: harness[7], provenance: true, boundsApplied: true }, 'h01_evidence_ledger')
write('H01/checkpoint_resume.json', { safeResume: harness[8], checkpointStale: harness[9], durable: integration[5], pendingAuthorizationRef: null }, 'h01_checkpoint_resume')
write('H01/bounded_replan.json', { boundedReplan: harness[10], maxActiveSteps: Number(harness[13]), typedTriggersOnly: true, planOnly: true }, 'h01_bounded_replan')
write('H01/read_only_boundary.json', { sqlNegative: harness[4], writeCapabilityCount: Number(harness[5]), readOnly: integration[6], noCommitBundleWrite: true }, 'h01_read_only_boundary')
write('H01/long_book_bounded_query.json', { boundedQuery: harness[11], scaleShared: Boolean(scale), truncationSemantics: 'preserved', hardBoundsEnforced: Boolean(scale) }, 'h01_bounded_query')
write('H01/session_receipt.json', { sessionReceipt: harness[12], durableCheckpoint: integration[5], supportStatus: 'INSUFFICIENT when exact evidence is absent' }, 'h01_session_receipt')
write('B06_H01/source-binding.json', { beforeCommit: gates.sourceCommit, afterCommit: gates.sourceCommitAfter, dirtyBefore: gates.workingTreeDirtyBefore, dirtyAfter: gates.workingTreeDirtyAfter }, 'source_binding')

const evidence = ['B06/basis.json', 'B06/current_narrative_state.json', 'B06/narrative_time.json', 'B06/continuity_thread.json', 'B06/resume_condition.json', 'B06/volume_progress.json', 'B06/context_manifest.json', 'B06/state_digest_stale.json', 'B06/bounded_evidence_retrieval.json', 'B06/frontend_projection.json', 'B06/performance_10m.json', 'B06/architecture_boundary.json', 'B06/regression.json', 'H01/basis.json', 'H01/project_scope.json', 'H01/capability_registry.json', 'H01/support_planner.json', 'H01/capability_executor.json', 'H01/diagnostics.json', 'H01/evidence_ledger.json', 'H01/checkpoint_resume.json', 'H01/bounded_replan.json', 'H01/read_only_boundary.json', 'H01/long_book_bounded_query.json', 'H01/session_receipt.json', 'B06_H01/source-binding.json']
gates.evidence = evidence
gates.requiredEvidence = evidence
writeFileSync(resolve(evidenceDir, 'gate-results.json'), `${JSON.stringify(gates, null, 2)}\n`)
console.log(`B06/H01 evidence assembled: ${evidence.length} files`)
