import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R4_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r4-mutations-')))
const mutationDir = path.join(evidenceDir, 'B14_R4', 'MUTATIONS')
fs.mkdirSync(mutationDir, { recursive: true })
const specs = [
  ['MUT-R4-01','src-tauri/src/repositories/narrative.rs','Self::active_foreshadows_complete(connection, project_id)?','Self::active_foreshadows(connection, project_id, &[], &[], 100)?','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_r4_continuity_complete_reads_501_rows_and_exposes_hard_thread']],
  ['MUT-R4-02','src-tauri/src/repositories/endgame.rs',"AND (json_extract(p.payload_json, '$.responsibilityRef')=?2 OR json_extract(p.payload_json, '$.payoffId')=?2 OR json_extract(p.payload_json, '$.reservationId')=?2 OR EXISTS (SELECT 1 FROM dependency_edges d WHERE d.project_id=a.project_id AND ((d.source_ref=?2 AND d.target_ref=a.artifact_id) OR (d.target_ref=?2 AND d.source_ref=a.artifact_id))))","AND 1=1",'cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_r4_mapping_candidates_are_relation_scoped_before_ui']],
  ['MUT-R4-03','src-tauri/src/commands.rs','"runtimeCheckRequests":runtime_check_requests','"runtimeCheckRequests":[]','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_s44_s45_run_through_model_runtime_and_stay_soft_only']],
  ['MUT-R4-04','src-tauri/tests/b14_endgame_integration.rs','assert_eq!(foreshadows.len(), 501);','assert_eq!(foreshadows.len(), 0);','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration','b14_r4_continuity_complete_reads_501_rows_and_exposes_hard_thread']],
  ['MUT-R4-05','scripts/generate-b14-r4-acceptance.mjs',"'B14_R4_FROZEN_INPUTS_CHECK.json',", "'B14_R4_MISSING_FROZEN_INPUT.json',",'node',['scripts/verify-b14-r4-architecture.mjs']],
]
function run(command,args){const r=spawnSync(command,args,{cwd:root,encoding:'utf8',maxBuffer:128*1024*1024});return {exitCode:r.status??1,output:`${r.stdout??''}${r.stderr??''}`}}
const results=[]
for(const [id,file,needle,replacement,command,args] of specs){
  const absolute=path.join(root,file);const original=fs.readFileSync(absolute,'utf8');const folder=path.join(mutationDir,id);fs.mkdirSync(folder,{recursive:true});
  if(!original.includes(needle)) throw new Error(`B14_R4_MUTATION_TARGET_MISSING:${id}`)
  let introduced
  try { fs.writeFileSync(absolute,original.replace(needle,replacement));fs.writeFileSync(path.join(folder,'patch.diff'),`${file}: ${needle} -> ${replacement}\n`);introduced=run(command,args);fs.writeFileSync(path.join(folder,'introduced.log'),introduced.output) } finally { fs.writeFileSync(absolute,original) }
  const reverted=run(command,args);fs.writeFileSync(path.join(folder,'reverted.log'),reverted.output)
  const result={id,file,introducedExitCode:introduced.exitCode,revertedExitCode:reverted.exitCode,status:introduced.exitCode!==0&&reverted.exitCode===0?'PASS':'FAIL',reverted:true};fs.writeFileSync(path.join(folder,'result.json'),`${JSON.stringify(result,null,2)}\n`);results.push(result)
}
const report={schema:'b14-r4-mutation-gates',sourceCommit:spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).stdout.trim(),mutations:results,status:results.every(item=>item.status==='PASS')?'PASS':'FAIL'}
fs.writeFileSync(path.join(evidenceDir,'B14_R4_MUTATION_GATES.json'),`${JSON.stringify(report,null,2)}\n`)
console.log(JSON.stringify(report,null,2));if(report.status!=='PASS')process.exitCode=1
