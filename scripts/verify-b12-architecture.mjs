import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => {
  const file = path.join(root, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}

const owner = read('src-tauri/src/change_retcon.rs')
const repository = read('src-tauri/src/repositories/change_retcon.rs')
const connection = read('src-tauri/src/project_db/connection.rs')
const production = read('src-tauri/src/production.rs')
const planning = read('src-tauri/src/planning.rs')
const history = read('src/ui/pages/HistoryCenter.vue')
const assistant = read('src/ui/pages/ProjectAssistant.vue')
const retconFlow = read('src/ui/flows/b12RetconFlow.ts')
const assistantGrounding = `${assistant}\n${retconFlow}`
const checks = [
  ['owner', /B12::ChangeRetconOwner/.test(owner) && /resolve_typed_target/.test(owner) && /B12_CHANGE_TARGET_UNSUPPORTED/.test(owner)],
  ['real_target_head', /load_authority_head/.test(repository) && /head_updates/.test(owner) && /validate_typed_target_payload/.test(owner)],
  ['transitive_impact', /impact_frontier/.test(repository) && /expand_impact_batch/.test(repository) && /initialize_impact_frontier/.test(owner + repository)],
  ['bounded_checkpoint', /processed_pages_json/.test(repository) && /visitedCount/.test(owner + repository) && /maxFrontierBatch/.test(repository)],
  ['stale_runtime', /activate_stale_queue/.test(repository) && /impact_items/.test(repository) && /has_pending_stale_for_refs/.test(repository) && /PLAN_STALE:RECOMPILE_REQUIRED/.test(production)],
  ['retcon_plan_binding', /retcon_plan_digest/.test(owner) && /planDigest/.test(owner) && /retconPlanId/.test(owner)],
  ['history_retcon_path', /retcon_plan_create/.test(retconFlow) && /retcon_apply/.test(retconFlow) && /change_request_decide/.test(retconFlow) && /confirmRetconPlan/.test(history)],
  ['assistant_grounding', /queryRetconContext/.test(assistantGrounding) && /targetFactId/.test(assistantGrounding) && /evidenceRef/.test(assistantGrounding) && /change_request_create/.test(assistantGrounding) && /impact_preflight/.test(assistantGrounding)],
  ['cold_summary_boundary', /SUMMARY_BOOTSTRAP_REQUIRED/.test(owner) && /summaryStatus/.test(repository)],
  ['migration_0011', fs.existsSync(path.join(root, 'src-tauri/migrations/0011_b12_change_retcon_runtime.sql')) && /B12_CHANGE_RETCON_MIGRATION/.test(connection)],
  ['no_runtime_trigger_creation', !/CREATE TRIGGER/.test(connection)],
  ['stale_plan_owner', /has_pending_stale_for_refs/.test(planning)],
]
const failures = checks.filter(([, passed]) => !passed).map(([name]) => name)
const result = {
  schema: 'b12-architecture-verifier-r1',
  verifier: 'scripts/verify-b12-architecture.mjs',
  checks: Object.fromEntries(checks.map(([name, passed]) => [name, passed ? 'PASS' : 'FAIL'])),
  status: failures.length === 0 ? 'PASS' : 'FAIL',
  failures,
}
console.log(JSON.stringify(result, null, 2))
if (failures.length > 0) process.exitCode = 1
