import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const commands = fs.readFileSync(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8')
const production = fs.readFileSync(new URL('../src-tauri/src/production.rs', import.meta.url), 'utf8')
const repair = fs.readFileSync(new URL('../src-tauri/src/repair.rs', import.meta.url), 'utf8')
const support = fs.readFileSync(new URL('../src-tauri/src/repositories/support.rs', import.meta.url), 'utf8')
const ui = fs.readFileSync(new URL('../src/ui/pages/ProductionCenter.vue', import.meta.url), 'utf8')

test('B08 production_start has one fail-closed composition root', () => {
  assert.match(commands, /if command_type == "production_start"/)
  assert.match(commands, /B08_CHAPTER_REQUIRED/)
  assert.match(commands, /run_chapter_production/)
  assert.doesNotMatch(production, /LocalProductionTransport|rendererBody/)
  assert.match(production, /ProductionModelGateway/)
  assert.match(production, /B06::ContextCompilerOwner/)
  assert.match(production, /B07::PlanningOwner/)
  assert.match(production, /AUDIT_EXTRACT/)
  assert.match(production, /validate_actual_extraction/)
  assert.match(production, /classify_plan_reality/)
})

test('H03 orchestration has no workflow SQL and UI requires explicit authorization', () => {
  assert.doesNotMatch(repair, /INSERT INTO repair_|UPDATE repair_|DELETE FROM repair_/)
  assert.match(support, /claim_repair_authorization/)
  assert.match(repair, /TypedRepairOwner/)
  assert.match(repair, /REPAIR_VERIFICATION_INCOMPLETE/)
  assert.match(commands, /B07::PlanningOwner/)
  assert.match(ui, /明确授权/)
  assert.match(ui, /approvedByUser: true/)
  assert.match(ui, /operationDigest/)
  assert.match(ui, /executionReceipt/)
})
