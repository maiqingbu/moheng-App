import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const commands = fs.readFileSync(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8')
const production = fs.readFileSync(new URL('../src-tauri/src/production.rs', import.meta.url), 'utf8')
const planning = fs.readFileSync(new URL('../src-tauri/src/planning.rs', import.meta.url), 'utf8')
const repair = fs.readFileSync(new URL('../src-tauri/src/repair.rs', import.meta.url), 'utf8')
const support = fs.readFileSync(new URL('../src-tauri/src/repositories/support.rs', import.meta.url), 'utf8')
const ui = fs.readFileSync(new URL('../src/ui/pages/ProductionCenter.vue', import.meta.url), 'utf8')
const projectStore = fs.readFileSync(new URL('../src/ui/stores/projectStore.ts', import.meta.url), 'utf8')

test('B08 production_start has one fail-closed composition root', () => {
  assert.match(commands, /if command_type == "production_start"/)
  assert.match(commands, /B08_CHAPTER_REQUIRED/)
  assert.match(commands, /run_chapter_production/)
  assert.doesNotMatch(production, /LocalProductionTransport|rendererBody/)
  assert.match(production, /ProductionModelGateway/)
  assert.match(production, /B06::ContextCompilerOwner/)
  assert.match(planning, /B07::PlanningOwner/)
  assert.match(production, /AUDIT_EXTRACT/)
  assert.match(production, /validate_actual_extraction/)
  assert.match(production, /classify_plan_reality/)
})

test('H03 orchestration has no workflow SQL and UI sends only plan identity for authorization', () => {
  assert.doesNotMatch(repair, /INSERT INTO repair_|UPDATE repair_|DELETE FROM repair_/)
  assert.match(support, /claim_repair_authorization/)
  assert.match(repair, /TypedRepairOwner/)
  assert.match(repair, /REPAIR_VERIFICATION_INCOMPLETE/)
  assert.match(commands, /B07::PlanningOwner/)
  assert.match(ui, /明确授权/)
  assert.match(ui, /operationDigest/)
  assert.match(ui, /executionReceipt/)
  // The previous approvedByUser assertion made renderer-supplied approval metadata part of the H03 contract.
  assert.doesNotMatch(ui + projectStore, /approvedByUser|issuedAt/)
  assert.match(projectStore, /payload: \{ repairPlanId: plan\.repairPlanId, operationDigest: plan\.operationDigest \}/)
})

test('R2 alignment keeps persistence SQL inside approved Rust owners', () => {
  const root = fileURLToPath(new URL('../src-tauri/src/', import.meta.url))
  const visit = directory => fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(directory, entry.name)
    return entry.isDirectory() ? visit(file) : entry.name.endsWith('.rs') ? [file] : []
  })
  const approved = new Set(['catalog', 'commit', 'project_db', 'repositories'])
  const violations = visit(root).filter(file => {
    const relative = path.relative(root, file)
    if (approved.has(relative.split(path.sep)[0])) return false
    const source = fs.readFileSync(file, 'utf8')
    return /\bquery_row\s*\(|\.prepare\s*\(|\bexecute_batch\s*\(|\.execute\s*\(\s*["']/.test(source)
  })
  assert.deepEqual(violations, [])
})
