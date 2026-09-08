import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('R5 assistant uses a one-way authorization and post-verification state machine', () => {
  const dock = read('src/ui/components/ProjectAssistantDock.vue')
  for (const state of ['PROPOSAL_READY', 'AWAITING_AUTHORIZATION', 'EXECUTING', 'EXECUTED', 'POST_VERIFYING', 'VERIFIED', 'FAILED']) {
    assert.match(dock, new RegExp(state))
  }
  assert.match(dock, /data-assistant-state/)
  assert.match(dock, /确认执行/) 
  assert.match(dock, /verified|VERIFIED/)
  assert.match(dock, /post.?verify|POST_VERIFYING/i)
})

test('R5 assistant hides confirmation after authorization and does not keep stale waiting copy', () => {
  const dock = read('src/ui/components/ProjectAssistantDock.vue')
  assert.match(dock, /assistantPhase.*AWAITING_AUTHORIZATION|AWAITING_AUTHORIZATION.*assistantPhase/)
  assert.match(dock, /v-if=.*AWAITING_AUTHORIZATION|v-if=.*assistantPhase/) 
  assert.doesNotMatch(dock, /proposal\.action === 'EXECUTABLE'.*等待授权.*确认执行/s)
})

test('R5 acceptance generators are fail-closed and require dedicated evidence', () => {
  const packageSource = fs.readdirSync(path.join(root, 'scripts')).filter(file => /post-b14-ui-r5|assemble-post-b14-ui-r5/i.test(file)).map(file => read(`scripts/${file}`)).join('\n')
  assert.ok(packageSource.length > 0, 'R5 package/evidence generator is missing')
  assert.doesNotMatch(packageSource, /const\s+(?:b13Pass|b14Pass)\s*=\s*true/)
  assert.doesNotMatch(packageSource, /status\s*:\s*['"]PASS['"]/)
  for (const evidence of ['ASSISTANT_STATE_MACHINE_R5.json', 'PROJECT_LIBRARY_E2E_MATRIX_R5.json', 'R5_DATA_ISOLATION.json', 'R5_EVIDENCE_SOURCE_BINDING.json']) {
    assert.match(packageSource, new RegExp(evidence.replaceAll('.', '\\.'), 'i'))
  }
})

