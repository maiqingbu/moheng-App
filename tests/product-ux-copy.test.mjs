import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')
const activePages = [
  'src/ui/components/CommandFeedback.vue',
  'src/ui/pages/CanonGraph.vue',
  'src/ui/pages/HistoryCenter.vue',
  'src/ui/pages/ManuscriptReader.vue',
  'src/ui/pages/PitchReview.vue',
  'src/ui/pages/ProductionCenter.vue',
  'src/ui/pages/ProjectAssistant.vue',
  'src/ui/pages/ProjectLibrary.vue',
  'src/ui/pages/ProjectOverview.vue',
  'src/ui/pages/SettingsPage.vue',
  'src/ui/pages/VolumeWorkspace.vue',
]
const templateText = source => source.match(/<template>([\s\S]*?)<\/template>/)?.[1]
  .replace(/\{\{[\s\S]*?\}\}/g, ' ')
  .replace(/<[^>]+>/g, ' ')
  .replace(/\s+/g, ' ')
  .trim() ?? ''

test('active product UI has no unapproved engineering language or AI template phrases', () => {
  const forbidden = [
    /\bRust\b/i, /\bIPC\b/i, /\bPinia\b/i, /\bKeychain\b/i, /\bWebView\b/i,
    /projectId/i, /payloadHash/i, /changeRequestId/i, /operationDigest/i,
    /PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED/,
    /根据架构要求|正式实现必须|不常驻页面内存|AI 推荐理由/,
  ]
  for (const relative of activePages) {
    const text = templateText(read(relative))
    for (const pattern of forbidden) assert.doesNotMatch(text, pattern, `${relative} leaked ${pattern}`)
  }
})

test('active product UI maps dynamic status and error values before display', () => {
  const feedback = read('src/ui/components/CommandFeedback.vue')
  const canon = read('src/ui/pages/CanonGraph.vue')
  const history = read('src/ui/pages/HistoryCenter.vue')
  const library = read('src/ui/pages/ProjectLibrary.vue')
  const assistant = read('src/ui/pages/ProjectAssistant.vue')
  assert.match(feedback, /commandMessage|productErrorMessage/)
  assert.match(canon, /statusLabel\(/)
  assert.match(history, /scopeLabel\(|changeStatusLabel\(|productErrorMessage/)
  assert.match(library, /productErrorMessage/)
  assert.match(assistant, /productErrorMessage|sanitizeAssistantText/)
  assert.doesNotMatch(history, /\{\{\s*entry\.scope\s*\}\}/)
  assert.doesNotMatch(history, /\{\{\s*entry\.impact\s*\}\}/)
  assert.doesNotMatch(library, /\{\{\s*store\.(?:loadError)|\{\{\s*item\.readError\s*\}\}/)
})

test('future health shell stays explicitly deferred instead of becoming an active UX gate', () => {
  const router = read('src/app/router.ts')
  const health = read('src/ui/pages/LongFormHealth.vue')
  assert.match(router, /UnavailableFeature/)
  assert.match(health, /义务 ID/)
})
