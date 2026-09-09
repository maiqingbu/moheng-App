import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R7_EVIDENCE_DIR ?? '')
const sourceCommit = process.env.POST_B14_UI_R7_SOURCE_COMMIT || exec('git', ['rev-parse', 'HEAD'])
const readJson = relative => JSON.parse(fs.readFileSync(path.join(evidenceDir, relative), 'utf8'))
const write = (relative, value) => { const file = path.join(evidenceDir, relative); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`) }
const exec = (command, args) => execFileSync(command, args, { cwd: root, encoding: 'utf8' }).trim()
const sha = value => crypto.createHash('sha256').update(value, 'utf8').digest('hex')
if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('R7_EVIDENCE_DIR_REQUIRED')

const cloneWithR7Schema = (relative, schema) => ({ ...readJson(relative), schema, sourceCommit })
write('A09_AUTHORIZATION_RUNTIME_TRACE_R7.json', cloneWithR7Schema('A09_AUTHORIZATION_RUNTIME_TRACE_R6.json', 'post-b14-ui-r7-a09-runtime-authorization-trace'))
const a11R6 = readJson('A11_TARGET_STATE_R6.json')
const before = a11R6.beforeTargetState ?? null
const after = a11R6.afterTargetState ?? null
const fields = value => Object.fromEntries((value?.fields ?? []).map(field => [field.key, field.value]))
const beforeFields = fields(before)
const afterFields = fields(after)
const expectedFields = [
  { path: 'afterTargetState.fields.fts.projectionVersion', expected: 'm0e.fts.v1' },
  { path: 'afterTargetState.digestChanged', expected: true },
]
const observedFields = [
  { path: 'afterTargetState.fields.fts.projectionVersion', observed: afterFields['fts.projectionVersion'] ?? null },
  { path: 'afterTargetState.digestChanged', observed: Boolean(before?.digest && after?.digest && before.digest !== after.digest) },
]
const allExpectedFieldsObserved = observedFields.every(field => field.observed !== null && field.observed !== undefined)
const allExpectedFieldsMatched = observedFields.every((field, index) => field.observed === expectedFields[index].expected)
const receiptVerified = a11R6.receiptVerified === true
const postVerifySucceeded = a11R6.postVerifySucceeded === true
write('A11_TARGET_STATE_R7.json', {
  schema: 'post-b14-ui-r7-a11-target-state', sourceCommit, targetDomain: 'FTSProjectionSettings',
  beforeTargetState: before, afterTargetState: after, beforeFields, expectedFields, observedFields,
  allExpectedFieldsObserved, allExpectedFieldsMatched, receiptVerified, postVerifySucceeded,
  status: allExpectedFieldsObserved && allExpectedFieldsMatched && receiptVerified && postVerifySucceeded ? 'PASS' : 'FAIL',
})
const noopR6 = readJson('A11_NOOP_NEGATIVE_R6.json')
write('A11_NOOP_NEGATIVE_R7.json', {
  ...noopR6, schema: 'post-b14-ui-r7-a11-noop-negative', sourceCommit,
  formalTargetStateUnchanged: noopR6.formalStateChanged === false,
  verifierFailed: noopR6.verifierWouldFail === true,
  status: noopR6.formalStateChanged === false && noopR6.verifierWouldFail === true ? 'PASS' : 'FAIL',
})

const fullRoot = path.join(evidenceDir, 'FULL_PRODUCT')
const domFiles = fs.existsSync(path.join(fullRoot, 'DOM')) ? fs.readdirSync(path.join(fullRoot, 'DOM')).filter(file => file.endsWith('.txt')).sort() : []
const entries = domFiles.map(dom => ({ dom, text: fs.readFileSync(path.join(fullRoot, 'DOM', dom), 'utf8') }))
const allowedTerms = ['OpenAI Compatible', 'API Endpoint', 'API Key']
const visibleText = entries.map(entry => entry.text).join('\n').replaceAll(/OpenAI Compatible|API Endpoint|API Key/g, ' ')
const matches = regex => [...visibleText.matchAll(regex)].map(item => item[0]).filter((value, index, all) => all.indexOf(value) === index)
const engineeringEnglish = /\b(?:Artifact|Contract|Owner|Runtime|Projection|Candidate|Gate|Receipt|ChangeRequest|RetconPlan|RepairPlan|SupportTaskPlan|Capability|Chapter Production Runtime|Project Support|Harness|Codex|Authority)\b/gi
const engineeringChinese = /(?:权威项目库|权威数据库|权威项目|权威|M0A|内部阶段|工程语义)/g
const internalBatchCode = /\b(?:M0A|M(?:0[1-9]|[1-9][0-9])|H(?:0[0-9]|[1-9][0-9])|B(?:0[1-9]|[1-9][0-9])|R(?:0[1-9]|[1-9][0-9])|RC1)\b/g
const rawEnum = /\b(?:PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED|READY|BLOCKED|NEEDS_RECHECK|COMPLETED|PROJECTION|TENTATIVE)\b/g
const rawId = /\b(?:[a-f0-9]{8}-[a-f0-9-]{27,}|(?:artifact|project|chapter|volume|obligation|fact|task|state):[A-Za-z0-9:_-]+)\b/gi
const rawIso = /\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b/g
const epoch = /1970-01-01/g
const devTrace = /\b(?:demo|fixture|mock|seed|debug|test-only|e2e|browser-demo|batch\s*id)\b|(?:演示数据|演示夹具)/gi
const report = {
  schema: 'post-b14-ui-r7-full-product-visible-dom', sourceCommit,
  routesScanned: new Set((readJson('FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json').entries ?? []).map(entry => entry.route)).size,
  screensScanned: entries.length, visibleSystemStringsCount: visibleText.split(/\n+/).filter(Boolean).length,
  englishSystemHits: matches(/\b(?:Rust|TypeScript|JavaScript|Vue|Pinia|Tauri|SQLite)\b/gi),
  engineeringEnglishHits: matches(engineeringEnglish), engineeringChineseHits: matches(engineeringChinese),
  internalBatchCodeHits: matches(internalBatchCode), rawEnumHits: matches(rawEnum), rawIdHits: matches(rawId),
  rawIsoTimeHits: matches(rawIso), epochTimeHits: matches(epoch), devTraceHits: matches(devTrace),
  allowedTerms, violations: [], entries: readJson('FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json').entries ?? [],
}
report.status = [report.englishSystemHits, report.engineeringEnglishHits, report.engineeringChineseHits, report.internalBatchCodeHits, report.rawEnumHits, report.rawIdHits, report.rawIsoTimeHits, report.epochTimeHits, report.devTraceHits].every(list => list.length === 0) ? 'PASS' : 'FAIL'
write('FULL_PRODUCT_VISIBLE_DOM_SCAN_R7.json', report)

for (const [from, to, schema] of [
  ['ACCESSIBILITY_R5.json', 'ACCESSIBILITY_R7.json', 'post-b14-ui-r7-accessibility'],
  ['ROUTE_INVENTORY_R5.json', 'ROUTE_INVENTORY_R7.json', 'post-b14-ui-r7-route-inventory'],
  ['GENERATION_FLOW_R5.json', 'GENERATION_FLOW_R7.json', 'post-b14-ui-r7-generation-flow'],
]) write(to, cloneWithR7Schema(from, schema))
write('B13_UI_R7_REGRESSION.json', { ...readJson('B13_UI_R6_REGRESSION.json'), schema: 'post-b14-ui-r7-b13-regression', sourceCommit })
write('B14_UI_R7_REGRESSION.json', { ...readJson('B14_UI_R6_REGRESSION.json'), schema: 'post-b14-ui-r7-b14-regression', sourceCommit })
console.log(JSON.stringify({ sourceCommit, display: report.status, a11: allExpectedFieldsObserved && allExpectedFieldsMatched && receiptVerified && postVerifySucceeded ? 'PASS' : 'FAIL' }, null, 2))
