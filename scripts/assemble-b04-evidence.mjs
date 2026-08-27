import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidenceDir = resolve(process.env.B04_EVIDENCE_DIR ?? resolve(root, 'evidence/B04'))
mkdirSync(evidenceDir, { recursive: true })
const gates = JSON.parse(readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8'))
const sourceCommit = gates.sourceCommit
const targeted = readFileSync(resolve(evidenceDir, 'b04_targeted_test.log'), 'utf8')
const scaleMaterialized = readFileSync(resolve(evidenceDir, 'b04_scale_materialization.log'), 'utf8')
const fixture = targeted.match(/B04_FIXTURE_RESULT chapters=(\d+) artifacts=(\d+) canon_facts=(\d+) dependencies=(\d+) model_runs=(\d+) target_words=(\d+)/)
const scale = targeted.match(/B04_SCALE_RESULT books=(\d+) volumes=(\d+) chapters=(\d+) target_words=(\d+) artifacts=(\d+) canon_facts=(\d+) dependencies=(\d+) model_runs=(\d+)/)
const performance = targeted.match(/B04_PERF_RESULT samples=(\d+) p50_ms=([0-9.]+) p95_ms=([0-9.]+)/)
const profile = targeted.match(/B04_PROFILE_RESULT page_size=(\d+) auto_vacuum=(\d+) mmap_size=(\d+) busy_timeout_ms=(\d+) synchronous=(\w+) wal_autocheckpoint=(\d+)/)
const wal = targeted.match(/B04_WAL_VACUUM_RESULT wal_bytes=(\d+) checkpointed_pages=(\d+) vacuum_pages=(\d+) threshold=(\w+)/)
const backup = targeted.match(/B04_BACKUP_RESULT checksum=([a-f0-9]{64}) restored=PASS tamper_rejected=PASS/)
const restore = targeted.match(/B04_RESTORE_WORKFLOW_RESULT .*maintenance=PASS .*checksum=PASS .*quick_check=PASS .*foreign_key_check=PASS .*deep_integrity=PASS .*receipt=PASS/)
const upgrade = targeted.match(/B04_FTS_UPGRADE_RESULT .*source_chapters=(\d+) source_artifacts=(\d+) pre_projection_docs=(\d+) post_projection_docs=(\d+) rebuild_triggered=(true|false) second_open_full_rebuild=(true|false) projection_version=([^ ]+) status=PASS/)
const integrityMatches = [...targeted.matchAll(/B04_INTEGRITY_RESULT failures=(\d+) status=FAIL_CLOSED/g)]
const materialized = scaleMaterialized.match(/B04_SCALE_MATERIALIZED books=(\d+) volumes=(\d+) chapters=(\d+) artifacts=(\d+) canon_facts=(\d+) dependencies=(\d+) model_runs=(\d+) target_words=(\d+) status=PASS/)
const scaleCertification = scaleMaterialized.match(/B04_SCALE_CERTIFICATION_RESULT .*integrity=PASS .*fts_incremental=PASS .*performance=PASS .*backup_restore=PASS .*restore_read_pool=PASS .*vacuum=PASS status=PASS/)
const perfOperations = [...targeted.matchAll(/B04_PERF_OPERATION operation=([a-z_]+) samples=(\d+) p50_ms=([0-9.]+) p95_ms=([0-9.]+)/g)]
const requiredOperations = { project_overview: 2000, volume_list: 300, chapter_list: 300, canon_current: 250, adjacency: 500, normal_commit: 1000, recovery_readable: 10000 }
const performancePass = Object.entries(requiredOperations).every(([operation, threshold]) => {
  const marker = perfOperations.find(item => item[1] === operation)
  return marker && Number(marker[2]) > 0 && Number(marker[4]) <= threshold
}) && Number(performance[3]) <= 1000
const readJson = name => JSON.parse(readFileSync(resolve(evidenceDir, name), 'utf8'))
let scaleIntegrity; let scalePerformance; let scaleRestore; let scaleCertificationReport; let benchmarkSamples; let benchmarkEnvironment
try {
  scaleIntegrity = readJson('long_book_scale_integrity.json')
  scalePerformance = readJson('scale_perf_p95.json')
  scaleRestore = readJson('scale_backup_restore.json')
  scaleCertificationReport = readJson('long_book_scale_certification.json')
  benchmarkSamples = readJson('scale_benchmark_samples.json')
  benchmarkEnvironment = readJson('benchmark_environment.json')
} catch {
  throw new Error('B04 scale certification evidence files are incomplete')
}
if (!fixture || !scale || !performance || !profile || !wal || !backup || !restore || !upgrade || !scaleCertification || integrityMatches.length === 0 || !materialized || !performancePass || scaleIntegrity.status !== 'PASS' || scalePerformance.status !== 'PASS' || scaleRestore.status !== 'PASS' || scaleCertificationReport.status !== 'PASS' || !Array.isArray(benchmarkSamples) || benchmarkSamples.length < 200 || benchmarkEnvironment.status !== 'PASS') {
  throw new Error('B04 targeted evidence markers are incomplete')
}
const integrityFailures = integrityMatches.reduce((sum, match) => sum + Number(match[1]), 0)
const write = (name, value) => writeFileSync(resolve(evidenceDir, name), `${JSON.stringify({ ...value, status: 'PASS', sourceCommit }, null, 2)}\n`)
write('long_book_10m_manifest.json', { generatorVersion: 'm0e.fixture.v1', seed: 'b04-release-scale', targetWords: Number(materialized[8]), materialized: true, materializedCounts: { books: Number(materialized[1]), volumes: Number(materialized[2]), chapters: Number(materialized[3]), artifacts: Number(materialized[4]), canonFacts: Number(materialized[5]), dependencies: Number(materialized[6]), modelRuns: Number(materialized[7]) }, sampleCounts: { chapters: Number(fixture[1]), artifacts: Number(fixture[2]), canonFacts: Number(fixture[3]), dependencies: Number(fixture[4]), modelRuns: Number(fixture[5]) }, scaleContract: { books: Number(scale[1]), volumes: Number(scale[2]), chapters: Number(scale[3]), minimumWords: Number(scale[4]), artifacts: Number(scale[5]), canonFacts: Number(scale[6]), dependencies: Number(scale[7]), modelRuns: Number(scale[8]) } })
write('perf_p95.json', { operation: 'all_prd_operations', fts: { samples: Number(performance[1]), p50Ms: Number(performance[2]), p95Ms: Number(performance[3]) }, operations: Object.fromEntries(perfOperations.map(item => [item[1], { samples: Number(item[2]), p50Ms: Number(item[3]), p95Ms: Number(item[4]), thresholdMs: requiredOperations[item[1]] }])), thresholdsMs: { ftsFirstScreen: 1000, projectOverview: 2000, chapterList: 300, canonCurrent: 250, adjacency: 500, normalCommit: 1000, recoveryReadable: 10000 }, referenceHardware: 'local test host' })
write('sqlite_runtime_profile.json', { profileVersion: 'm0e.sqlite.v1', pageSize: Number(profile[1]), autoVacuum: Number(profile[2]), mmapSize: Number(profile[3]), busyTimeoutMs: Number(profile[4]), synchronous: profile[5], walAutocheckpoint: Number(profile[6]), mmapPolicy: '256MiB_READ_CACHE', vacuumPolicy: 'NONE_WITH_FULL_MAINTENANCE_ONLY', checkpointPolicy: 'PASSIVE_WITH_MILESTONE_CHECKPOINT' })
write('wal_vacuum_report.json', { walBytes: Number(wal[1]), checkpointedPages: Number(wal[2]), vacuumPageCount: Number(wal[3]), thresholdState: wal[4], policy: 'PASSIVE_WITH_MILESTONE_CHECKPOINT' })
write('backup_restore_report.json', { checksumSha256: backup[1], restored: true, tamperRejected: true, restoreMode: 'maintenance-staged-atomic', quickCheck: true, foreignKeyCheck: true, deepIntegrity: true, receipt: true })
write('integrity_tamper_cases.json', { detectedFailureCount: integrityFailures, cases: ['ARTIFACT_PAYLOAD_HASH_MISMATCH', 'CANONICAL_HEAD_HASH_MISMATCH', 'DEPENDENCY_SOURCE_HASH_MISMATCH', 'COMMIT_RECEIPT_DIGEST_MISMATCH', 'BACKUP_CHECKSUM_MISMATCH'], failClosed: true })
write('long_book_scale_integrity.json', scaleIntegrity)
write('fts_incremental_scale.json', readJson('fts_incremental_scale.json'))
write('fts_upgrade_backfill.json', { sourceChapters: Number(upgrade[1]), sourceArtifacts: Number(upgrade[2]), preProjectionDocs: Number(upgrade[3]), postProjectionDocs: Number(upgrade[4]), rebuildTriggered: upgrade[5] === 'true', secondOpenFullRebuild: upgrade[6] === 'true', projectionVersion: upgrade[7] })
write('scale_perf_p95.json', scalePerformance)
write('scale_backup_restore.json', scaleRestore)
write('long_book_scale_certification.json', scaleCertificationReport)
write('benchmark_environment.json', benchmarkEnvironment)
writeFileSync(resolve(evidenceDir, 'changed-files.txt'), execFileSync('git', ['diff', '--name-status', 'd39e35ca3a4a9998e69fdc4c22cadaf5e281e494', sourceCommit], { cwd: root, encoding: 'utf8' }))
writeFileSync(resolve(evidenceDir, 'commands.log'), gates.gates.map(gate => `${gate.status} ${gate.name}: ${gate.command}`).join('\n') + '\n')
console.log(`B04 evidence assembled: 6 required files at ${evidenceDir}`)
