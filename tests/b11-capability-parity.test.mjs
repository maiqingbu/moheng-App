import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { inspectB11CapabilityParity } from '../scripts/verify-b11-capability-parity.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('b11_formal_ui_capability_parity', () => {
  const report = inspectB11CapabilityParity()
  assert.equal(report.status, 'PASS', JSON.stringify(report, null, 2))
  assert.deepEqual(report.missingHandlers, [])
  assert.ok(report.invokedCommands.includes('query_volume_detail'))
  assert.ok(report.registeredHandlers.includes('query_volume_detail'))
})

test('b11_future_route_hidden', () => {
  const report = inspectB11CapabilityParity()
  assert.deepEqual(report.futureRouteLeaks, [])
})

test('r2_04a_market_actions_have_a_formal_owner_and_no_unsupported_pulse_action', () => {
  const report = inspectB11CapabilityParity()
  assert.deepEqual(report.unsupportedMarketActions, [])
  assert.deepEqual(report.missingMarketActionOwners, [])
  assert.deepEqual(report.authorizedMarketActions, {
    marketRefresh: true,
    marketOpportunitySelect: true,
  })
})

test('b11_frozen_ia_routes_preserved', () => {
  const router = read('src/app/router.ts')
  assert.match(router, /path:\s*['"]\/projects\/:id\/health['"]/, 'health route must remain in frozen IA')
  assert.match(router, /path:\s*['"]\/projects\/:id\/history['"]/, 'history route must remain in frozen IA')
})

test('b11_frozen_health_route_is superseded by B13 formal capability', () => {
  const router = read('src/app/router.ts')
  const healthRoute = router.split('\n').find(line => line.includes('/projects/:id/health')) ?? ''
  assert.match(healthRoute, /LongFormHealth/)
  assert.doesNotMatch(healthRoute, /UnavailableFeature/)
})

test('b12_history_change_execution_is_active_after_b11_freeze', () => {
  const router = read('src/app/router.ts')
  const historyRoute = router.split('\n').find(line => line.includes('/projects/:id/history')) ?? ''
  assert.match(historyRoute, /HistoryCenter/)
  assert.doesNotMatch(historyRoute, /UnavailableFeature/)
})

test('b11_sidebar preserves the frozen IA while B13 enables health', () => {
  const shell = read('src/ui/components/AppShell.vue')
  assert.match(shell, /长篇健康/)
  assert.match(shell, /版本与变更记录/)
  assert.match(shell, /\/health/)
  assert.match(shell, /match: 'history'/)
})
