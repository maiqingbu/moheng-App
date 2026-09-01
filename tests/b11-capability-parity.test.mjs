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

test('b11_market_refresh_truthfulness', () => {
  const report = inspectB11CapabilityParity()
  assert.deepEqual(report.unsupportedMarketActions, [])
})

test('b11_frozen_ia_routes_preserved', () => {
  const router = read('src/app/router.ts')
  assert.match(router, /path:\s*['"]\/projects\/:id\/health['"]/, 'health route must remain in frozen IA')
  assert.match(router, /path:\s*['"]\/projects\/:id\/history['"]/, 'history route must remain in frozen IA')
})

test('b11_health_route_unavailable_before_b13', () => {
  const router = read('src/app/router.ts')
  const healthRoute = router.split('\n').find(line => line.includes('/projects/:id/health')) ?? ''
  assert.match(healthRoute, /UnavailableFeature/)
  assert.doesNotMatch(healthRoute, /LongFormHealth|query_book_health/)
})

test('b11_history_change_execution_unavailable_before_b12', () => {
  const router = read('src/app/router.ts')
  const historyRoute = router.split('\n').find(line => line.includes('/projects/:id/history')) ?? ''
  assert.match(historyRoute, /UnavailableFeature/)
  assert.doesNotMatch(historyRoute, /HistoryCenter|change_request_apply|retcon_apply/)
})

test('b11_sidebar_preserves_frozen_ia_without_fake_capability', () => {
  const shell = read('src/ui/components/AppShell.vue')
  assert.match(shell, /长篇健康/)
  assert.match(shell, /版本与变更记录/)
  assert.match(shell, /尚未开放/)
  assert.match(shell, /aria-disabled|disabled/)
  assert.doesNotMatch(shell, /to: `\/projects\/\$\{id\.value\}\/(?:health|history)`/)
})
