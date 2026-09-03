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

test('Product UX gate exposes independent dynamic and evidence behavior gates', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.dynamic_presentation_leakage, 'PASS')
  assert.equal(result.checks.assistant_dynamic_leakage, 'PASS')
  assert.equal(result.checks.b12_evidence_data_separation, 'PASS')
  assert.equal(result.checks.evidence_selection_identity, 'PASS')
})

test('Product UX gate fails closed when the shared dynamic guard is bypassed', () => {
  const source = read('src/ui/presentation/productLanguage.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/presentation/productLanguage.ts'
      ? source.replace('containsInternalProductToken(raw)', 'false')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.dynamic_presentation_leakage, 'FAIL')
})

test('Product UX gate fails closed when the Retcon flow maps Evidence for display', () => {
  const source = read('src/ui/flows/b12RetconFlow.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/flows/b12RetconFlow.ts'
      ? source.replace('return { fact, evidence }', 'return { fact, evidence: { ...evidence, label: evidenceLabel(evidence.label) } }')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.b12_evidence_data_separation, 'FAIL')
})

test('Product UX gate fails closed when History uses display text as Evidence identity', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('evidenceSelector(item)', 'item.label').replace('selectEvidenceBySelector', 'findEvidenceByLabel')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.evidence_selection_identity, 'FAIL')
})
