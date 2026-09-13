import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8')
const commands = read('src-tauri/src/commands.rs')
const planning = read('src-tauri/src/planning.rs')
const runtime = read('src-tauri/src/repositories/runtime.rs')
const detailProjection = read('src-tauri/src/repositories/ui_projection.rs')
const contracts = read('src/application/contracts.ts')
const port = read('src/infrastructure/tauri/frontendPort.ts')
const store = read('src/ui/stores/projectStore.ts')
const manager = read('src/ui/pages/VolumeManager.vue')
const workspace = read('src/ui/pages/VolumeWorkspace.vue')
const intentSchema = read('contracts/schemas/narrative/volume-intent-r1.schema.json')
const planSchema = read('contracts/schemas/narrative/volume-plan-r1.schema.json')

test('B03 production route rejects empty shells and compiles mandatory authorities', () => {
  assert.match(planning, /GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED/)
  assert.match(planning, /GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED/)
  assert.match(planning, /GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED/)
  assert.match(planning, /GOAL_CONTRACT_VOLUME_INTENT_REQUIRED/)
  assert.match(planning, /compile_volume_intent_from_authority/)
  assert.match(planning, /compile_volume_plan_from_authority/)
  assert.match(planning, /dependsOnTaskIds/)
  assert.match(planning, /completionPredicate/)
  assert.match(commands, /planning_create_volume_intent/)
  assert.match(commands, /planning_create_volume_plan/)
  assert.doesNotMatch(commands, /payload = json!\(\{\"payloadSchema\":\"volume-intent\.r1\"[^;]*mustAchieve":\[\]/)
  assert.doesNotMatch(commands, /payload = json!\(\{\"payloadSchema\":\"volume-plan\.r1\"[^;]*tasks":\[\]/)
})

test('B03 runtime phases are backend projected and UI displays plan/reality/projection', () => {
  for (const phase of ['READ_BOOK_PLAN', 'READ_CURRENT_STATE', 'BUILD_VOLUME_GOAL', 'VALIDATE_VOLUME_GOAL', 'COMMIT_VOLUME_INTENT', 'READ_VOLUME_INTENT', 'BUILD_TASK_ROUTE', 'VALIDATE_DEPENDENCIES', 'CALCULATE_CAPACITY', 'VALIDATE_ROUTE', 'COMMIT_VOLUME_PLAN']) {
    assert.match(runtime, new RegExp(phase))
  }
  assert.match(detailProjection, /runtimeOperation/)
  assert.match(manager, /查看当前卷规划进度/)
  assert.match(manager, /建立当前卷目标|正在建立当前卷目标/)
  assert.match(manager, /data-volume-planning-progress/)
  assert.match(workspace, /必须完成|可选安排|弹性安排|高潮任务|预留任务/)
  assert.match(workspace, /完成条件|路线约束|退出条件|容量预算/)
  assert.match(workspace, /REALITY/)
  assert.match(workspace, /PROJECTION/)
})

test('B03 contracts carry source bindings without adding a second authority', () => {
  for (const field of ['sourceBookPlanRef', 'compiledFromProjectRevision', 'sourceCanonRefs', 'sourceEvidenceRefs', 'semanticDigest']) assert.match(intentSchema, new RegExp(field))
  for (const field of ['goalContractRefs', 'compiledFromProjectRevision', 'sourceEvidenceRefs', 'semanticDigest']) assert.match(planSchema, new RegExp(field))
  assert.match(contracts, /planning_create_volume_intent/)
  assert.match(contracts, /planning_create_volume_plan/)
  assert.match(port, /planning_create_volume_intent|execute_command/)
  assert.match(store, /planning_create_volume_intent/)
  assert.match(store, /planning_create_volume_plan/)
})
