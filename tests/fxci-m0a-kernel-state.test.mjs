import assert from 'node:assert/strict'
import test from 'node:test'
import { hasKernelStateManageCall } from '../scripts/m0a-kernel-state-check.mjs'

test('FX-CI M0A accepts a single-line KernelState::new manage call', () => {
  assert.equal(hasKernelStateManageCall('app.manage(KernelState::new(paths));'), true)
})

test('FX-CI M0A accepts a multiline with_* KernelState constructor', () => {
  assert.equal(hasKernelStateManageCall(`app.manage(
    KernelState::with_skill_bundle_loader(paths, loader)
);`), true)
})

test('FX-CI M0A rejects main without KernelState registration', () => {
  assert.equal(hasKernelStateManageCall('app.manage(OtherState::new(paths));'), false)
})
