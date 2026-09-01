import test from 'node:test'
import assert from 'node:assert/strict'
import { inspectB11CapabilityParity } from '../scripts/verify-b11-capability-parity.mjs'

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
