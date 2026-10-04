import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const read = relative => readFileSync(resolve(root, relative), 'utf8')

test('B11 exposes the two Chinese project-entry paths and the opening flow', () => {
  const launch = read('src/ui/pages/CreationLaunch.vue')
  const router = read('src/app/router.ts')
  assert.match(launch, /我已经有故事想法/)
  assert.match(launch, /帮我找值得写的方向/)
  assert.match(router, /StorySetup|story-setup|\/setup/)
  assert.match(router, /OpeningReady|opening-ready|\/opening/)
})

test('B11 wires Project Assistant through the existing frontend port and store-owned H03 commands', () => {
  assert.equal(existsSync(resolve(root, 'src/ui/pages/ProjectAssistant.vue')), true)
  const page = read('src/ui/pages/ProjectAssistant.vue')
  const port = read('src/infrastructure/tauri/frontendPort.ts')
  assert.match(page, /项目助手/)
  assert.match(page, /supportSession/)
  const store = read('src/ui/stores/projectStore.ts')
  assert.match(store, /prepareAssistantRepair/)
  assert.match(store, /repair_plan_create/)
  assert.match(store, /repair_authorize/)
  assert.match(store, /repair_execute/)
  assert.match(port, /support_session/)
  assert.match(port, /query_assistant_repair_plan/)
})

test('B11 formal UI does not expose demo or engineering fallback text', () => {
  const shell = read('src/ui/components/AppShell.vue')
  const page = read('src/ui/pages/ProjectAssistant.vue')
  assert.doesNotMatch(shell, /浏览器演示适配器只用于 UI 验证/)
  assert.doesNotMatch(page, /PLAN_STALE|RepairPlan|operationDigest|sourceRefs|IPC_UNSUPPORTED|作为 AI/)
})

test('R2-04A market entry requires an explicit live official sample or user-provided material', () => {
  const launch = read('src/ui/pages/CreationLaunch.vue')
  assert.match(launch, /marketSource/)
  assert.match(launch, /marketMaterial/)
  assert.match(launch, /project_create_from_market/)
  assert.match(launch, /selectedMarket = ref\(''\)/)
  assert.match(launch, /marketOpportunityId = selected\.id/)
  assert.match(launch, /marketDiscovery\.value\.status === 'READY'|\['READY', 'PARTIAL'\]/)
  assert.match(launch, /selected\.stale !== true/)
  assert.match(launch, /marketMaterial\.value\.trim\(\)/)
})

test('B11 decision inbox renders only backend-authorized decision actions', () => {
  const page = read('src/ui/pages/DecisionInbox.vue')
  const contracts = read('src/application/contracts.ts')
  const projection = read('src-tauri/src/repositories/ui_projection.rs')
  assert.match(contracts, /decisionType\?: string/)
  assert.match(contracts, /allowedActions\?: Array<'RETRY_REMOTE' \| 'MARK_FAILED' \| 'CANCEL_JOB'>/)
  assert.match(projection, /"decisionType"/)
  assert.match(projection, /"allowedActions"/)
  assert.match(page, /RETRY_REMOTE/)
  assert.match(page, /MARK_FAILED/)
  assert.match(page, /CANCEL_JOB/)
  assert.doesNotMatch(page, /choice: 'recommended'/)
})
