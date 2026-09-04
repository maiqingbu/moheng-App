import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => {
  const file = path.join(root, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}

export const FACTOR_CODES = [
  'OBLIGATION_OVERDUE',
  'FORESHADOW_AGING',
  'ROUTE_DRIFT',
  'TASK_BLOCKAGE',
  'CONTEXT_GROWTH',
  'MODEL_COST_DRIFT',
  'READER_DESIRE',
  'QUALITY_DRIFT',
]

const FROZEN_MIGRATIONS = {
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

function sourceCommit(projectRoot = root) {
  try { return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: projectRoot, encoding: 'utf8' }).trim() } catch { return null }
}

function templateText(source) {
  return source.match(/<template>([\s\S]*?)<\/template>/)?.[1]
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/\{\{[\s\S]*?\}\}/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim() ?? ''
}

function migrationHashes(projectRoot = root) {
  return Object.fromEntries(Object.entries(FROZEN_MIGRATIONS).map(([name, expected]) => {
    const file = path.join(projectRoot, 'src-tauri/migrations', name)
    const actual = fs.existsSync(file) ? crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null
    return [name, { expected, actual, status: actual === expected ? 'PASS' : 'FAIL' }]
  }))
}

export function inspectB13Architecture(projectRoot = root) {
  const readFrom = relative => {
    const file = path.join(projectRoot, relative)
    return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
  }
  const health = readFrom('src-tauri/src/health/mod.rs')
  const repository = readFrom('src-tauri/src/repositories/health.rs')
  const commands = readFrom('src-tauri/src/commands.rs')
  const lib = readFrom('src-tauri/src/lib.rs')
  const repos = readFrom('src-tauri/src/repositories/mod.rs')
  const router = readFrom('src/app/router.ts')
  const healthPage = readFrom('src/ui/pages/LongFormHealth.vue')
  const healthLanguage = readFrom('src/ui/presentation/healthLanguage.ts')
  const appShell = readFrom('src/ui/components/AppShell.vue')
  const runtimePort = readFrom('src/app/providers/runtimePort.ts')
  const connection = readFrom('src-tauri/src/project_db/connection.rs')
  const migration = readFrom('src-tauri/migrations/0012_b13_longform_health_indexes.sql')
  const schema = readFrom('contracts/schemas/health/long-form-health-projection-r1.schema.json')
  const oldHealthCall = /ui_projection::book_health/.test(commands)
  const healthSql = /\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM|PRAGMA\s+)/i.test(health)
  const forbiddenPage = /终局推进|progressToEnding|targetLength|obligationId|payloadHash|dependencyType|Completion|Endgame/.test(healthPage)
  const routeLine = router.split('\n').find(line => line.includes('/projects/:id/health')) ?? ''
  const frozen = migrationHashes(projectRoot)
  const frozenMigrations = Object.values(frozen).every(item => item.status === 'PASS')
  const checks = {
    health_owner_files: Boolean(health && repository),
    health_module_registered: /pub mod health/.test(lib) && /pub mod health/.test(repos),
    factor_policy: FACTOR_CODES.every(code => health.includes(code)) && /DETERMINISTIC_RUNTIME/.test(health) && /SOFT_ADVISORY/.test(health),
    contract: /LongFormHealthProjectionR1/.test(schema) && /long-form-health-projection\.r1/.test(schema) && FACTOR_CODES.every(code => schema.includes(code)),
    repository_sql_owner: /pub struct HealthRepository/.test(repository) && /LIMIT/.test(repository) && healthSql === false,
    no_authority_mutation: !/\b(?:INSERT|UPDATE|DELETE)\s+(?:INTO\s+)?(?:canon_|narrative_obligations|foreshadows|reader_desires)/i.test(health),
    no_model_call: !/(?:model.*transport|transport.*model|call.*model|ProductionModelGateway)/i.test(health),
    single_formal_query: /pub fn query_book_health/.test(health) && /crate::health::query_book_health/.test(commands) && !oldHealthCall,
    route_capability: /LongFormHealth/.test(routeLine) && !/UnavailableFeature/.test(routeLine) && /query_book_health/.test(commands),
    formal_page: Boolean(healthPage) && /八类健康因素/.test(healthPage) && /重新检查/.test(healthPage) && /数据积累中/.test(healthPage),
    presentation_mapper: /factorLabel/.test(healthPage) && /statusLabel/.test(healthPage) && /factorLabel/.test(healthLanguage) && /severityLabel/.test(healthLanguage),
    sidebar_active: /长篇健康/.test(appShell) && /\/health/.test(appShell) && !/尚未开放/.test(appShell.split('\n').find(line => line.includes('长篇健康')) ?? ''),
    browser_demo_explicit: /VITE_BROWSER_DEMO/.test(runtimePort) && /throw new Error/.test(runtimePort),
    migration_0012: fs.existsSync(path.join(projectRoot, 'src-tauri/migrations/0012_b13_longform_health_indexes.sql')) && /CREATE INDEX IF NOT EXISTS/.test(migration) && !/CREATE TABLE|CREATE TRIGGER|ALTER TABLE/i.test(migration),
    migration_history_12: /B13_LONGFORM_HEALTH_MIGRATION/.test(connection) && /version = 12/.test(connection) && /apply_numbered_migration\(connection, 12/.test(connection),
    frozen_migrations: frozenMigrations,
    no_future_owner: !/(?:B14|B15|RC1|EndgameOwner|CompletionGate|ExportReleaseOwner)/.test(`${health}\n${repository}`),
    product_surface_closed: !forbiddenPage,
    no_health_authority_table: !/CREATE\s+TABLE[^\n]*health/i.test(`${migration}\n${health}\n${repository}`),
  }
  const status = Object.values(checks).every(Boolean) ? 'PASS' : 'FAIL'
  return {
    schema: 'b13-architecture-r1',
    batch: 'B13',
    sourceCommit: sourceCommit(projectRoot),
    factorCodes: FACTOR_CODES,
    checks: Object.fromEntries(Object.entries(checks).map(([name, passed]) => [name, passed ? 'PASS' : 'FAIL'])),
    frozenMigrations: frozen,
    status,
  }
}

if (process.argv[1]?.endsWith('verify-b13-architecture.mjs')) {
  const report = inspectB13Architecture()
  console.log(JSON.stringify(report, null, 2))
  console.log(`B13_ARCHITECTURE_RESULT factors=${FACTOR_CODES.length} healthAuthority=NONE migration=0012 additive=${report.checks.migration_0012} status=${report.status}`)
  if (report.status !== 'PASS') process.exitCode = 1
}
