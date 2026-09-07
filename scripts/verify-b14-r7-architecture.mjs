import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const failures = []
const changed = process.env.B14_R7_CHANGED_FILES ?? ''
if (/(^|\n)[AM].*\s(src\/|src-tauri\/src\/|contracts\/|migrations\/|creative-skills\/)/.test(changed)) failures.push('PRODUCTION_CODE_NOT_ZERO')
if (!/b14_r7_paged_sources_enter_continuity_and_change_digest/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('R7_E01_TEST_MISSING')
if (!/b14_r7_one_fixture_keeps_all_11_domains_unchanged_across_queries_and_completion/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('R7_E02_TEST_MISSING')
if (!/b14_r7_hard_continuity_responsibility_blocks_completion_independently/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('R7_HARD_REGRESSION_MISSING')
if (!/PAGED_SOURCE_101_MISSING_FROM_CONTINUITY/.test(read('scripts/run-b14-r7-mutations.mjs'))) failures.push('R7_MUTATION_FAILURE_CLASS_MISSING')
if (!/B14_R7/.test(read('scripts/run-b14-r7-mutations.mjs'))) failures.push('R7_MUTATION_EVIDENCE_MISSING')
if (!/d34df5cb7b4b0e93a51c966e50536d3ee3c30b14f99b3039808f4175fa974934/.test(read('scripts/verify-b14-r7-frozen-inputs.mjs'))) failures.push('R7_FROZEN_SHA_GATE_MISSING')
if (/B15|RC1/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('FUTURE_SCOPE_LEAK')
console.log(JSON.stringify({ schema: 'b14-r7-architecture-verification', failures, status: failures.length ? 'FAIL' : 'PASS', nextBatchAllowed: false }, null, 2))
if (failures.length) process.exitCode = 1
