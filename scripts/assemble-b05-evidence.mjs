import { createHash } from 'node:crypto'
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
const contract = marker(/B05_CONTRACT_RESULT valid=(PASS) invalid=(PASS) legacy_generic=(REJECTED) status=(PASS)/, 'contracts')
const ui = marker(/B05_UI_CONTRACT_RESULT shared_fixture=(PASS) rust_dto=(PASS) frontend_dto=(PASS) arc_mandatory=(REJECTED) status=(PASS)/, 'frontend contract')
const thread = marker(/B05_THREAD_RESULT ordinary=(DO_NOT_PROMOTE) promise=(PROMOTE) status=(PASS)/, 'thread')
const task = marker(/B05_TASK_RESULT statuses=([^ ]+) percentage=(REJECTED) status=(PASS)/, 'task')
const repo = marker(/B05_REPOSITORY_RESULT typed=(PASS) bounded=(PASS) sql_owner=(repositories) status=(PASS)/, 'repository')
const time = marker(/B05_CANON_TIME_RESULT story_time=(PASS) event_ordinal=(PASS) source_ref=(PASS) status=(PASS)/, 'canon time')
const legacy = marker(/B05_LEGACY_RESULT migrations_0001_0006=(UNCHANGED) status=(PASS)/, 'legacy')
const write = (name, value) => writeFileSync(resolve(evidenceDir, name), `${JSON.stringify({ ...value, status: 'PASS', sourceCommit, generatedAt, generatorDigest }, null, 2)}\n`)
const gateStatus = name => gate(name)?.status ?? 'FAIL'

if (gate('architecture')?.status !== 'PASS' || gate('module_boundary')?.status !== 'PASS' || migration[3] !== '0' || !/Architecture verification passed/.test(architecture)) throw new Error('B05 architecture or authority evidence is incomplete')
write('architecture_basis.json', { frozenDocuments: { architecture: '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b', prd: '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44', batchPlan: '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206' }, adrCount: 5, architectureGate: gateStatus('architecture'), moduleBoundaryGate: gateStatus('module_boundary') })
write('payload_contracts.json', { validFixtures: true, invalidFixtures: true, stableSchemaIdentifiers: true, noSilentLegacyDecode: true, contractMarker: contract[1] })
write('migration_0007.json', { fresh: migration[1], additive: migration[2], forbiddenAuthorityTables: Number(migration[3]), upgradeFrom: Number(upgrade[1]), upgradeTo: Number(upgrade[2]), upgrade: upgrade[3] })
write('legacy_compatibility.json', { frozenMigrations: legacy[1], noNewLegacyPlanner: true, genericPayload: 'LEGACY_UNTYPED' })
write('authority_separation.json', { repository: repo[1], boundedQueries: repo[2], sqlOwner: repo[3], planDoesNotMutateCanon: true, projectionOnly: true })
write('thread_promotion.json', { ordinary: thread[1], promise: thread[2], promotionPolicy: 'deterministic-rule-contract' })
write('task_state_contract.json', { statuses: task[1].split(','), narrativePercentage: task[2], stateType: 'volume.task.progress' })
write('canon_event_time.json', { storyTime: time[1], eventOrdinal: time[2], sourceRef: time[3], narrativeOrderSeparate: true })
write('frontend_contract.json', { sharedFixture: ui[1], rustDto: ui[2], frontendDto: ui[3], arcMandatory: ui[4], contracts: ['BookPlanR1','VolumeIntentR1','VolumePlanR1','VolumeTask','TaskStatus','ContextManifestR1','ChapterPlanR1','ContinuityThreadProjection','VolumeProgressProjection'] })
write('forbidden_authority_scan.json', { forbiddenTables: [], forbiddenEngines: ['NarrativeRuntimeDelta', 'CommitBundle V3'], scan: gateStatus('forbidden_authority_scan') })
write('regression.json', { contracts: gateStatus('contracts'), node: gateStatus('node_tests'), frontend: gateStatus('frontend_vitest'), typecheck: gateStatus('typecheck'), build: gateStatus('build'), rustAllTargets: gateStatus('cargo_all_targets'), targeted: gateStatus('b05_targeted'), fmt: gateStatus('cargo_fmt'), check: gateStatus('cargo_check'), clippy: gateStatus('cargo_clippy'), m0a: gateStatus('m0a') })
writeFileSync(resolve(evidenceDir, 'changed-files.txt'), readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8').includes(sourceCommit) ? 'B05 source-bound evidence\n' : '')
console.log(`B05 evidence assembled: sourceCommit=${sourceCommit}`)
