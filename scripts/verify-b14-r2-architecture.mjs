import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = relative => fs.readFileSync(path.join(root, relative), 'utf8')
const repository = source('src-tauri/src/repositories/endgame.rs')
const commands = source('src-tauri/src/commands.rs')
const domain = source('src-tauri/src/endgame.rs')
const completion = source('src/ui/pages/Completion.vue')
const runtime = source('src-tauri/src/repositories/narrative.rs')
const failures = []
const require = (ok, reason) => { if (!ok) failures.push(reason) }
require(/task_states_by_ids_complete/.test(repository) && /VolumePlan/.test(repository), 'TASK_REALITY_FORMAL_OWNER_MISSING')
require(!/\$\.class/.test(repository), 'TASK_REALITY_CLASS_FROM_STATE_DELTA')
require(/continuity_threads_complete/.test(repository) && /ContinuityThreadView/.test(runtime), 'CONTINUITY_FORMAL_PROJECTION_MISSING')
require(!/open_hard_continuity.*narrative_obligations/s.test(repository), 'CONTINUITY_USES_OBLIGATION_COUNT')
require(/hard_payoff_count/.test(repository) && /hardPayoffMappings/.test(repository), 'HARD_PAYOFF_MAPPING_NOT_CONSUMED')
require(/reservationMappings/.test(repository) && /fulfillment_evidence_refs_valid/.test(repository), 'RESERVATION_MAPPING_NOT_CONSUMED')
require(/value_contains_ref/.test(repository) && !/payload\.contains\(reference\)/.test(repository), 'RELEVANT_REFS_NOT_STRUCTURED')
require(/execute_production_model/.test(commands) && /query_b14_skill_runtime/.test(commands), 'S44_S45_RUNTIME_MISSING')
require(!/AUTO_MAPPING/.test(commands) && !/hardcoded S44/.test(commands), 'SKILL_AUTO_MAPPING_OR_FAKE_RUNTIME')
require(!/S45_HARD_AUTHORITY/.test(commands), 'S45_HARD_AUTHORITY')
require(!/DETAIL_LIMIT_AFFECTS_GLOBAL_TRUTH/.test(domain), 'DETAIL_LIMIT_AFFECTS_GLOBAL_TRUTH')
require(!/(EndgameContract|BookExitState|BookCompletionReport|ExitState|CompletionReport)/.test(completion.match(/<template>[\s\S]*?<\/template>/)?.[0] ?? ''), 'COMPLETION_INTERNAL_TERM_VISIBLE')
require(!/ExportRuntime|ReleaseOwner|ReleaseCertification/.test(`${repository}\n${commands}\n${domain}`), 'B15_SCOPE_LEAK')
const result = { schema:'b14-r2-architecture-verification', sourceCommit:process.env.B14_R2_SOURCE_COMMIT ?? null, failures, status:failures.length ? 'FAIL':'PASS', nextBatchAllowed:false }
console.log(JSON.stringify(result,null,2)); if(failures.length) process.exitCode=1
