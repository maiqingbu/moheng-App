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

test('Product UX gate fails closed when BookPlan and VolumePlan return to visible copy', () => {
  const source = read('src/ui/pages/VolumeManager.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/VolumeManager.vue'
      ? source.replace('区分规划、实际进展、系统推算与暂定安排，不把计划当成已经发生的事实。', 'BookPlan 只保存全书战略；详细 VolumePlan 按当前卷创建。')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.active_ui_language, 'FAIL')
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

test('Product UX gate exposes the R1-R2 acceptance gates independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.dynamic_capability_leakage, 'PASS')
  assert.equal(result.checks.composite_evidence_identity, 'PASS')
  assert.equal(result.checks.retcon_plan_user_surface, 'PASS')
  assert.equal(result.checks.browser_evidence_fixtures, 'PASS')
})

test('Product UX gate exposes the R1-R3 Canon surface gate independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.canon_user_surface, 'PASS')
})

test('Product UX gate exposes Reader capability truth independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.reader_capability_truth, 'PASS')
  assert.equal(result.readerCapabilityTruth.status, 'PASS')
})

test('Product UX gate fails closed when Reader invokes an unsupported capability', () => {
  const source = read('src/ui/pages/ManuscriptReader.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/ManuscriptReader.vue'
      ? source.replace('buildReaderRevisionCommands', "'chapter_revision_request'")
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails closed when Export appears available without backend support', () => {
  const source = read('src/ui/pages/ManuscriptReader.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/ManuscriptReader.vue'
      ? source.replace('const capability = await store.loadExportCapability(projectId)', 'const capability = { available:true, reason:null, supportedFormats:[\'TXT\'], currentFinalChapterCount:1 }')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails when Reader drops captured project identity from export requests', () => {
  const source = read('src/ui/pages/ManuscriptReader.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/ManuscriptReader.vue'
      ? source.replace("store.createExport(requestId, 'TXT', projectId)", "store.createExport(requestId, 'TXT')")
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails closed when Reader proposals regain the generic apply action', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('v-if="!readerProposalOnly" class="btn primary small"', 'class="btn primary small"')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails closed when Reader proposal history gets a generic applied label', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('historyEntryStatusLabel(entry)', 'historyStatusLabel(entry.status)')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails closed when History drops backend Reader provenance', () => {
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src-tauri/src/repositories/ui_projection.rs'
      ? ''
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate fails closed when History all filter excludes all change rows', () => {
  const source = read('src-tauri/src/repositories/ui_projection.rs')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src-tauri/src/repositories/ui_projection.rs'
      ? source.replace("WHERE ?2='all' OR entry_kind=?2", 'WHERE entry_kind=?2')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.reader_capability_truth, 'FAIL')
})

test('Product UX gate exposes cross-type History pagination integrity independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.history_pagination_integrity, 'PASS')
  assert.equal(result.historyPaginationIntegrity.status, 'PASS')
})

test('Product UX gate fails when History drops its deterministic cross-type ordering', () => {
  const source = read('src-tauri/src/repositories/ui_projection.rs')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src-tauri/src/repositories/ui_projection.rs'
      ? source.replace('ORDER BY created_at DESC,entry_id DESC,entry_kind DESC', 'ORDER BY created_at DESC')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_pagination_integrity, 'FAIL')
})

test('Product UX gate exposes History filter cache binding independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.history_filter_state_integrity, 'PASS')
  assert.equal(result.historyFilterStateIntegrity.status, 'PASS')
})

test('Product UX gate fails when History can reuse a page from another filter', () => {
  const source = read('src/ui/stores/projectStore.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/stores/projectStore.ts'
      ? source.replace('historyKind.value === kind && !force && cursor === null', 'historyKind.value !== kind && !force && cursor === null')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_filter_state_integrity, 'FAIL')
})

test('Product UX gate exposes History filter failure recovery independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.history_filter_async_recovery, 'PASS')
  assert.equal(result.historyFilterAsyncRecovery.status, 'PASS')
})

test('Product UX gate fails when History removes the filter retry path', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('@click="retryHistoryFilter"', '')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_filter_async_recovery, 'FAIL')
})

test('Product UX gate fails when History exposes old rows during filter loading', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('v-if="history && !historyFilterLoading"', 'v-if="history"')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_filter_async_recovery, 'FAIL')
})

test('Product UX gate exposes History cold-start recovery independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.history_filter_cold_recovery, 'PASS')
  assert.equal(result.historyFilterColdRecovery.status, 'PASS')
})

test('Product UX gate fails when History hides errors before any projection exists', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('v-if="history || historyFilterLoading || historyFilterError"', 'v-if="history"')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_filter_cold_recovery, 'FAIL')
})

test('Product UX gate exposes cold-start error copy truth independently', () => {
  const result = evaluateProductUx()
  assert.equal(result.checks.history_filter_copy_truth, 'PASS')
  assert.equal(result.historyFilterCopyTruth.status, 'PASS')
})

test('Product UX gate fails when a cold start claims old history remains visible', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replace('暂时无法加载历史记录，请重试。', '未能加载“全部”记录，仍显示“全部”记录。')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.history_filter_copy_truth, 'FAIL')
})

test('Product UX gate fails closed when the shared dynamic guard is bypassed', () => {
  const source = read('src/ui/presentation/productLanguage.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/presentation/productLanguage.ts'
      ? source.replaceAll('containsInternalProductToken(raw)', 'false')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.dynamic_presentation_leakage, 'FAIL')
})

test('Product UX gate fails closed when bare capability protection is removed', () => {
  const source = read('src/ui/presentation/productLanguage.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/presentation/productLanguage.ts'
      ? source.replaceAll('capabilityPairPattern', 'noCapabilityPairPattern')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.dynamic_capability_leakage, 'FAIL')
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
      ? source.replaceAll('evidenceSelector(item)', 'item.label').replaceAll('selectEvidenceBySelector', 'findEvidenceByLabel')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.evidence_selection_identity, 'FAIL')
})

test('Product UX gate fails closed when History drops Evidence version identity', () => {
  const source = read('src/ui/pages/HistoryCenter.vue')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/ui/pages/HistoryCenter.vue'
      ? source.replaceAll('item.version === active.evidenceVersion', 'true')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.composite_evidence_identity, 'FAIL')
})

test('Product UX gate fails closed when the internal RetconPlan name reaches demo copy', () => {
  const source = read('src/infrastructure/browser-demo/demoPort.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/infrastructure/browser-demo/demoPort.ts'
      ? source.replace('事实修订方案示例', 'RetconPlan 示例')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.retcon_plan_user_surface, 'FAIL')
})

test('Product UX gate fails closed when Canon returns to user-visible impact copy', () => {
  const source = read('src/infrastructure/browser-demo/demoPort.ts')
  const result = evaluateProductUx({
    sourceReader: relative => relative === 'src/infrastructure/browser-demo/demoPort.ts'
      ? source.replace('不改变已经确认的正式事实', '不改变已提交 Canon')
      : read(relative),
  })
  assert.equal(result.status, 'FAIL')
  assert.equal(result.checks.canon_user_surface, 'FAIL')
})
