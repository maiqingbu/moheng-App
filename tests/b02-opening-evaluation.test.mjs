import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B02 formal opening evaluation reads committed reality and routes one signing owner', () => {
  const commands = read('src-tauri/src/commands.rs')
  const signing = read('src-tauri/src/signing.rs')
  const narrative = read('src-tauri/src/repositories/narrative.rs')
  const contracts = read('src/application/contracts.ts')
  const testSource = read('src-tauri/tests/b02_opening_evaluation.rs')
  assert.match(commands, /"opening_evaluate"\s*\|\s*"signing_retry"/)
  assert.match(commands, /"book_plan_revise"/) 
  assert.match(signing, /B02::OpeningEvaluationOwner/)
  assert.match(signing, /SimulatedSigningReceipt/)
  assert.match(narrative, /opening_evaluation_snapshot/)
  assert.doesNotMatch(commands, /signing_demo_(?:pass|revise|reject)/)
  assert.doesNotMatch(contracts, /signing_demo_(?:pass|revise|reject)/)
  assert.match(read('src/infrastructure/browser-demo/demoPort.ts'), /signing_demo_pass/)
  assert.doesNotMatch(read('src/infrastructure/browser-demo/demoPort.ts'), /case 'opening_evaluate'/)
  assert.doesNotMatch(testSource, /INSERT\s+(?:OR\s+IGNORE\s+)?INTO\s+(?:chapters|chapter_production_budgets|artifacts|project_runtime_state)/i)
})

test('B02 UI consumes backend evaluation phases and exposes no demo signing action', () => {
  const production = read('src/ui/pages/ProductionCenter.vue')
  const contracts = read('src/application/contracts.ts')
  const styles = read('src/ui/styles/pages.css')
  assert.match(production, /data-opening-evaluation/)
  assert.match(production, /正在评估/)
  assert.match(production, /评估完成/)
  assert.match(production, /评估依据摘要/)
  assert.match(production, /生成正式总纲/)
  assert.match(production, /opening_evaluate/)
  assert.match(production, /book_plan_revise/)
  assert.match(contracts, /opening_evaluate/)
  assert.match(styles, /opening-evaluation-pulse/)
  assert.doesNotMatch(production, /signing_demo_(?:pass|revise|reject)/)
  assert.doesNotMatch(production, /cursor\s*:\s*(?:wait|progress)/i)
  assert.doesNotMatch(production, /pointer-events\s*:\s*none/i)
})
