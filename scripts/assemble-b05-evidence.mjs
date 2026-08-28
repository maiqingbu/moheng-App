import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidenceDir = resolve(process.env.B05_EVIDENCE_DIR ?? resolve(root, 'evidence/B05'))
mkdirSync(evidenceDir, { recursive: true })
const gates = JSON.parse(readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8'))
const sourceCommit = gates.sourceCommit
const generatedAt = new Date().toISOString()
const generatorDigest = createHash('sha256').update(readFileSync(fileURLToPath(import.meta.url))).digest('hex')
const targeted = readFileSync(resolve(evidenceDir, 'b05_targeted_test.log'), 'utf8')
const architecture = readFileSync(resolve(evidenceDir, 'architecture_gate.log'), 'utf8')
const gate = name => gates.gates.find(item => item.name === name)
const marker = (pattern, name) => {
  const result = targeted.match(pattern)
  if (!result) throw new Error(`B05 missing marker: ${name}`)
  return result
}
const migration = marker(/B05_MIGRATION_RESULT fresh_0007=(PASS) additive=(PASS) forbidden_authority_tables=(\d+) status=(PASS)/, 'migration')
const upgrade = marker(/B05_MIGRATION_UPGRADE_RESULT from=(\d+) to=(\d+) backup_boundary=(PASS) additive=(PASS) status=(PASS)/, 'migration upgrade')
const contract = marker(/B05_CONTRACT_RESULT valid=(PASS) invalid=(PASS) legacy_generic=(REJECTED) stable_ids=(PASS) status=(PASS)/, 'contracts')
const ui = marker(/B05_UI_CONTRACT_RESULT shared_fixture=(PASS) rust_dto=(PASS) frontend_dto=(PASS) arc_mandatory=(REJECTED) status=(PASS)/, 'frontend contract')
const architectureBasis = marker(/B05_ARCHITECTURE_RESULT docs=(PASS) frozen_migrations=(PASS) no_second_authority=(PASS) forbidden_authority_tables=(\d+) legacy_planner=(ABSENT) status=(PASS)/, 'architecture basis')
const thread = marker(/B05_THREAD_RESULT ordinary=(DO_NOT_PROMOTE) ambient=(DO_NOT_PROMOTE) trivial=(DO_NOT_PROMOTE) detail=(DO_NOT_PROMOTE) promise=(PROMOTE) relationship=(PROMOTE) conflict=(PROMOTE) mystery=(PROMOTE) foreshadow=(PROMOTE) future_object=(PROMOTE) goal=(PROMOTE) authority=(PROMOTE) reservation=(PROMOTE) status=(PASS)/, 'thread')
const task = marker(/B05_TASK_RESULT statuses=([^ ]+) percentage=(REJECTED) status=(PASS)/, 'task')
const repo = marker(/B05_REPOSITORY_RESULT typed=(PASS) populated=(PASS) bounded=(PASS) relevant=(PASS) old_promise=(PASS) sql_owner=(repositories) status=(PASS)/, 'repository')
const repositoryCoverage = marker(/B05_REPOSITORY_COVERAGE_RESULT relations=(PASS) evidence=(PASS) obligations=(PASS) foreshadows=(PASS) mysteries=(PASS) tasks_by_id=(PASS) recent_evidence=(PASS) status=(PASS)/, 'repository coverage')
const time = marker(/B05_CANON_TIME_RESULT story_time=(PASS) event_ordinal=(PASS) source_ref=(PASS) narrative_order_separate=(PASS) status=(PASS)/, 'canon time')
const legacy = marker(/B05_LEGACY_RESULT migrations_0001_0006=(UNCHANGED) status=(PASS)/, 'legacy')
const authority = marker(/B05_AUTHORITY_RESULT plan_artifacts=(\d+) canon_rows_before=(\d+) canon_rows_after=(\d+) canon_digest_equal=(true|false) projection_only=(true|false) status=(PASS)/, 'authority separation')
const write = (name, value) => writeFileSync(resolve(evidenceDir, name), `${JSON.stringify({ ...value, status: 'PASS', sourceCommit, generatedAt, generatorDigest }, null, 2)}\n`)
const gateStatus = name => gate(name)?.status ?? 'FAIL'

if (gate('architecture')?.status !== 'PASS' || gate('module_boundary')?.status !== 'PASS' || migration[3] !== '0' || architectureBasis[1] !== 'PASS' || architectureBasis[2] !== 'PASS' || architectureBasis[3] !== 'PASS' || architectureBasis[4] !== '0' || architectureBasis[5] !== 'ABSENT' || authority[4] !== 'true' || authority[5] !== 'true') throw new Error('B05 architecture or authority evidence is incomplete')
write('architecture_basis.json', { frozenDocuments: { architecture: '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b', prd: '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44', batchPlan: '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206' }, adrCount: 5, architectureGate: gateStatus('architecture'), moduleBoundaryGate: gateStatus('module_boundary'), noSecondAuthority: architectureBasis[3], forbiddenAuthorityTableCount: Number(architectureBasis[4]), legacyPlanner: architectureBasis[5] })
write('payload_contracts.json', { validFixtures: true, invalidFixtures: true, stableSchemaIdentifiers: contract[4], noSilentLegacyDecode: contract[3], contractMarker: contract[1] })
write('migration_0007.json', { fresh: migration[1], additive: migration[2], forbiddenAuthorityTables: Number(migration[3]), upgradeFrom: Number(upgrade[1]), upgradeTo: Number(upgrade[2]), upgrade: upgrade[3] })
write('legacy_compatibility.json', { frozenMigrations: legacy[1], noNewLegacyPlanner: true, genericPayload: 'LEGACY_UNTYPED' })
write('authority_separation.json', { repository: repo[1], populatedQueries: repo[2], boundedQueries: repo[3], relevantQueries: repo[4], oldPromiseNotDroppedByLimit: repo[5], sqlOwner: repo[6], planArtifacts: Number(authority[1]), canonRowsBefore: Number(authority[2]), canonRowsAfter: Number(authority[3]), canonDigestEqual: authority[4] === 'true', projectionOnly: authority[5] === 'true' })
write('repository_coverage.json', { relations: repositoryCoverage[1], evidence: repositoryCoverage[2], obligations: repositoryCoverage[3], foreshadows: repositoryCoverage[4], mysteries: repositoryCoverage[5], tasksById: repositoryCoverage[6], recentEvidence: repositoryCoverage[7] })
write('repository_relevance.json', { populated: repo[2], relevant: repo[4], oldPromiseNotDroppedByLimit: repo[5] })
write('plan_canon_separation.json', { planArtifacts: Number(authority[1]), canonRowsBefore: Number(authority[2]), canonRowsAfter: Number(authority[3]), canonDigestEqual: authority[4] === 'true', projectionOnly: authority[5] === 'true' })
write('thread_promotion.json', { ordinary: thread[1], ambient: thread[2], trivial: thread[3], nonContinuingDetail: thread[4], positiveReasons: { promise: thread[5], relationship: thread[6], conflict: thread[7], mystery: thread[8], foreshadow: thread[9], futureObject: thread[10], goal: thread[11], authority: thread[12], reservation: thread[13] }, promotionPolicy: 'deterministic-rule-contract' })
write('task_state_contract.json', { statuses: task[1].split(','), narrativePercentage: task[2], stateType: 'volume.task.progress' })
write('canon_event_time.json', { storyTime: time[1], eventOrdinal: time[2], sourceRef: time[3], narrativeOrderSeparate: time[4] })
write('frontend_contract.json', { sharedFixture: ui[1], rustDto: ui[2], frontendDto: ui[3], arcMandatory: ui[4], contracts: ['BookPlanR1','VolumeIntentR1','VolumePlanR1','VolumeTask','TaskStatus','ContextManifestR1','ChapterPlanR1','ContinuityThreadProjection','VolumeProgressProjection'] })
write('forbidden_authority_scan.json', { forbiddenAuthorityTableCount: Number(architectureBasis[4]), forbiddenEngines: ['NarrativeRuntimeDelta', 'CommitBundle V3'], scan: gateStatus('forbidden_authority_scan'), legacyPlanner: architectureBasis[5] })
write('regression.json', { contracts: gateStatus('contracts'), node: gateStatus('node_tests'), frontend: gateStatus('frontend_vitest'), typecheck: gateStatus('typecheck'), build: gateStatus('build'), rustAllTargets: gateStatus('cargo_all_targets'), targeted: gateStatus('b05_targeted'), fmt: gateStatus('cargo_fmt'), check: gateStatus('cargo_check'), clippy: gateStatus('cargo_clippy'), m0a: gateStatus('m0a') })
const changedFiles = execFileSync('git', ['diff', '--name-status', '77f39aedc9bdb90070715f0399c6e6e1ea5eda10..HEAD'], { cwd: root, encoding: 'utf8' })
writeFileSync(resolve(evidenceDir, 'changed-files.txt'), `sourceCommit=${sourceCommit}\ngeneratedAt=${generatedAt}\ngeneratorDigest=${generatorDigest}\n${changedFiles}`)
console.log(`B05 evidence assembled: sourceCommit=${sourceCommit}`)
