import fs from 'node:fs'
import path from 'node:path'

const evidenceDir = path.resolve(process.env.MOHENG_R3_EVIDENCE_DIR ?? process.argv[2] ?? '')
const required = [
  ['normalProductUiFullPositive', value => value === 'PASS'],
  ['openingChaptersAllFromNormalUi', value => value === true],
  ['longFormChapterFromNormalUi', value => value === true],
  ['realProviderPositive', value => value === 'PASS'],
  ['planningPressureRuntime', value => value === 'PASS'],
  ['threadPromotionThreeCases', value => value === 'PASS'],
  ['F1', value => value === 'PASS'],
  ['F2', value => value === 'PASS'],
  ['F3', value => value === 'PASS'],
  ['F4', value => value === 'PASS'],
  ['providerFailure', value => value === 'PASS'],
  ['longFormContextManifestCount', value => Number(value) >= 2],
  ['realityT1NextPlan', value => value === 'PASS'],
  ['traceValidation', value => value === 'PASS'],
  ['sourceConsistency', value => value === 'PASS'],
  ['projectConsistency', value => value === 'PASS'],
  ['secretScan', value => value === 'PASS'],
]

function readJson(file) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return null }
}

if (!evidenceDir) {
  console.error('R3_PREFLIGHT_EVIDENCE_DIR_REQUIRED')
  process.exit(1)
}

const summary = readJson(path.join(evidenceDir, 'R3_GATE_SUMMARY.json'))
const failures = []
if (!summary) failures.push('R3_GATE_SUMMARY.json missing')
for (const [field, predicate] of required) {
  const value = summary?.[field]
  if (!predicate(value)) failures.push(`${field}=${JSON.stringify(value)}`)
}
for (const forbidden of ['NOT_RUN', 'NOT_PROVEN', 'PASS_WITH_SCOPE_LIMIT', 'SUPPORTING_ONLY', 'FAIL', 'REWORK_REQUIRED', 'EMPTY']) {
  if (JSON.stringify(summary ?? {}).includes(forbidden)) failures.push(`forbidden-status:${forbidden}`)
}

const result = {
  gate: 'REAL_LLM_PRODUCT_CLOSED_LOOP',
  sourceCommit: summary?.sourceCommit ?? null,
  evidenceDir,
  failures: [...new Set(failures)],
  status: failures.length === 0 ? 'PASS' : 'FAIL',
}
fs.writeFileSync(path.join(evidenceDir, 'R3_PREFLIGHT.json'), `${JSON.stringify(result, null, 2)}\n`)
if (failures.length) {
  console.error(JSON.stringify(result, null, 2))
  process.exit(1)
}
console.log(JSON.stringify(result, null, 2))
