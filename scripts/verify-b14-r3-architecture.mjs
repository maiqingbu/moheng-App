import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')
const repository = read('src-tauri/src/repositories/endgame.rs')
const narrative = read('src-tauri/src/repositories/narrative.rs')
const commands = read('src-tauri/src/commands.rs')
const skills = read('src-tauri/src/creative_skills.rs')
const completion = read('src/ui/pages/Completion.vue')
const failures = []
const require = (value, reason) => { if (!value) failures.push(reason) }
require(/responsibility_satisfied/.test(repository) && /fulfillment_evidence_refs_valid/.test(repository), 'RESPONSIBILITY_DISPOSITION_RELATION_MISSING')
require(/continuity_threads_complete/.test(narrative) && /active_foreshadows/.test(narrative) && /open_mysteries/.test(narrative) && /recent_evidence/.test(narrative), 'FULL_CONTINUITY_SOURCE_MISSING')
require(/instance_responsibilities/.test(repository) && /instance_responsibilities/.test(commands) && !/"narrative_obligations"/.test(commands), 'GENERIC_RESPONSIBILITY_REF_REMAINS')
require(/SkillBundleLoader::tracked_package|SkillBundleLoader::tracked_package/.test(commands) || /tracked_package/.test(skills), 'SKILL_BUNDLE_LOADER_MISSING')
require(/load_selected/.test(commands) && /build_method_guidance/.test(commands) && /build_skill_task_context/.test(commands), 'SKILL_GUIDANCE_CONTEXT_MISSING')
require(/loadedSkillRefs/.test(commands) && /resourceDigests/.test(commands) && /guidanceDigest/.test(commands), 'SKILL_PROVENANCE_MISSING')
require(!/(EndgameContract|BookExitState|BookCompletionReport|ExitState|CompletionReport)/.test(completion.match(/<template>[\s\S]*?<\/template>/)?.[0] ?? ''), 'PRODUCT_INTERNAL_TERMS_VISIBLE')
require(!/ExportRuntime|ReleaseOwner|ReleaseCertification/.test(`${repository}\n${narrative}\n${commands}`), 'B15_SCOPE_LEAK')
const endgame = read('src-tauri/src/endgame.rs')
require(!/ExportRuntime|ReleaseOwner|ReleaseCertification/.test(`${repository}\n${narrative}\n${commands}\n${endgame}`), 'B15_SCOPE_LEAK')
const result = { schema:'b14-r3-architecture-verification', sourceCommit:process.env.B14_R3_SOURCE_COMMIT ?? null, failures, status:failures.length?'FAIL':'PASS', nextBatchAllowed:false }
console.log(JSON.stringify(result,null,2)); if (failures.length) process.exitCode=1
