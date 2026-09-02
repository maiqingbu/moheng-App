import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => {
  const file = path.join(root, relative)
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : ''
}

function functionBody(source, marker) {
  const start = source.indexOf(marker)
  if (start < 0) return ''
  const remainder = source.slice(start)
  const next = remainder.slice(marker.length).search(/\n(?:pub(?:\([^)]*\))?\s+)?fn\s+/)
  return next < 0 ? remainder : remainder.slice(0, marker.length + next + 1)
}

export function evaluateB12Architecture({
  owner = '',
  repository = '',
  connection = '',
  production = '',
  planning = '',
  history = '',
  assistant = '',
  retconFlow = '',
  runtime = '',
  migrationPresent = true,
} = {}) {
  const assistantGrounding = `${assistant}\n${retconFlow}`
  const targetOwner = functionBody(owner, 'fn materialize_typed_target')
  const bootstrap = functionBody(owner, 'pub fn bootstrap_dependency_summary')
  const worker = functionBody(owner, 'fn run_dependency_summary_worker')
  const checks = [
    ['owner', /B12::ChangeRetconOwner/.test(owner) && /resolve_typed_target/.test(owner) && /B12_CHANGE_TARGET_UNSUPPORTED/.test(owner)],
    ['real_target_head', /load_authority_head/.test(repository) && /head_updates/.test(owner) && /PlanningOwner::apply_typed_change/.test(targetOwner) && /VolumeLifecycleOwner::apply_typed_change/.test(targetOwner)],
    ['semantic_target_owner', /PlanningOwner::apply_typed_change/.test(targetOwner) && /VolumeLifecycleOwner::apply_typed_change/.test(targetOwner) && !/build_artifact_with_owner/.test(targetOwner) && /owner_receipt/.test(targetOwner) && /validate_chapter_plan_change_semantics/.test(planning) && /volume_plan_route/.test(planning) && /context_input_from_authority/.test(planning) && /validate_current_chapter_plan/.test(planning) && /validate_task_reality/.test(planning) && /validate_reservation_actions/.test(planning) && !/let route_task_ids\s*=\s*candidate/.test(planning) && !/let triggered_threads\s*=\s*Vec::new\(\)/.test(planning)],
    ['transitive_impact', /impact_frontier/.test(repository) && /expand_impact_batch/.test(repository) && /initialize_impact_frontier/.test(owner + repository)],
    ['bounded_checkpoint', /processed_pages_json/.test(repository) && /visitedCount/.test(owner + repository) && /maxFrontierBatch/.test(repository)],
    ['stale_runtime', /activate_stale_queue/.test(repository) && /impact_items/.test(repository) && /has_pending_stale_for_refs/.test(repository) && /PLAN_STALE:RECOMPILE_REQUIRED/.test(production)],
    ['retcon_plan_binding', /retcon_plan_digest/.test(owner) && /planDigest/.test(owner) && /retconPlanId/.test(owner)],
    ['history_retcon_path', /retcon_plan_create/.test(retconFlow) && /retcon_apply/.test(retconFlow) && /change_request_decide/.test(retconFlow) && /confirmRetconPlan/.test(history)],
    ['assistant_grounding', /queryRetconContext/.test(assistantGrounding) && /targetFactId/.test(assistantGrounding) && /evidenceRef/.test(assistantGrounding) && /change_request_create/.test(assistantGrounding) && /impact_preflight/.test(assistantGrounding)],
    ['grounding_semantics', /grounding_terms/.test(repository) && /score_grounding_candidate/.test(repository) && /AMBIGUOUS/.test(repository) && /needsUserSelection/.test(repository) && /exact_fact_evidence/.test(repository) && /supportsFactId/.test(repository) && /EVIDENCE_GAP/.test(repository) && !/f\.fact_id LIKE/.test(repository) && !/artifact_status='COMMITTED' AND artifact_type IN/.test(repository)],
    ['cold_summary_boundary', /SUMMARY_BOOTSTRAP_REQUIRED/.test(owner) && /summaryStatus/.test(repository)],
    ['async_summary_worker', /std::thread::Builder/.test(bootstrap) && /run_dependency_summary_worker/.test(bootstrap) && !/rebuild_global_dependency_summary/.test(bootstrap) && /rebuild_global_dependency_summary/.test(worker) && /save_checkpoint/.test(worker) && /worker_holder/.test(bootstrap) && /matches!\(current\.as_str\(\), "COMPLETED" \| "RUNNING"\)/.test(runtime) && !/QUEUED" \| "PAUSED" \| "RUNNING" \| "FAILED/.test(runtime) && /release_lease_for_holder/.test(runtime)],
    ['migration_0011', migrationPresent && /B12_CHANGE_RETCON_MIGRATION/.test(connection)],
    ['no_runtime_trigger_creation', !/CREATE TRIGGER/.test(connection)],
    ['stale_plan_owner', /has_pending_stale_for_refs/.test(planning)],
  ]
  const failures = checks.filter(([, passed]) => !passed).map(([name]) => name)
  return {
    schema: 'b12-architecture-verifier-r2',
    verifier: 'scripts/verify-b12-architecture.mjs',
    checks: Object.fromEntries(checks.map(([name, passed]) => [name, passed ? 'PASS' : 'FAIL'])),
    status: failures.length === 0 ? 'PASS' : 'FAIL',
    failures,
  }
}

if (process.argv[1]?.endsWith('verify-b12-architecture.mjs')) {
  const result = evaluateB12Architecture({
    owner: read('src-tauri/src/change_retcon.rs'),
    repository: read('src-tauri/src/repositories/change_retcon.rs'),
    connection: read('src-tauri/src/project_db/connection.rs'),
    production: read('src-tauri/src/production.rs'),
    planning: read('src-tauri/src/planning.rs'),
    history: read('src/ui/pages/HistoryCenter.vue'),
    assistant: read('src/ui/pages/ProjectAssistant.vue'),
    retconFlow: read('src/ui/flows/b12RetconFlow.ts'),
    runtime: read('src-tauri/src/repositories/runtime.rs'),
    migrationPresent: fs.existsSync(path.join(root, 'src-tauri/migrations/0011_b12_change_retcon_runtime.sql')),
  })
  console.log(JSON.stringify(result, null, 2))
  if (result.failures.length > 0) process.exitCode = 1
}
