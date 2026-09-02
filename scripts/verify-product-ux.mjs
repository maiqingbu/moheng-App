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
]

const forbiddenVisiblePatterns = [
  /\bRust\b/i,
  /\bTypeScript\b|\bJavaScript\b/i,
  /\bVue\b|\bPinia\b|\bTauri\b|\bIPC\b|\bSQLite\b/i,
  /\bKeychain\b|\bWebView\b/i,
  /\b(?:projectId|artifactId|payloadHash|sourceCommit|changeRequestId|authorizationId|repairPlanId|operationDigest|expectedRevision|ownerReceiptRef)\b/i,
  /\b(?:PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED)\b/,
  /\b(?:CommitBundle|Narrative Authority|PlanningOwner|ProjectionOwner|VolumeLifecycleOwner|CommitService|RepairPlan|CandidatePatch|RepairSuggestion|RuntimeCheckRequest)\b/,
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
  const language = sourceReader('src/ui/presentation/productLanguage.ts')
  const router = sourceReader('src/app/router.ts')
  const checks = {
    active_ui_language: missing.length === 0 && visibleLeaks.length === 0,
    presentation_mapping: /commandMessage/.test(feedback) && /productErrorMessage/.test(library) && /scopeLabel/.test(history) && /statusLabel/.test(canon) && /productErrorMessage/.test(production) && /sanitizeProductText/.test(volume) && /versionReason/.test(reader) && /export function (?:statusLabel|productErrorMessage|receiptSummary)/.test(language),
    assistant_leakage: /productErrorMessage/.test(assistant) && /sanitizeProductText/.test(assistant) && !forbiddenVisiblePatterns.some(pattern => pattern.test(templateText(assistant))),
    error_receipt_diagnostics: /receiptSummary/.test(language) && /receiptSummary\(receipt\)/.test(sourceReader('src/ui/stores/projectStore.ts')) && /productErrorMessage/.test(language) && /commandMessage/.test(feedback) && /repairMessage/.test(assistant) && /actionMessage/.test(history),
    ai_trace_lint: aiLeaks.length === 0,
    future_feature_boundary: /UnavailableFeature/.test(router) && !/LongFormHealth/.test(router) && Boolean(sourceReader('src/ui/pages/LongFormHealth.vue')),
  }
  const internalCodeOnly = activeFiles.flatMap(item => {
    const source = item.source
    const template = templateText(source)
    const count = forbiddenVisiblePatterns.reduce((total, pattern) => total + (source.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`)) ?? []).length - (template.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`)) ?? []).length, 0)
    return count > 0 ? [{ file: item.relative, count }] : []
  })
  const result = {
    schema: 'post-b12-product-ux-gate-r1',
    sourceCommit: sourceCommit(),
    activeUserSurface: { files: ACTIVE_USER_SURFACE, missing, visibleLeaks, status: checks.active_ui_language ? 'PASS' : 'FAIL' },
    presentationMapping: { status: checks.presentation_mapping ? 'PASS' : 'FAIL' },
    assistantLeakage: { status: checks.assistant_leakage ? 'PASS' : 'FAIL' },
    errorReceiptDiagnostics: { status: checks.error_receipt_diagnostics ? 'PASS' : 'FAIL' },
    aiTraceLint: { status: checks.ai_trace_lint ? 'PASS' : 'FAIL', blockingMatches: aiLeaks },
    futureShellDeferred: { status: checks.future_feature_boundary ? 'PASS' : 'FAIL', deferred: ['LongFormHealth.vue'] },
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
