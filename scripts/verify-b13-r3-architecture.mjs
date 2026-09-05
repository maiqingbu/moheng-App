import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

function read(relative) {
  const file = path.join(root, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}

function git(args) {
  try { return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim() } catch { return '' }
}

export function inspectB13R3Architecture(projectRoot = root, baseline = 'e6419ce') {
  const narrative = readFrom(projectRoot, 'src-tauri/src/repositories/narrative.rs')
  const volume = readFrom(projectRoot, 'src-tauri/src/volume_lifecycle.rs')
  const health = readFrom(projectRoot, 'src-tauri/src/repositories/health.rs')
  const uxGate = readFrom(projectRoot, 'scripts/verify-product-ux.mjs')
  const dualReader = readFrom(projectRoot, 'scripts/verify-b13-r3-dual-reader.mjs')
  const packageVerifier = readFrom(projectRoot, 'scripts/verify-package-consistency.mjs')
  const changed = gitFrom(projectRoot, ['diff', '--name-only', baseline, '--', 'src', 'src-tauri', 'contracts'])
  const changedFiles = changed.split('\n').filter(Boolean)
  const checks = {
    sharedChunkedTaskReader: /pub fn task_states_by_ids_complete/.test(narrative)
      && /TASK_STATE_CHUNK_SIZE: usize = 100/.test(narrative)
      && /chunks\(TASK_STATE_CHUNK_SIZE\)/.test(narrative),
    globalTaskLimitUnchanged: /fn limit\(limit: i64\)[\s\S]{0,120}limit\.clamp\(1, 100\)/.test(narrative),
    b09UsesSharedReader: /task_states_by_ids_complete\(connection, project_id, &task_ids\)/.test(volume),
    healthUsesSharedReader: /task_states_by_ids_complete\(connection, project_id, &task_id_refs\)/.test(health),
    currentFormalQueryConstants: /FORMAL_ROUTE_ROWS_SQL/.test(health)
      && /HEALTH_SOURCE_TIME_SQL/.test(health)
      && /TASK_STATES_BY_IDS_SQL/.test(narrative)
      && /ARTIFACT_PAYLOAD_BY_REF_SQL/.test(narrative),
    productUxCapturesPageHeaderProps: /<PageHeader\b[^>]*>/.test(uxGate)
      && /BookPlan|VolumePlan/.test(uxGate),
    dualReaderIndependentImplementations: /python-zipfile/.test(dualReader)
      && /resolveReaderB/.test(dualReader)
      && /7z/.test(dualReader)
      && /entrySetsEqual/.test(dualReader)
      && /contentHashesEqual/.test(dualReader),
    packageVerifierR3ReaderGate: /batch === 'B13_R3'/.test(packageVerifier)
      && /reader-b-list\.log/.test(packageVerifier),
    noMigrationChanges: !changedFiles.some(file => /src-tauri\/migrations\//.test(file)),
    noFutureBatchImplementation: !/(?:EndgameContract|CompletionGate|BookExitState|ExportReleaseOwner|RC1 certification)/i.test(narrative + volume + health),
  }
  return {
    schema: 'b13-r3-architecture',
    baseline,
    checks: Object.fromEntries(Object.entries(checks).map(([key, value]) => [key, value ? 'PASS' : 'FAIL'])),
    changedFiles,
    status: Object.values(checks).every(Boolean) ? 'PASS' : 'FAIL',
  }
}

function readFrom(projectRoot, relative) {
  const file = path.join(projectRoot, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}

function gitFrom(projectRoot, args) {
  try { return execFileSync('git', args, { cwd: projectRoot, encoding: 'utf8' }).trim() } catch { return '' }
}

if (process.argv[1]?.endsWith('verify-b13-r3-architecture.mjs')) {
  const baseline = process.argv[2] || 'e6419ce'
  const result = inspectB13R3Architecture(root, baseline)
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}
