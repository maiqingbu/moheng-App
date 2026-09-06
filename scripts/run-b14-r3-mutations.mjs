import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R3_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(),'moheng-b14-r3-mutations-')))
const mutationDir = path.join(evidenceDir,'B14_R3','MUTATIONS'); fs.mkdirSync(mutationDir,{recursive:true})
const specs = [
  ['MUT-R3-01','src-tauri/src/repositories/endgame.rs','Self::responsibility_refs_satisfied(\n                connection,\n                project_id,\n                mapping.and_then(|item| item.get("mappedResponsibilityRefs")),','Self::mapping_refs_valid(\n                connection,\n                project_id,\n                mapping.and_then(|item| item.get("mappedResponsibilityRefs")),','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_mystery_mapping_requires_current_responsibility_disposition']],
  ['MUT-R3-02','src-tauri/src/repositories/endgame.rs',"Self::fulfillment_evidence_refs_valid(\n                connection,\n                project_id,\n                mapping.and_then(|item| item.get(\"fulfillmentEvidenceRefs\")),", "Self::mapping_refs_valid(\n                connection,\n                project_id,\n                mapping.and_then(|item| item.get(\"fulfillmentEvidenceRefs\")),",'cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_unrelated_final_chapter_cannot_fulfill_reservation_or_payoff']],
  ['MUT-R3-03','src-tauri/src/repositories/endgame.rs','Self::fulfillment_evidence_refs_valid(\n                connection,\n                project_id,\n                mapping.get("fulfillmentEvidenceRefs"),','Self::mapping_refs_valid(\n                connection,\n                project_id,\n                mapping.get("fulfillmentEvidenceRefs"),','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_unrelated_final_chapter_cannot_fulfill_reservation_or_payoff']],
  ['MUT-R3-04','src-tauri/src/repositories/narrative.rs','&foreshadows,\n            &mysteries,\n            &evidence,','&[],\n            &[],\n            &[],','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_continuity_projection_includes_foreshadow_mystery_and_evidence_sources']],
  ['MUT-R3-05','src-tauri/src/commands.rs','instance_responsibilities','mapping_workflow','node',['scripts/verify-b14-r3-architecture.mjs']],
  ['MUT-R3-06','src-tauri/src/commands.rs','loader.load_selected(&selection, &required_resources)?','loader.load_selected(&selection, &[])?','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_s44_s45_run_through_model_runtime_and_stay_soft_only']],
  ['MUT-R3-07','src-tauri/src/repositories/endgame.rs','let blocked_required_tasks = Self::blocked_hard_tasks(connection, project_id)?;','let blocked_required_tasks = 0;','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_task_truth_uses_volume_plan_class_and_current_task_progress']],
  ['MUT-R3-08','src-tauri/src/endgame.rs','pub struct CompletionInput','pub struct ExportRuntime {}\npub struct CompletionInput','node',['scripts/verify-b14-r3-architecture.mjs']],
]
function run(command,args){ const value=spawnSync(command,args,{cwd:root,encoding:'utf8',maxBuffer:64*1024*1024});return {exitCode:value.status??1,output:`${value.stdout??''}${value.stderr??''}`} }
const results=[]
for(const [id,file,needle,replacement,command,args] of specs){
 const absolute=path.join(root,file), original=fs.readFileSync(absolute,'utf8'), folder=path.join(mutationDir,id);fs.mkdirSync(folder,{recursive:true});
 if(!original.includes(needle)) throw new Error(`B14_R3_MUTATION_TARGET_MISSING:${id}`)
 let introduced; try { const mutated=original.replace(needle,replacement);fs.writeFileSync(absolute,mutated);fs.writeFileSync(path.join(folder,'patch.diff'),`${file}: ${needle} -> ${replacement}\n`);introduced=run(command,args);fs.writeFileSync(path.join(folder,'introduced.log'),introduced.output) } finally { fs.writeFileSync(absolute,original) }
 const reverted=run(command,args);fs.writeFileSync(path.join(folder,'reverted.log'),reverted.output)
 const result={id,file,introducedExitCode:introduced.exitCode,revertedExitCode:reverted.exitCode,status:introduced.exitCode!==0&&reverted.exitCode===0?'PASS':'FAIL',reverted:true};fs.writeFileSync(path.join(folder,'result.json'),`${JSON.stringify(result,null,2)}\n`);results.push(result)
}
const result={schema:'b14-r3-mutation-gates',sourceCommit:spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).stdout.trim(),mutations:results,status:results.every(item=>item.status==='PASS')?'PASS':'FAIL'};fs.writeFileSync(path.join(evidenceDir,'B14_R3_MUTATION_GATES.json'),`${JSON.stringify(result,null,2)}\n`);console.log(JSON.stringify(result,null,2));if(result.status!=='PASS')process.exitCode=1
