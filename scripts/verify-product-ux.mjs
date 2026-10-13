import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.existsSync(path.join(root, relative)) ? fs.readFileSync(path.join(root, relative), 'utf8') : ''

export const ACTIVE_USER_SURFACE = [
  'src/ui/components/AppShell.vue',
  'src/ui/components/CommandFeedback.vue',
  'src/ui/components/ProjectSwitcher.vue',
  'src/ui/components/RuntimeBanner.vue',
  'src/ui/pages/CanonGraph.vue',
  'src/ui/pages/CreationLaunch.vue',
  'src/ui/pages/DecisionInbox.vue',
  'src/ui/pages/HistoryCenter.vue',
  'src/ui/pages/ManuscriptReader.vue',
  'src/ui/pages/MarketStrategy.vue',
  'src/ui/pages/OpeningReady.vue',
  'src/ui/pages/PitchReview.vue',
  'src/ui/pages/ProductionCenter.vue',
  'src/ui/pages/ProjectAssistant.vue',
  'src/ui/pages/ProjectLibrary.vue',
  'src/ui/pages/ProjectOverview.vue',
  'src/ui/pages/SettingsPage.vue',
  'src/ui/pages/StorySetup.vue',
  'src/ui/pages/UnavailableFeature.vue',
  'src/ui/pages/VolumeManager.vue',
  'src/ui/pages/VolumeWorkspace.vue',
  'src/ui/pages/LongFormHealth.vue',
  'src/ui/pages/Completion.vue',
]

const forbiddenVisiblePatterns = [
  /\bRust\b/i,
  /\bTypeScript\b|\bJavaScript\b/i,
  /\bVue\b|\bPinia\b|\bTauri\b|\bIPC\b|\bSQLite\b/i,
  /\bKeychain\b|\bWebView\b/i,
  /\b(?:projectId|artifactId|payloadHash|sourceCommit|changeRequestId|authorizationId|repairPlanId|operationDigest|expectedRevision|ownerReceiptRef)\b/i,
  /\b(?:PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED)\b/,
  /\b(?:CommitBundle|Narrative Authority|PlanningOwner|ProjectionOwner|VolumeLifecycleOwner|CommitService|RepairPlan|CandidatePatch|RepairSuggestion|RuntimeCheckRequest|RetconPlan)\b/,
  /\b(?:BookExitState|BookCompletionReport|EndgameContract|CompletionSnapshot|CompletionGate|ExitState|CompletionReport|READY|BLOCKED|NEEDS_DECISION|SOURCE_UNAVAILABLE)\b/,
  /\b(?:BookPlan|VolumePlan)\b/,
  /\b(?:OBLIGATION_OVERDUE|FORESHADOW_AGING|ROUTE_DRIFT|TASK_BLOCKAGE|CONTEXT_GROWTH|MODEL_COST_DRIFT|READER_DESIRE|QUALITY_DRIFT|SOURCE_UNAVAILABLE|INSUFFICIENT_DATA)\b/,
  /\bCanon\b/,
  /根据架构要求|正式实现必须|不常驻页面内存|AI 推荐理由|根据你提供的上下文|作为 AI|以下是详细分析|希望这些建议/,
]

const aiTracePatterns = [
  /作为\s*AI/,
  /根据你提供的(?:信息|上下文)/,
  /以下是详细分析/,
  /希望(?:这些建议|以上内容)对你有所帮助/,
  /正式实现必须|根据架构要求/,
]

function templateText(source) {
  return source.match(/<template>([\s\S]*?)<\/template>/)?.[1]
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/\{\{[\s\S]*?\}\}/g, ' ')
    // PageHeader props are rendered user-facing copy even though they live
    // on a component tag rather than as text nodes in the template.
    .replace(/<PageHeader\b([^>]*)>/g, '$1 ')
    .replace(/=>/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim() ?? ''
}

