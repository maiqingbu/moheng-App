import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const requiredFiles = [
  'src-tauri/src/endgame.rs',
  'src-tauri/src/repositories/endgame.rs',
  'src-tauri/tests/b14_endgame.rs',
  'src-tauri/tests/b14_endgame_integration.rs',
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
}

const failures = []
for (const file of requiredFiles) if (!fs.existsSync(path.join(root, file))) failures.push({ file, reason: 'MISSING_FILE' })
for (const [file, patterns] of Object.entries(requiredPatterns)) {
  const target = path.join(root, file)
  if (!fs.existsSync(target)) continue
  const source = fs.readFileSync(target, 'utf8')
  for (const pattern of patterns) if (!pattern.test(source)) failures.push({ file, reason: `MISSING_PATTERN:${pattern}` })
}

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

