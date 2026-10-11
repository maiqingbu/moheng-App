import assert from 'node:assert/strict'
import test from 'node:test'
import { findDemoCharacterIdLeaks } from '../scripts/architecture-rules.mjs'

test('fx3 architecture rule flags demo character ids in UI source', () => {
  assert.deepEqual(findDemoCharacterIdLeaks([
    { path: 'src/ui/pages/Graph.vue', source: "const id = 'shen'" },
  ]), [{ path: 'src/ui/pages/Graph.vue', id: 'shen' }])
})

test('fx3 architecture rule allows demo ids outside UI and arbitrary UI ids', () => {
  assert.deepEqual(findDemoCharacterIdLeaks([
    { path: 'src/infrastructure/browser-demo/demoPort.ts', source: "const id = 'shen'" },
    { path: 'src/ui/pages/Graph.vue', source: "const id = 'person-alpha'" },
  ]), [])
})
