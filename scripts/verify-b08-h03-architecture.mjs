import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const production = read('src-tauri/src/production.rs')
const repair = read('src-tauri/src/repair.rs')
const support = read('src-tauri/src/repositories/support.rs')
const commands = read('src-tauri/src/commands.rs')
const planning = read('src-tauri/src/planning.rs')
const repositoryProduction = read('src-tauri/src/repositories/production.rs')
const ui = read('src/ui/pages/ProductionCenter.vue')
const productionDispatchStart = commands.indexOf('if command_type == "production_start"')
const productionOwnerReturn = commands.indexOf('return crate::production::run_chapter_production', productionDispatchStart)
const runtimeControlCall = commands.indexOf('execute_runtime_command(', productionOwnerReturn)
const approvedSqlOwners = new Set(['catalog', 'commit', 'project_db', 'repositories'])
function rustFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const target = path.join(directory, entry.name)
    return entry.isDirectory() ? rustFiles(target) : entry.name.endsWith('.rs') ? [target] : []
  })
}
const sqlOutsideOwners = rustFiles(path.join(root, 'src-tauri/src')).filter(file => {
  const relative = path.relative(path.join(root, 'src-tauri/src'), file)
  if (approvedSqlOwners.has(relative.split(path.sep)[0])) return false
  return /\bquery_row\s*\(|\.prepare\s*\(|\bexecute_batch\s*\(|\.execute\s*\(\s*["']/.test(fs.readFileSync(file, 'utf8'))
})

const checks = {
  singleProductionRoot: commands.includes('return crate::production::run_chapter_production') && commands.includes('B08_CHAPTER_REQUIRED'),
  noLegacyProductionSuccess: productionDispatchStart >= 0 && productionOwnerReturn > productionDispatchStart && runtimeControlCall > productionOwnerReturn,
  b06ContextOwner: production.includes('context_input_from_authority') && production.includes('B06::ContextCompilerOwner'),
  b07PlanningOwner: planning.includes('B07::PlanningOwner') && commands.includes('recompile_chapter_plan_owner'),
  injectedTransport: production.includes('ProductionModelGateway') && !production.includes('LocalProductionTransport') && !production.includes('rendererBody'),
  actualAuditGate: production.includes('AUDIT_EXTRACT') && production.includes('validate_actual_extraction'),
  realityDelta: production.includes('canonDelta') && production.includes('knowledgeDelta') && production.includes('stateDelta') && production.includes('obligationDelta'),
  reconciliation: production.includes('classify_plan_reality'),
  candidateIsolation: !production.includes('"artifactType":"ChapterCandidate","scope"'),
  h03NoRawWorkflowSql: !/(INSERT|UPDATE|DELETE)\s+(INTO\s+)?repair_/i.test(repair),
  h03RepositoryOwner: /INSERT INTO repair_plans/.test(support) && support.includes('claim_repair_authorization'),
  h03TypedOwners: repair.includes('TypedRepairOwner') && commands.includes('B07::PlanningOwner') && commands.includes('B04::BackupService'),
  h03Requery: repair.includes('owner.verify') && repair.includes('REPAIR_VERIFICATION_INCOMPLETE'),
  explicitAuthorizationUi: ui.includes('明确授权') && ui.includes('approvedByUser: true') && ui.includes('operationDigest'),
  productionCenterR1: ['chapterPlan','triggeredContinuity','modelRuns','candidateRef','finalChapterRef','reconciliation','repair'].every(field => ui.includes(field)),
  sqlOwnerBoundary: sqlOutsideOwners.length === 0 && repositoryProduction.includes('center_projection'),
  noFutureBatch: !production.includes('B09::') && !repair.includes('H04::'),
}
const forbidden = ['NarrativeRuntimeDelta', 'CommitBundleV3'].filter(pattern => `${production}\n${repair}`.includes(pattern))
const failed = Object.entries(checks).filter(([, pass]) => !pass).map(([name]) => name)
const result = { schema:'b08-h03-architecture-r2', status:failed.length === 0 && forbidden.length === 0 ? 'PASS' : 'FAIL', checks:Object.fromEntries(Object.entries(checks).map(([name,pass]) => [name,pass?'PASS':'FAIL'])), sqlOutsideOwners, forbidden, failed }
console.log(JSON.stringify(result, null, 2))
if (result.status !== 'PASS') process.exit(1)
