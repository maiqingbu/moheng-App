import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const narrative = read('src-tauri/src/repositories/narrative.rs')
const endgame = read('src-tauri/src/repositories/endgame.rs')
const commands = read('src-tauri/src/commands.rs')
const skills = read('src-tauri/src/creative_skills.rs')
const acceptance = read('scripts/generate-b14-r4-acceptance.mjs')
const failures = []
const require = (value, reason) => { if (!value) failures.push(reason) }
require(/active_foreshadows_complete/.test(narrative) && /open_mysteries_complete/.test(narrative) && /recent_evidence_complete/.test(narrative), 'COMPLETE_CONTINUITY_READERS_MISSING')
require(/CONTINUITY_SOURCE_CHUNK_SIZE/.test(narrative) && /LIMIT \?2 OFFSET \?3/.test(narrative), 'CONTINUITY_CHUNK_BOUNDARY_MISSING')
require(/related_fulfillment_candidates/.test(endgame) && /related_fulfillment_evidence_exists/.test(endgame), 'MAPPING_RELATION_OWNER_MISSING')
require(/reservationCandidates/.test(endgame) && /hardPayoffCandidates/.test(endgame), 'MAPPING_SCOPED_PROJECTION_MISSING')
require(/runtimeCheckRequests/.test(commands) && /run_b14_runtime_rechecks/.test(commands) && /runtimeCheckResults/.test(commands), 'RUNTIME_CHECK_HANDOFF_MISSING')
require(/runtime_check_kind_allowed/.test(skills), 'RUNTIME_CHECK_ALLOWLIST_MISSING')
require(/B14_R4_FROZEN_INPUTS_CHECK\.json/.test(acceptance), 'FROZEN_INPUT_ACCEPTANCE_GATE_MISSING')
require(!/ExportRuntime|ReleaseOwner|ReleaseCertification/.test(`${narrative}\n${endgame}\n${commands}`), 'B15_SCOPE_LEAK')
console.log(JSON.stringify({schema:'b14-r4-architecture-verification',sourceCommit:process.env.B14_R4_SOURCE_COMMIT??null,failures,status:failures.length?'FAIL':'PASS',nextBatchAllowed:false},null,2))
if (failures.length) process.exitCode=1
