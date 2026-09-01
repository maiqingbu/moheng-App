import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const baseline = '6992b544f59f385160e3aa2f34f2e206ae715e59'
const frozenMigrationHashes = {
  'src-tauri/migrations/0001_catalog.sql': 'b56e29641a2edeb1cae02508c8c38363c9abdd97ef56c86d926668c121ab22e1',
  'src-tauri/migrations/0002_project_authority.sql': '7396fe1ec7769a898d423c97c3d063d7a64c3dc217de3967332bd25b84864924',
  'src-tauri/migrations/0003_project_command.sql': '2734404c53a80872b29d5fc4b455830e9fa8bda9d52760d0b812d9370e1630c7',
  'src-tauri/migrations/0004_project_longterm.sql': 'd06fbae9138bc15a72012b1e2b7ce063a5d25b3d54fa5f9deafe8bdcb6c5ae11',
  'src-tauri/migrations/0005_m0c_commit_delta.sql': 'e13b964f497c6398d912dc7f0ded92f39199de6f79f6d40043116e5d51a31d70',
  'src-tauri/migrations/0006_m0e_ops.sql': '13f11ea79c897a56580251fdf6b88b8babba4c67e623710246f56f694c516305',
  'src-tauri/migrations/0007_b05_narrative_foundation.sql': '90f148e5c8d3d9d72bfb2b17c00b06b14f6d201afe1fe5ebc1ee26f7b118d52f',
  'src-tauri/migrations/0008_h01_support_checkpoint.sql': '6cba8e0e1be0e24cebd8e66d2a0850552a27b56895a32db3577e434f25bb1d61',
  'src-tauri/migrations/0009_h03_repair_workflow.sql': 'b2ac4457e982b1b4e93d50b95e4c07d3e6a77f6b36ed2b796e2bc6ed576353ba',
}
const hasGit = fs.existsSync(path.join(root, '.git'))
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')
const sourceBindingPath = path.join(root, 'evidence/B09_H04/B09_H04_SOURCE_BINDING.json')
const sourceBinding = fs.existsSync(sourceBindingPath)
  ? JSON.parse(fs.readFileSync(sourceBindingPath, 'utf8'))
  : null
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

const sourceBindingPass = sourceBinding?.startCommit === baseline
  && sourceBinding?.startCommitIsAncestor === true
  && typeof sourceBinding?.sourceCommit === 'string'
  && sourceBinding.sourceCommit === sourceBinding.evidenceSourceCommit
  && sourceBinding.sourceCommit === sourceBinding.sourceCommitAfter
  && sourceBinding.workingTreeDirtyBefore === false
  && sourceBinding.workingTreeDirtyAfter === false
  && Array.isArray(sourceBinding.excludedUncommittedPaths)
  && sourceBinding.excludedUncommittedPaths.length === 0

const fileSha256 = relative => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, relative))).digest('hex')

const checks = {
  baselineAncestor: hasGit
    ? spawnSync('git', ['merge-base', '--is-ancestor', baseline, 'HEAD'], { cwd: root }).status === 0
    : sourceBindingPass,
  additiveMigration: migration.includes('B09/H04') && migration.includes('volume_planning_state_projection') && migration.includes('volume_route_replans') && migration.includes('volume_close_records') && migration.includes('volume_handoffs'),
  frozenMigrationsUnchanged: Object.entries(frozenMigrationHashes).every(([file, expectedHash]) => fs.existsSync(path.join(root, file)) && fileSha256(file) === expectedHash),
  singleLifecycleOwner: lifecycle.includes('pub struct VolumeLifecycleOwner') && lifecycle.includes('CommitService::commit_project_command') && !commands.includes('H03_CAPABILITY_UNAVAILABLE_BEFORE_B09'),
  b07Reuse: lifecycle.includes('planning::validate_volume_route') && lifecycle.includes('planning::validate_volume_route_reference') && lifecycle.includes('B09::VolumeLifecycleOwner'),
  projectionIsDerived: lifecycle.includes('persist_planning_state') && migration.includes("'B09::PlanningStateProjectionOwner', 1") && !lifecycle.includes('UPDATE canon_') && !lifecycle.includes('UPDATE state_deltas'),
  staleRaceIdempotency: lifecycle.includes('expected_revision') && lifecycle.includes('routeVersion') && lifecycle.includes('compile_route_replan_input') && lifecycle.includes('ProjectCommandOutcome::Replayed') && read('src-tauri/src/commit/mod.rs').includes('BEGIN IMMEDIATE'),
  closeAndHandoff: lifecycle.includes('VOLUME_CLOSE_BLOCKED') && lifecycle.includes('VolumeExitState') && lifecycle.includes('handoff_next_volume') && lifecycle.includes('NEXT_VOLUME_BLOCKED_ELIGIBILITY'),
  productionRootConnected: production.includes('VolumeLifecycleOwner::rebuild_planning_state') && commands.includes('return crate::production::run_chapter_production'),
  h03TypedOwner: repair.includes('replan_volume_route') && repair.includes('rebuild_planning_state_projection') && harness.includes('B09::VolumeLifecycleOwner') && harness.includes('H03_REPAIR_STAGE'),
  queryBoundary: commands.includes('query_volume_lifecycle_impl') && commands.includes('query_volume_lifecycle') && frontend.includes("'query_volume_lifecycle'") && contracts.includes('VolumeLifecycleProjection'),
  uiTruthful: store.includes('loadVolumeLifecycle') && ui.includes('B09::VolumeLifecycleOwner') && ui.includes('volume_close') && ui.includes('closeReadiness') && ui.includes('routeReplans') && ui.includes('latestHandoff') && ui.includes('unresolvedResponsibilities') && ui.includes('OPEN / SUSPENDED'),
  evidenceMarkers: read('src-tauri/tests/b09_h04.rs').includes('B09_ROUTE_REPLAN_RACE_RESULT') && read('src-tauri/tests/b09_h04.rs').includes('H04_REPLAN_AUTH_RESULT') && read('src-tauri/tests/b09_h04.rs').includes('B09_MULTI_VOLUME_RESULT'),
  noFourthHarness: !rust.includes('FourthHarness') && !rust.includes('H05::') && !contracts.includes("'h05_"),
  noFutureBatch: !rust.includes('B10::') && !rust.includes('B11::') && !contracts.includes("'b10_") && !contracts.includes("'b11_"),
  noSecondCommitProtocol: !rust.includes('CommitBundleV3') && !rust.includes('NarrativeRuntimeDelta'),
  receiverSourceBinding: hasGit || sourceBindingPass,
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
  hasGit,
  sourceBinding: sourceBinding ?? null,
  status: failed.length === 0 ? 'PASS' : 'FAIL',
  checks: Object.fromEntries(Object.entries(checks).map(([name, pass]) => [name, pass ? 'PASS' : 'FAIL'])),
  sqlOutsideOwners: sqlOutsideOwners.map(file => path.relative(root, file)),
  failed,
}
console.log(JSON.stringify(result, null, 2))
if (result.status !== 'PASS') process.exit(1)
console.log('B09_H04_ARCHITECTURE_RESULT projection=PASS route_owner=PASS close_handoff=PASS h03_typed_owner=PASS ui_query=PASS migration_freeze=PASS status=PASS')