function findMatches(text, patterns) {
  return patterns.flatMap(pattern => {
    const matches = text.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`)) ?? []
    return matches.map(match => ({ pattern: pattern.source, match }))
  })
}

function sourceCommit() {
  try { return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim() } catch { return null }
}

export function evaluateProductUx({ sourceReader = read } = {}) {
  const activeFiles = ACTIVE_USER_SURFACE.map(relative => ({ relative, source: sourceReader(relative) }))
  const missing = activeFiles.filter(item => !item.source).map(item => item.relative)
  const visibleLeaks = activeFiles.flatMap(item => findMatches(templateText(item.source), forbiddenVisiblePatterns).map(match => ({ file: item.relative, ...match })))
  const aiLeaks = activeFiles.flatMap(item => findMatches(templateText(item.source), aiTracePatterns).map(match => ({ file: item.relative, ...match })))
  const feedback = sourceReader('src/ui/components/CommandFeedback.vue')
  const history = sourceReader('src/ui/pages/HistoryCenter.vue')
  const library = sourceReader('src/ui/pages/ProjectLibrary.vue')
  const assistant = sourceReader('src/ui/pages/ProjectAssistant.vue')
  const production = sourceReader('src/ui/pages/ProductionCenter.vue')
  const canon = sourceReader('src/ui/pages/CanonGraph.vue')
  const volume = sourceReader('src/ui/pages/VolumeWorkspace.vue')
  const reader = sourceReader('src/ui/pages/ManuscriptReader.vue')
  const readerRevision = sourceReader('src/ui/flows/readerRevision.ts')
  const tauriPort = sourceReader('src/infrastructure/tauri/frontendPort.ts')
  const historyCenter = sourceReader('src/ui/pages/HistoryCenter.vue')
  const historyProjection = sourceReader('src-tauri/src/repositories/ui_projection.rs')
  const historyStore = sourceReader('src/ui/stores/projectStore.ts')
  const contracts = sourceReader('src/application/contracts.ts')
  const language = sourceReader('src/ui/presentation/productLanguage.ts')
  const assistantPresentation = sourceReader('src/ui/presentation/assistantPresentation.ts')
  const flow = sourceReader('src/ui/flows/b12RetconFlow.ts')
  const demoPort = sourceReader('src/infrastructure/browser-demo/demoPort.ts')
  const router = sourceReader('src/app/router.ts')
  const healthPage = sourceReader('src/ui/pages/LongFormHealth.vue')
  const healthLanguage = sourceReader('src/ui/presentation/healthLanguage.ts')
  const completionPage = sourceReader('src/ui/pages/Completion.vue')
  const healthRoute = router.split('\n').find(line => line.includes('/projects/:id/health')) ?? ''
  const dynamicTokenCoverage = /export function containsInternalProductToken/.test(language)
    && /containsInternalProductToken\(raw\)/.test(language)
    && /Rust/.test(language)
    && /TypeScript/.test(language)
    && /Harness/.test(language)
    && /skillId/.test(language)
  const evidenceDataSeparation = !/evidenceLabel/.test(flow) && /return \{ fact, evidence \}/.test(flow)
  const evidenceSelectionIdentity = /evidenceSelector/.test(history)
    && /selectEvidenceBySelector/.test(history)
    && !/evidence\.find\(item => item\.label ===/.test(history)
  const dynamicCapabilityCoverage = /const capabilityPairPattern/.test(language)
    && /capabilityPairPattern\.test\(value\)/.test(language)
  const compositeEvidenceIdentity = /item\.artifactId === active\.evidenceRef/.test(history)
    && /item\.version === active\.evidenceVersion/.test(history)
    && /active\.evidencePayloadHash/.test(history)
    && /item\.payloadHash === active\.evidencePayloadHash/.test(history)
  const retconPlanUserSurface = !/\bRetconPlan\b/.test(demoPort)
  const browserEvidenceFixtures = /productUxEvidenceMode/.test(assistant)
    && /m11/.test(assistant)
    && /productUxEvidenceMode/.test(history)
    && /m12/.test(history)
    && /productUxEvidenceMode/.test(production)
    && /m13/.test(production)
    && /m14-stale/.test(production)
    && /m14-blocked/.test(production)
    && /m14-error/.test(production)
  const canonDisplayLeak = /(?:summary|impact|message)\s*[:=][^\n]*\bCanon\b/.test(demoPort)
  const canonUserSurface = !canonDisplayLeak
  // FX-10 / ADR-032: a busy writer is shown in product language with a retry,
  // never as the raw manager error text.
  const rawWriterErrorLeak = [...activeFiles.map(item => item.source), feedback, language, historyStore]
    .some(source => /project writer already open/i.test(source))
  const projectBusyRecovery = /PROJECT_BUSY:\s*'项目正在处理后台任务，请稍后再试/.test(language)
    && /retryCommand/.test(feedback)
    && />重试</.test(feedback)
    && /function retryCommand/.test(historyStore)
    && /PROJECT_BUSY/.test(historyStore)
    && !rawWriterErrorLeak
  // FX-12: withdraw-responsibility requests are decided in the History Center
  // with product-language outcomes, never raw codes.
  const withdrawRequestSurface = /data-withdraw-requests/.test(history)
    && /decideWithdrawRequest/.test(history)
    && /withdrawOriginLabel/.test(history)
    && /function decideWithdrawRequest/.test(historyStore)
    && /RESPONSIBILITY_WITHDRAW_NOT_PENDING:\s*'[\u4e00-\u9fff]/.test(language)
    && /WITHDRAW_APPROVED/.test(language)
  const checks = {
    active_ui_language: missing.length === 0 && visibleLeaks.length === 0,
    presentation_mapping: /commandMessage/.test(feedback) && /productErrorMessage/.test(library) && /scopeLabel/.test(history) && /statusLabel/.test(canon) && /productErrorMessage/.test(production) && /sanitizeProductText/.test(volume) && /versionReason/.test(reader) && /export function (?:statusLabel|productErrorMessage|receiptSummary)/.test(language),
    assistant_leakage: /productErrorMessage/.test(assistant) && /sanitizeProductText/.test(assistant) && !forbiddenVisiblePatterns.some(pattern => pattern.test(templateText(assistant))),
    dynamic_presentation_leakage: dynamicTokenCoverage && /containsInternalProductToken/.test(language),
    assistant_dynamic_leakage: /containsInternalProductToken/.test(assistantPresentation) && /safeAssistantFallback/.test(assistantPresentation),
    b12_evidence_data_separation: evidenceDataSeparation,
    evidence_selection_identity: evidenceSelectionIdentity,
    dynamic_capability_leakage: dynamicCapabilityCoverage && /containsInternalProductToken/.test(language),
    composite_evidence_identity: compositeEvidenceIdentity,
    retcon_plan_user_surface: retconPlanUserSurface,
    browser_evidence_fixtures: browserEvidenceFixtures,
    canon_user_surface: canonUserSurface,
    project_busy_recovery: projectBusyRecovery,
    withdraw_request_surface: withdrawRequestSurface,
    error_receipt_diagnostics: /receiptSummary/.test(language) && /receiptSummary\(receipt\)/.test(sourceReader('src/ui/stores/projectStore.ts')) && /productErrorMessage/.test(language) && /commandMessage/.test(feedback) && /repairMessage/.test(assistant) && /actionMessage/.test(history),
    ai_trace_lint: aiLeaks.length === 0,
    b13_health_surface: /LongFormHealth\.vue/.test(router) && /factorLabel/.test(healthPage) && /statusLabel/.test(healthPage) && /factorLabel/.test(healthLanguage) && /issueKey/.test(healthLanguage),
    future_feature_boundary: /LongFormHealth/.test(healthRoute) && !/UnavailableFeature/.test(healthRoute) && Boolean(healthPage),
    b14_completion_surface: /queryCompletionStatus/.test(completionPage) && /queryEndgameAdvice/.test(completionPage) && /checkName/.test(completionPage) && /userError/.test(completionPage),
    reader_capability_truth: /buildReaderRevisionCommands/.test(reader)
      && /name:\s*['"]history['"]/.test(reader)
      && /type:\s*['"]change_request_create['"]/.test(readerRevision)
      && /type:\s*['"]impact_preflight['"]/.test(readerRevision)
      && /store\.loadExportCapability\(projectId\)/.test(reader)
      && /store\.loadLatestExportReceipt\(projectId\)/.test(reader)
      && /store\.createExport\(requestId,\s*['"]TXT['"],\s*projectId\)/.test(reader)
      && /store\.queryExportReceipt\(requestId,\s*projectId\)/.test(reader)
      && /watch\(\(\)\s*=>\s*store\.currentProjectId/.test(reader)
      && /onUnmounted\(/.test(reader)
      && /isCurrentExport\(projectId,\s*generation\)/.test(reader)
      && /exportReceipt\?\.status\s*===\s*['"]COMPLETED['"]/.test(reader)
      && /:disabled="!exportCapability\?\.available\s*\|\|\s*exporting"/.test(reader)
      && /query_export_capability/.test(tauriPort)
      && /create_export/.test(tauriPort)
      && /query_export_receipt/.test(tauriPort)
      && !/chapter_revision_request|type:\s*['"]export_create['"]/.test(reader)
      && !/['"]chapter_revision_request['"]|['"]export_create['"]/.test(sourceReader('src/application/contracts.ts'))
      && /if\s*\(isReaderProposalOnlySource\(activeChange\.value\.source\)\)/.test(historyCenter)
      && /v-if="!readerProposalOnly"[^>]*@click="approveAndApply"/.test(historyCenter)
      && /章节修改建议仅作为意见留存/.test(historyCenter)
      && /readerProposalStatusLabel\(activeChange\.value\.status\)/.test(historyCenter)
      && /historyEntryStatusLabel\(entry\)/.test(historyCenter)
      && /historyEntryStatusClass\(entry\)/.test(historyCenter)
      && /readerHistoryStatusLabel/.test(readerRevision)
      && /source\?: string \| null/.test(contracts)
      && /targetClass\?: string \| null/.test(contracts)
      && /WITH history_rows AS/.test(historyProjection)
      && /UNION ALL/.test(historyProjection)
      && /WHERE \?2='all' OR entry_kind=\?2/.test(historyProjection)
      && /requestedChange\/source/.test(historyProjection)
      && /json_extract\(p\.payload_json,'\$\.changeRequestId'\)/.test(historyProjection)
      && /"targetClass":target_class/.test(historyProjection),
    history_pagination_integrity: /WITH history_rows AS/.test(historyProjection)
      && /UNION ALL/.test(historyProjection)
      && /WHERE \?2='all' OR entry_kind=\?2/.test(historyProjection)
      && /ORDER BY created_at DESC,entry_id DESC,entry_kind DESC/.test(historyProjection)
      && /LIMIT \?3 OFFSET \?4/.test(historyProjection)
      && /queryHistory\(\{ \.\.\.queryBase\('history', 40\), cursor: pageCursor, kind \}\)/.test(historyStore)
      && /function loadMoreHistory\(\)/.test(historyCenter)
      && /history\.entries\.nextCursor/.test(historyCenter)
      && /loadHistory\(kind\.value, false, cursor\)/.test(historyCenter)
      && /加载更多历史记录/.test(historyCenter),
    history_filter_state_integrity: /type HistoryKind = 'all' \| 'VERSION' \| 'CHANGE_REQUEST' \| 'RETCON'/.test(historyStore)
      && /const historyKind = ref<HistoryKind \| null>\(null\)/.test(historyStore)
      && /historyKind\.value === kind && !force && cursor === null/.test(historyStore)
      && /const pageCursor = cursor && historyKind\.value === kind \? cursor : null/.test(historyStore)
      && /!history\.value \|\| historyKind\.value !== kind/.test(historyStore)
      && /historyKind\.value=kind/.test(historyStore)
      && /historyKind\.value = null/.test(historyStore)
      && /history, historyKind, settings/.test(historyStore)
      && /onMounted\(\(\) => \{\s*void loadHistoryKind\('all', false\)/.test(historyCenter)
      && /setKind\(next: HistoryKind\)[\s\S]*loadHistoryKind\(next, true\)/.test(historyCenter),
    history_filter_async_recovery: /function loadHistoryKind\(next: HistoryKind, force: boolean\)/.test(historyCenter)
      && /requestSequence !== historyFilterRequestSequence/.test(historyCenter)
      && /kind\.value = restoredKind/.test(historyCenter)
      && /store\.historyKind/.test(historyCenter)
      && /failedHistoryKind\.value = next/.test(historyCenter)
      && /:disabled="historyFilterLoading"/.test(historyCenter)
      && /role="status" aria-live="polite"/.test(historyCenter)
      && /v-if="history && !historyFilterLoading"/.test(historyCenter)
      && /role="alert"/.test(historyCenter)
      && /@click="retryHistoryFilter"/.test(historyCenter)
      && /loadHistoryKind\(failedHistoryKind\.value, true\)/.test(historyCenter),
    history_filter_cold_recovery: /v-if="history \|\| historyFilterLoading \|\| historyFilterError"/.test(historyCenter)
      && /v-if="history" class="metric-grid section"/.test(historyCenter)
      && /v-if="history && !historyFilterLoading"/.test(historyCenter)
      && /v-if="history" class="section actions"/.test(historyCenter),
    history_filter_copy_truth: /const hasHistoryProjection = store\.history !== null/.test(historyCenter)
      && /hasHistoryProjection\s*\?[\s\S]*?仍显示/.test(historyCenter)
      && /hasHistoryProjection\s*\?[\s\S]*?暂时无法加载历史记录，请重试。/.test(historyCenter),
  }
  const internalCodeOnly = activeFiles.flatMap(item => {
    const source = item.source
    const template = templateText(source)
    const count = forbiddenVisiblePatterns.reduce((total, pattern) => total + (source.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`)) ?? []).length - (template.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`)) ?? []).length, 0)
    return count > 0 ? [{ file: item.relative, count }] : []
  })
  const result = {
    schema: 'post-b12-product-ux-gate-r1-r2',
    sourceCommit: sourceCommit(),
    activeUserSurface: { files: ACTIVE_USER_SURFACE, missing, visibleLeaks, status: checks.active_ui_language ? 'PASS' : 'FAIL' },
    presentationMapping: { status: checks.presentation_mapping ? 'PASS' : 'FAIL' },
    assistantLeakage: { status: checks.assistant_leakage ? 'PASS' : 'FAIL' },
    dynamicPresentationLeakage: { status: checks.dynamic_presentation_leakage ? 'PASS' : 'FAIL', verifiedBy: 'runtime presentation fixtures in verify:product-ux' },
    assistantDynamicLeakage: { status: checks.assistant_dynamic_leakage ? 'PASS' : 'FAIL', verifiedBy: 'runtime Assistant fixtures in verify:product-ux' },
    b12EvidenceDataSeparation: { status: checks.b12_evidence_data_separation ? 'PASS' : 'FAIL', verifiedBy: 'B12 flow raw-value assertions' },
    evidenceSelectionIdentity: { status: checks.evidence_selection_identity ? 'PASS' : 'FAIL', verifiedBy: 'History stable selector checks' },
    dynamicCapabilityLeakage: { status: checks.dynamic_capability_leakage ? 'PASS' : 'FAIL', verifiedBy: 'bare capability production presentation fixtures' },
    compositeEvidenceIdentity: { status: checks.composite_evidence_identity ? 'PASS' : 'FAIL', verifiedBy: 'History artifact/version/hash restoration boundary' },
    retconPlanUserSurface: { status: checks.retcon_plan_user_surface ? 'PASS' : 'FAIL', verifiedBy: 'browser demo user-visible copy scan' },
    browserEvidenceFixtures: { status: checks.browser_evidence_fixtures ? 'PASS' : 'FAIL', verifiedBy: 'explicit demo-only M11-M14 route fixtures' },
    canonUserSurface: { status: checks.canon_user_surface ? 'PASS' : 'FAIL', verifiedBy: 'user-visible summary/impact/message surface scan', detectedDisplayLeak: canonDisplayLeak },
    withdrawRequestSurface: { status: checks.withdraw_request_surface ? 'PASS' : 'FAIL', verifiedBy: 'History Center withdraw requests with product-language decisions' },
    projectBusyRecovery: { status: checks.project_busy_recovery ? 'PASS' : 'FAIL', verifiedBy: 'PROJECT_BUSY product message, retry with a new command id, no raw writer text' },
    errorReceiptDiagnostics: { status: checks.error_receipt_diagnostics ? 'PASS' : 'FAIL' },
    aiTraceLint: { status: checks.ai_trace_lint ? 'PASS' : 'FAIL', blockingMatches: aiLeaks },
    futureShellDeferred: { status: checks.future_feature_boundary ? 'PASS' : 'FAIL', deferred: ['LongFormHealth.vue'], note: 'B13 formal health workspace is active.' },
    b13HealthSurface: { status: checks.b13_health_surface ? 'PASS' : 'FAIL', route: healthRoute },
    b14CompletionSurface: { status: checks.b14_completion_surface ? 'PASS' : 'FAIL', verifiedBy: 'Completion.vue formal query and presentation mapper' },
    readerCapabilityTruth: { status: checks.reader_capability_truth ? 'PASS' : 'FAIL', verifiedBy: 'Reader Change path and backend Export capability/receipt binding' },
    historyPaginationIntegrity: { status: checks.history_pagination_integrity ? 'PASS' : 'FAIL', verifiedBy: 'single cross-type ordered page, frontend cursor request and append control' },
    historyFilterStateIntegrity: { status: checks.history_filter_state_integrity ? 'PASS' : 'FAIL', verifiedBy: 'kind-bound cache, project reset, filter refresh and append isolation' },
    historyFilterAsyncRecovery: { status: checks.history_filter_async_recovery ? 'PASS' : 'FAIL', verifiedBy: 'pending-result mask, restored filter, typed alert, retry and stale-response guard' },
    historyFilterColdRecovery: { status: checks.history_filter_cold_recovery ? 'PASS' : 'FAIL', verifiedBy: 'loading/error shell renders without history while data-only regions remain guarded' },
    historyFilterCopyTruth: { status: checks.history_filter_copy_truth ? 'PASS' : 'FAIL', verifiedBy: 'different safe recovery copy for empty cold start versus retained cached history' },
    internalCodeOnly,
    allowedProviderTerms: ['OpenAI Compatible', 'API Endpoint', 'API Key'],
    checks: Object.fromEntries(Object.entries(checks).map(([name, passed]) => [name, passed ? 'PASS' : 'FAIL'])),
  }
  result.status = Object.values(checks).every(Boolean) ? 'PASS' : 'FAIL'
  return result
}

if (process.argv[1]?.endsWith('verify-product-ux.mjs')) {
  const result = evaluateProductUx()
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}
