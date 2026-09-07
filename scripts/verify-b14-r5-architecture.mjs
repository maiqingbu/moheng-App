import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const completion = read('src/ui/pages/Completion.vue')
const contracts = read('src/application/contracts.ts')
const mapping = read('src/application/endgameMapping.ts')
const commands = read('src-tauri/src/commands.rs')
const endgame = read('src-tauri/src/repositories/endgame.rs')
const scale = read('src-tauri/tests/b14_r2_scale.rs')
const failures = []
const require = (value, reason) => { if (!value) failures.push(reason) }
require(/reservationCandidates/.test(contracts) && /hardPayoffCandidates/.test(contracts), 'SCOPED_MAPPING_CONTRACT_MISSING')
require(/scopedMappingCandidates/.test(mapping) && !/fulfillmentCandidates/.test(mapping), 'SCOPED_MAPPING_HELPER_MISSING')
require(/scopedMappingCandidates\(mappingWorkflow\.value, 'reservation'/.test(completion) && /scopedMappingCandidates\(mappingWorkflow\.value, 'payoff'/.test(completion), 'COMPLETION_UI_SCOPED_BINDING_MISSING')
require(!/mappingWorkflow\.fulfillmentCandidates/.test(completion), 'COMPLETION_UI_GLOBAL_FALLBACK')
require(/current_hard_task_ids/.test(commands) && /current_endgame_hard_task_reality/.test(commands), 'TASK_EMPTY_SCOPE_MISSING')
require(/current_endgame_continuity/.test(commands) && /findingCount/.test(commands), 'THREAD_EMPTY_SCOPE_MISSING')
require(/B14_R5_NO_AUTHORITY_REAL/.test(read('src-tauri/tests/b14_endgame_integration.rs')), 'NO_AUTHORITY_REAL_FIXTURE_MISSING')
require(/B14_R5_CONTINUITY_REAL/.test(read('src-tauri/tests/b14_endgame_integration.rs')), 'CONTINUITY_REAL_FIXTURE_MISSING')
require(/rowCount=/.test(scale) && /rows/.test(scale), 'QUERY_PLAN_ALL_ROWS_MISSING')
require(/B14_R5_FROZEN_INPUTS_CHECK\.json/.test(read('scripts/generate-b14-r5-acceptance.mjs')), 'R5_ACCEPTANCE_MISSING')
require(!/ExportRuntime|ReleaseOwner|ReleaseCertification|\bB15\b|\bRC1\b/.test(`${completion}\n${contracts}\n${commands}\n${endgame}`), 'FUTURE_SCOPE_LEAK')
console.log(JSON.stringify({ schema: 'b14-r5-architecture-verification', sourceCommit: process.env.B14_R5_SOURCE_COMMIT ?? null, failures, status: failures.length ? 'FAIL' : 'PASS', nextBatchAllowed: false }, null, 2))
if (failures.length) process.exitCode = 1
