import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');const read=f=>fs.readFileSync(path.join(root,f),'utf8');const failures=[]
const changed=process.env.B14_R6_CHANGED_FILES??''
if(/(^|\n)[AM].*\s(src\/|src-tauri\/src\/|contracts\/|migrations\/|creative-skills\/)/.test(changed)) failures.push('PRODUCTION_CODE_NOT_ZERO')
if(!/B14_R6_CONTINUITY_PAGED_BOUNDARY/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('R6_E01_TEST_MISSING')
if(!/B14_R6_CONTINUITY_DOMAIN/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('R6_E02_CONTINUITY_TEST_MISSING')
if(!/MUT-R6-01/.test(read('scripts/run-b14-r6-mutations.mjs'))||!/MUT-R6-02/.test(read('scripts/run-b14-r6-mutations.mjs'))) failures.push('R6_MUTATION_SET_MISSING')
if(!/FROZEN_INPUT_SHA_MISMATCH/.test(read('scripts/verify-b14-r6-frozen-inputs.mjs'))) failures.push('R6_FROZEN_SHA_GATE_MISSING')
if(!/generateB14R6Acceptance/.test(read('scripts/generate-b14-r6-acceptance.mjs'))) failures.push('R6_ACCEPTANCE_MISSING')
if(/B15|RC1/.test(read('src-tauri/tests/b14_endgame_integration.rs'))) failures.push('FUTURE_SCOPE_LEAK')
console.log(JSON.stringify({schema:'b14-r6-architecture-verification',failures,status:failures.length?'FAIL':'PASS',nextBatchAllowed:false},null,2));if(failures.length)process.exitCode=1
