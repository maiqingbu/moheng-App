import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R2_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(),'moheng-b14-r2-mutations-')))
const mutationDir = path.join(evidenceDir,'B14_R2','MUTATIONS'); fs.mkdirSync(mutationDir,{recursive:true})
const specs = [
  ['MUT-R2-01','src-tauri/src/repositories/endgame.rs','task_states_by_ids_complete','task_states_by_ids_complete /* $.class */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-02','src-tauri/src/repositories/endgame.rs','continuity_threads_complete','continuity_threads_complete /* open_hard_continuity narrative_obligations */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-03','src-tauri/src/commands.rs','query_b14_skill_runtime','query_b14_skill_runtime /* AUTO_MAPPING */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-04','src-tauri/src/commands.rs','execute_production_model','execute_production_model /* hardcoded S44 */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-05','src-tauri/src/commands.rs','query_completion_quality_impl','query_completion_quality_impl /* S45_HARD_AUTHORITY CompletionStatus::Blocked */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-06','src/ui/pages/Completion.vue','title="终局与完结"','title="CompletionReport"','node',['scripts/verify-product-ux.mjs']],
  ['MUT-R2-07','src-tauri/src/endgame.rs','detail_limit','detail_limit /* DETAIL_LIMIT_AFFECTS_GLOBAL_TRUTH */','node',['scripts/verify-b14-r2-architecture.mjs']],
  ['MUT-R2-08','src-tauri/src/endgame.rs','pub struct CompletionInput','pub struct ExportRuntime {}\npub struct CompletionInput','node',['scripts/verify-b14-r2-architecture.mjs']],
]
function run(command,args){ const value=spawnSync(command,args,{cwd:root,encoding:'utf8',maxBuffer:64*1024*1024});return {exitCode:value.status??1,output:`${value.stdout??''}${value.stderr??''}`} }
const results=[]
for(const [id,file,needle,replacement,command,args] of specs){
 const absolute=path.join(root,file), original=fs.readFileSync(absolute,'utf8'), folder=path.join(mutationDir,id);fs.mkdirSync(folder,{recursive:true});
 if(!original.includes(needle)) throw new Error(`B14_R2_MUTATION_TARGET_MISSING:${id}`)
 let introduced; try { const mutated=original.replace(needle,replacement);fs.writeFileSync(absolute,mutated);fs.writeFileSync(path.join(folder,'patch.diff'),`${file}: ${needle} -> ${replacement}\n`);introduced=run(command,args);fs.writeFileSync(path.join(folder,'introduced.log'),introduced.output) } finally { fs.writeFileSync(absolute,original) }
 const reverted=run(command,args);fs.writeFileSync(path.join(folder,'reverted.log'),reverted.output)
 const result={id,file,introducedExitCode:introduced.exitCode,revertedExitCode:reverted.exitCode,status:introduced.exitCode!==0&&reverted.exitCode===0?'PASS':'FAIL',reverted:true};fs.writeFileSync(path.join(folder,'result.json'),`${JSON.stringify(result,null,2)}\n`);results.push(result)
}
const result={schema:'b14-r2-mutation-gates',sourceCommit:spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).stdout.trim(),mutations:results,status:results.every(item=>item.status==='PASS')?'PASS':'FAIL'};fs.writeFileSync(path.join(evidenceDir,'B14_R2_MUTATION_GATES.json'),`${JSON.stringify(result,null,2)}\n`);console.log(JSON.stringify(result,null,2));if(result.status!=='PASS')process.exitCode=1
