import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const baseline = '6992b544f59f385160e3aa2f34f2e206ae715e59'
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')
const lifecycle = read('src-tauri/src/volume_lifecycle.rs')
const repository = read('src-tauri/src/repositories/volume_lifecycle.rs')
const commands = read('src-tauri/src/commands.rs')
const repair = read('src-tauri/src/repair.rs')
const harness = read('src-tauri/src/harness/mod.rs')
const production = read('src-tauri/src/production.rs')
const contracts = read('src/application/contracts.ts')
const frontend = read('src/infrastructure/tauri/frontendPort.ts')
const store = read('src/ui/stores/projectStore.ts')
const ui = read('src/ui/pages/VolumeWorkspace.vue')
const migration = read('src-tauri/migrations/0010_b09_h04_volume_lifecycle.sql')
const rust = [lifecycle, repository, commands, repair, harness, production].join('\n')

const checks = {
  baselineAncestor: spawnSync('git', ['merge-base', '--is-ancestor', baseline, 'HEAD'], { cwd: root }).status === 0,
  additiveMigration: migration.includes('B09/H04') && migration.includes('volume_planning_state_projection') && migration.includes('volume_route_replans') && migration.includes('volume_close_records') && migration.includes('volume_handoffs'),
  frozenMigrationsUnchanged: Array.from({ length: 9 }, (_, index) => `src-tauri/migrations/${String(index + 1).padStart(4, '0')}_${['catalog','project_authority','project_command','project_longterm','m0c_commit_delta','m0e_ops','b05_narrative_foundation','h01_support_checkpoint','h03_repair_workflow'][index]}.sql`).every(file => {
    const current = path.join(root, file)
    const baselineFile = spawnSync('git', ['show', `${baseline}:${file}`], { cwd: root, encoding: 'buffer' })
    return baselineFile.status === 0 && fs.existsSync(current) && crypto.createHash('sha256').update(fs.readFileSync(current)).digest('hex') === crypto.createHash('sha256').update(baselineFile.stdout).digest('hex')
  }),
  singleLifecycleOwner: lifecycle.includes('pub struct VolumeLifecycleOwner') && lifecycle.includes('CommitService::commit_project_command') && !commands.includes('H03_CAPABILITY_UNAVAILABLE_BEFORE_B09'),
  b07Reuse: lifecycle.includes('planning::validate_volume_route') && lifecycle.includes('planning::validate_volume_route_reference') && lifecycle.includes('B09::VolumeLifecycleOwner'),
  projectionIsDerived: lifecycle.includes('persist_planning_state') && migration.includes("'B09::PlanningStateProjectionOwner', 1") && !lifecycle.includes('UPDATE canon_') && !lifecycle.includes('UPDATE state_deltas'),
  staleRaceIdempotency: lifecycle.includes('expected_revision') && lifecycle.includes('ROUTE_VERSION_CONFLICT') && lifecycle.includes('ProjectCommandOutcome::Replayed') && read('src-tauri/src/commit/mod.rs').includes('BEGIN IMMEDIATE'),
  closeAndHandoff: lifecycle.includes('VOLUME_CLOSE_BLOCKED') && lifecycle.includes('VolumeExitState') && lifecycle.includes('handoff_next_volume') && lifecycle.includes('NEXT_VOLUME_BLOCKED_ELIGIBILITY'),
  productionRootConnected: production.includes('VolumeLifecycleOwner::rebuild_planning_state') && commands.includes('return crate::production::run_chapter_production'),
  h03TypedOwner: repair.includes('replan_volume_route') && repair.includes('rebuild_planning_state_projection') && harness.includes('B09::VolumeLifecycleOwner') && harness.includes('H03_REPAIR_STAGE'),
  queryBoundary: commands.includes('query_volume_lifecycle_impl') && commands.includes('query_volume_lifecycle') && frontend.includes("'query_volume_lifecycle'") && contracts.includes('VolumeLifecycleProjection'),
  uiTruthful: store.includes('loadVolumeLifecycle') && ui.includes('B09::VolumeLifecycleOwner') && ui.includes('volume_close') && ui.includes('closeReadiness'),
  evidenceMarkers: read('src-tauri/tests/b09_h04.rs').includes('B09_ROUTE_REPLAN_RACE_RESULT') && read('src-tauri/tests/b09_h04.rs').includes('H04_REPLAN_AUTH_RESULT') && read('src-tauri/tests/b09_h04.rs').includes('B09_MULTI_VOLUME_RESULT'),
  noFutureBatch: !rust.includes('B10::') && !rust.includes('B11::') && !contracts.includes("'b10_") && !contracts.includes("'b11_"),
  noSecondCommitProtocol: !rust.includes('CommitBundleV3') && !rust.includes('NarrativeRuntimeDelta'),
}

const sourceFiles = []
const walk = directory => {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) walk(target)
    else if (entry.name.endsWith('.rs')) sourceFiles.push(target)
  }
}
walk(path.join(root, 'src-tauri/src'))
const sqlOutsideOwners = sourceFiles.filter(file => {
  const relative = path.relative(path.join(root, 'src-tauri/src'), file).split(path.sep)[0]
  if (['catalog', 'commit', 'project_db', 'repositories'].includes(relative)) return false
  return /\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM)\b/i.test(fs.readFileSync(file, 'utf8'))
})
checks.sqlOwnerBoundary = sqlOutsideOwners.length === 0

const failed = Object.entries(checks).filter(([, pass]) => !pass).map(([name]) => name)
const result = {
  schema: 'b09-h04-architecture-r1',
  baseline,
  status: failed.length === 0 ? 'PASS' : 'FAIL',
  checks: Object.fromEntries(Object.entries(checks).map(([name, pass]) => [name, pass ? 'PASS' : 'FAIL'])),
  sqlOutsideOwners: sqlOutsideOwners.map(file => path.relative(root, file)),
  failed,
}
console.log(JSON.stringify(result, null, 2))
if (result.status !== 'PASS') process.exit(1)
console.log('B09_H04_ARCHITECTURE_RESULT projection=PASS route_owner=PASS close_handoff=PASS h03_typed_owner=PASS ui_query=PASS migration_freeze=PASS status=PASS')
