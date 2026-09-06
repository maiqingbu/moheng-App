import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const requiredFiles = [
  'src-tauri/src/endgame.rs',
  'src-tauri/src/repositories/endgame.rs',
  'src-tauri/tests/b14_endgame.rs',
  'src-tauri/tests/b14_endgame_integration.rs',
  'src-tauri/tests/b14_r1_scale.rs',
  'contracts/schemas/narrative/endgame-contract-r1.schema.json',
  'contracts/schemas/narrative/book-exit-state-r1.schema.json',
  'contracts/schemas/narrative/book-completion-report-r1.schema.json',
  'src/ui/pages/Completion.vue',
  'src/app/router.ts',
]
const requiredPatterns = {
  'src-tauri/src/endgame.rs': [/OBLIGATION/, /MYSTERY/, /PAYOFF/, /REQUESTED_ENDING/, /CHANGE_STALE/, /EXPORT_INTEGRITY/],
  'src-tauri/src/commands.rs': [/endgame_contract_create/, /complete_book/, /CommitService::commit/],
  'src-tauri/src/repositories/endgame.rs': [/narrative_obligations/, /stale_queue/, /canonical_heads/, /artifact_payloads/],
  'src/ui/pages/Completion.vue': [/USER_CONFIRMED/, /complete_book/, /queryCompletionStatus/],
  'src/app/router.ts': [/\/projects\/:id\/completion/],
  'src-tauri/src/commands.rs': [/query_endgame_advice/, /query_completion_quality/],
}

const failures = []
for (const file of requiredFiles) if (!fs.existsSync(path.join(root, file))) failures.push({ file, reason: 'MISSING_FILE' })
for (const [file, patterns] of Object.entries(requiredPatterns)) {
  const target = path.join(root, file)
  if (!fs.existsSync(target)) continue
  const source = fs.readFileSync(target, 'utf8')
  for (const pattern of patterns) if (!pattern.test(source)) failures.push({ file, reason: `MISSING_PATTERN:${pattern}` })
}
const repositorySource = fs.readFileSync(path.join(root, 'src-tauri/src/repositories/endgame.rs'), 'utf8')
const commandSource = fs.readFileSync(path.join(root, 'src-tauri/src/commands.rs'), 'utf8')
const rustSource = fs.readdirSync(path.join(root, 'src-tauri/src')).filter(file => file.endsWith('.rs')).map(file => fs.readFileSync(path.join(root, 'src-tauri/src', file), 'utf8')).join('\n')
const forbiddenPatterns = [
  ['schema mappingRefs dependency', /mappingRefs/],
  ['FutureReservation status dependency', /authorityLevel.*status|status.*authorityLevel/],
  ['CharacterArc continuity authority', /character_arc_states/],
  ['project revision freshness equality', /compiledFromProjectRevision[\s\S]{0,240}== Some\(revision/],
  ['permanent completion lock', /completion_already_committed/],
]
for (const [reason, pattern] of forbiddenPatterns) {
  if (pattern.test(repositorySource) || pattern.test(commandSource)) failures.push({ file: 'B14-R1 source boundary', reason })
}
if (!/completion_history/.test(repositorySource)) failures.push({ file: 'src-tauri/src/repositories/endgame.rs', reason: 'MISSING_DIGEST_BOUND_COMPLETION_HISTORY' })
if (/\b(?:ExportRuntime|ReleaseOwner|ReleaseCertification)\b/.test(rustSource)) failures.push({ file: 'src-tauri/src', reason: 'B15_EXPORT_RUNTIME_SCOPE_LEAK' })

const result = {
  schema: 'b14-architecture-verification',
  batch: 'B14',
  owner: 'B14::EndgameCompletionOwner',
  requiredFiles,
  failures,
  status: failures.length === 0 ? 'PASS' : 'FAIL',
  nextBatchAllowed: false,
}
console.log(JSON.stringify(result, null, 2))
if (failures.length) process.exitCode = 1
