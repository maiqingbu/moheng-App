import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { evaluateProductUx } from '../scripts/verify-product-ux.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('Product UX gate passes the active user surface and separates future shell', () => {
  const result = evaluateProductUx()
  assert.equal(result.status, 'PASS')
  assert.equal(result.activeUserSurface.visibleLeaks.length, 0)
  assert.equal(result.aiTraceLint.blockingMatches.length, 0)
  assert.equal(result.futureShellDeferred.status, 'PASS')
})

test('Product UX gate fails closed when active copy reintroduces an engineering term', () => {
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/SettingsPage.vue'
      ? read(relative).replace('可在这里测试模型服务连接', '正式产品由 Rust 受限命令测试连接')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.active_ui_language, 'FAIL')
})

test('Product UX gate fails closed when dynamic mapping is removed', () => {
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/components/CommandFeedback.vue'
      ? read(relative).replaceAll('commandMessage', 'rawMessage')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.presentation_mapping, 'FAIL')
})

test('Product UX gate does not treat internal source names as active UI leakage', () => {
  const source = read('src/ui/pages/ProjectAssistant.vue')
  assert.match(source, /projectId|prepareAssistantRepair/)
  const result = evaluateProductUx()
  assert.equal(result.activeUserSurface.visibleLeaks.length, 0)
})
