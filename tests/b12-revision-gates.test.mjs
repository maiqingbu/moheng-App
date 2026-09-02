import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

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
