import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { evaluateB12Architecture } from '../scripts/verify-b12-architecture.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

const architectureSources = () => ({
  owner: read('src-tauri/src/change_retcon.rs'),
  repository: read('src-tauri/src/repositories/change_retcon.rs'),
  connection: read('src-tauri/src/project_db/connection.rs'),
  production: read('src-tauri/src/production.rs'),
  planning: read('src-tauri/src/planning.rs'),
  history: read('src/ui/pages/HistoryCenter.vue'),
  assistant: read('src/ui/pages/ProjectAssistant.vue'),
  retconFlow: read('src/ui/flows/b12RetconFlow.ts'),
  runtime: read('src-tauri/src/repositories/runtime.rs'),
})

test('B12 has an independent architecture verifier', () => {
  assert.equal(fs.existsSync(path.join(root, 'scripts/verify-b12-architecture.mjs')), true)
})

test('History UI exposes the complete Retcon plan and execution path', () => {
  const history = read('src/ui/pages/HistoryCenter.vue')
  const flow = read('src/ui/flows/b12RetconFlow.ts')
  assert.match(flow, /retcon_plan_create/)
  assert.match(flow, /retcon_apply/)
  assert.match(flow, /planDigest/)
  assert.match(history, /createRetconPlan/)
  assert.match(history, /confirmRetconPlan/)
})

test('B12 acceptance maps every gate to an existing evidence source', () => {
  const generator = read('scripts/generate-b12-acceptance.mjs')
  assert.match(generator, /existsSync\(source\)/)
  assert.match(generator, /b12_architecture/)
})

test('B12 Product UX gates consume real frontend sequence markers', () => {
  const runner = read('scripts/run-b12-evidence-gates.mjs')
  assert.match(runner, /B12_FRONTEND_HISTORY_RETCON_RESULT/)
  assert.match(runner, /B12_FRONTEND_ASSISTANT_RETCON_RESULT/)
  assert.match(runner, /B12_FRONTEND_EVIDENCE_FILE/)
  assert.match(runner, /B12_FRONTEND_EVIDENCE\.json/)
  assert.match(runner, /historyTwoStageConfirmation/)
  assert.doesNotMatch(runner, /setGate\('B12-A49',[^\n]*retcon_plan_create/)
})

test('B12 global preflight has a formal durable bootstrap job boundary', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  const runtime = read('src-tauri/src/repositories/runtime.rs')
  const commands = read('src-tauri/src/commands.rs')
  assert.match(owner, /ensure_dependency_summary_job/)
  assert.match(runtime, /DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE/)
  assert.match(runtime, /claim_dependency_summary_job/)
  assert.match(commands, /query_dependency_summary_bootstrap/)
})

test('b12_acceptance_rejects_chapter_self_whitelist', () => {
  const sources = architectureSources()
  const injected = {
    ...sources,
    planning: `${sources.planning}\nlet route_task_ids = candidate.activeTaskRefs;\nlet triggered_threads = Vec::new();`,
  }
  const result = evaluateB12Architecture(injected)
  assert.equal(result.checks.semantic_target_owner, 'FAIL')
})

test('b12_acceptance_rejects_unrelated_retcon_evidence', () => {
  const sources = architectureSources()
  const result = evaluateB12Architecture({
    ...sources,
    repository: sources.repository
      .replaceAll('exact_fact_evidence', 'arbitrary_project_latest_evidence')
      .replaceAll('supportsFactId', 'unrelatedEvidence'),
  })
  assert.equal(result.checks.grounding_semantics, 'FAIL')
})

test('b12_acceptance_rejects_expired_running_job_reclaim', () => {
  const sources = architectureSources()
  const result = evaluateB12Architecture({
    ...sources,
    runtime: sources.runtime
      .replace('"COMPLETED" | "RUNNING"', '"COMPLETED"')
      .replace("'QUEUED','PAUSED','FAILED'", "'QUEUED','PAUSED','RUNNING','FAILED'"),
  })
  assert.equal(result.checks.async_summary_worker, 'FAIL')
})

test('B12 semantic gates are blocking and source-bound', () => {
  const verifier = read('scripts/verify-b12-architecture.mjs')
  const runner = read('scripts/run-b12-evidence-gates.mjs')
  assert.match(verifier, /semantic_target_owner/)
  assert.match(verifier, /grounding_semantics/)
  assert.match(verifier, /async_summary_worker/)
  assert.match(runner, /B12_ASSISTANT_GROUNDING_RESULT/)
  assert.match(runner, /B12_ASSISTANT_RETCON_EVIDENCE_RESULT/)
  assert.match(runner, /B12_SUMMARY_LEASE_EXPIRY_RESULT/)
  assert.match(runner, /B12_SUMMARY_BOOTSTRAP_ASYNC_RESULT/)
  assert.match(runner, /B12_R08_NEGATIVE_INJECTIONS\.json/)
})
