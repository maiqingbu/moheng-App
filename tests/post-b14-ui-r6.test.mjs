import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('R6 authorization evidence must come from a runtime recorder, not static capture literals', () => {
  const bridge = read('src-tauri/src/bin/post_b14_ui_r4_bridge.rs')
  const capture = read('scripts/capture-post-b14-ui-r5.mjs')
  assert.match(bridge, /R6_TRACE|r6_query_trace|BRIDGE_RUNTIME_RECORDER/)
  assert.doesNotMatch(bridge, /SELECT\s+setting_key/) 
  assert.match(capture, /A09_AUTHORIZATION_RUNTIME_TRACE_R6\.json/)
  const confirmStart = capture.indexOf('const confirmTrace')
  assert.ok(confirmStart >= 0)
  assert.doesNotMatch(capture.slice(confirmStart, confirmStart + 180), /backendTrace:/)
})

test('R6 library delete failure is consumed by the UI event handler', () => {
  const library = read('src/ui/pages/ProjectLibrary.vue')
  assert.match(library, /async function deleteProject\(\)[\s\S]*try\s*\{[\s\S]*catch/)
})

test('R6 scans the complete product surface and uses the real production profile', () => {
  const capture = read('scripts/capture-post-b14-ui-r5.mjs')
  assert.match(capture, /FULL_PRODUCT_VISIBLE_DOM_SCAN_R6\.json/)
  for (const surface of ['P13_ASSISTANT_QUERY', 'P14_ASSISTANT_PROPOSAL', 'P15_ASSISTANT_RESULT']) assert.match(capture, new RegExp(surface))
  assert.match(capture, /Library\/Application Support\/com\.moheng\.ai-novel-production-system|TAURI_APP_DATA_DIR/)
  assert.doesNotMatch(capture, /const productionRoot = fs\.mkdtempSync/)
})

test('R6 package gate and acceptance are separate from the R5 package', () => {
  const scripts = fs.readdirSync(path.join(root, 'scripts')).join('\n')
  assert.match(scripts, /assemble-post-b14-ui-r6-package\.mjs/)
  assert.match(scripts, /run-post-b14-ui-r6-evidence\.mjs/)
})
