import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14R3Acceptance, R3_REPAIR_BASELINE } from './generate-b14-r3-acceptance.mjs'

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const evidenceDir=path.resolve(process.env.B14_R3_EVIDENCE_DIR??fs.mkdtempSync(path.join(os.tmpdir(),'moheng-b14-r3-evidence-')))
const automatic=path.join(evidenceDir,'B14_R3','AUTOMATIC'); fs.mkdirSync(automatic,{recursive:true})
const git=args=>spawnSync('git',args,{cwd:root,encoding:'utf8'}).stdout.trim()
const sourceCommit=git(['rev-parse','HEAD']); const dirtyBefore=git(['status','--porcelain','--untracked-files=all'])!==''; const commands={}
function run(name,command,args,env={}){const startedAt=new Date().toISOString(),started=Date.now();const result=spawnSync(command,args,{cwd:root,encoding:'utf8',env:{...process.env,...env},maxBuffer:512*1024*1024});const output=`${result.stdout??''}${result.stderr??''}`;const logPath=`B14_R3/AUTOMATIC/${name}.log`;fs.writeFileSync(path.join(evidenceDir,logPath),output);const record={name,command:[command,...args].join(' '),startedAt,finishedAt:new Date().toISOString(),durationMs:Date.now()-started,exitCode:result.status??1,status:result.status===0?'PASS':'FAIL',rawLogPath:logPath,sourceCommit};commands[name]=record;return {...record,output}}
const gate=(record,extra={})=>({status:record.status,blocking:true,source:record.rawLogPath,...extra})
const architecture=run('architecture','node',['scripts/verify-b14-r3-architecture.mjs'],{B14_R3_SOURCE_COMMIT:sourceCommit})
const integration=run('integration','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_endgame_integration'])
const scale=run('scale','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--test','b14_r2_scale','--','--nocapture'])
const browser=run('browser','node',['scripts/capture-b14-r3-browser-evidence.mjs'],{B14_R3_EVIDENCE_DIR:evidenceDir,B14_R3_SOURCE_COMMIT:sourceCommit})
const mutations=run('mutations','node',['scripts/run-b14-r3-mutations.mjs'],{B14_R3_EVIDENCE_DIR:evidenceDir})
const ux=run('product_ux','npm',['run','verify:product-ux'])
const moduleBoundary=run('module_boundary','node',['scripts/verify-module-boundaries.mjs'])
const contracts=run('contracts','npm',['run','verify:contracts'])
const typecheck=run('typecheck','npm',['run','typecheck'])
const build=run('build','npm',['run','build'])
const nodeTests=run('node_tests','npm',['test'])
const fmt=run('rust_fmt','cargo',['fmt','--manifest-path','src-tauri/Cargo.toml','--all','--','--check'])
const check=run('rust_check','cargo',['check','--manifest-path','src-tauri/Cargo.toml','--all-targets'])
const clippy=run('rust_clippy','cargo',['clippy','--manifest-path','src-tauri/Cargo.toml','--all-targets','--all-features','--','-D','warnings'])
const tests=run('rust_tests','cargo',['test','--manifest-path','src-tauri/Cargo.toml','--all-targets'])
const historicalRestore=run('historical_b01_restore','git',['restore','--source',sourceCommit,'--','evidence/B01/registry_integrity.json','evidence/B01/schema_inventory.json'])
const dirtyAfter=git(['status','--porcelain','--untracked-files=all'])!==''
const browserJson=(()=>{try{return JSON.parse(fs.readFileSync(path.join(evidenceDir,'B14_R3_MANUAL_WALKTHROUGH.json'),'utf8'))}catch{return null}})()
const mutationJson=(()=>{try{return JSON.parse(fs.readFileSync(path.join(evidenceDir,'B14_R3_MUTATION_GATES.json'),'utf8'))}catch{return null}})()
for(const [file,record,extra] of [
  ['B14_R3_TASK_REALITY_CURRENT.json',integration,{test:'B14 task reality formal current reader'}],['B14_R3_CONTINUITY_FORMAL.json',integration,{test:'B14 continuity formal projection'}],['B14_R3_HARD_PAYOFF.json',integration,{test:'B14 hard payoff mapping'}],['B14_R3_RESERVATION_EVIDENCE.json',integration,{test:'B14 reservation evidence'}],['B14_R3_RELEVANT_REFS.json',integration,{test:'B14 structured relevant refs'}],['B14_R3_EXPORT_PREFLIGHT.json',integration,{test:'B14 structured export preflight'}],['B14_R3_S44_RUNTIME.json',integration,{test:'S44 model run provenance'}],['B14_R3_S45_RUNTIME.json',integration,{test:'S45 model run provenance'}],['B14_R3_PRODUCT_UX.json',ux,{}],['B14_R3_SCALE_10M.json',scale,{chapters:4000,minCharacters:10000000,pageSizes:[1,30,50]}],['B14_R3_QUERY_PLANS.json',scale,{fixture:'B14_R3 scale formal query'}],
]) fs.writeFileSync(path.join(evidenceDir,file),`${JSON.stringify({sourceCommit,baselineCommit:R3_REPAIR_BASELINE,command:record.command,startedAt:record.startedAt,finishedAt:record.finishedAt,exitCode:record.exitCode,status:record.status,rawLogPath:record.rawLogPath,...extra},null,2)}\n`)
const changed=git(['diff','--name-status',`${R3_REPAIR_BASELINE}..${sourceCommit}`]).split('\n').filter(Boolean);const forbidden=changed.filter(line=>/creative-skills\/v1\.2\.0|ExportRuntime|ReleaseOwner|ReleaseCertification|\bB15\b|\bRC1\b/.test(line))
const gates={formal_completion_truth:gate(integration),mapping_workflow:gate(integration),skill_runtime:gate(integration),product_ux:gate(ux),scale_10m:gate(scale,{chapters:4000,minCharacters:10000000}),page_independent_truth:gate(scale,{pageSizes:[1,30,50]}),browser_m01_m17:{status:browserJson?.status==='PASS'&&browserJson.items?.length===17?'PASS':'FAIL',blocking:true,source:'B14_R3_MANUAL_WALKTHROUGH.json'},mutation_gates:{status:mutationJson?.status??'FAIL',blocking:true,source:'B14_R3_MUTATION_GATES.json'},architecture:gate(architecture),rust_fmt:gate(fmt),rust_check:gate(check),rust_clippy:gate(clippy),rust_tests:gate(tests),node_tests:gate(nodeTests),typecheck:gate(typecheck),build:gate(build),contracts:gate(contracts),module_boundary:gate(moduleBoundary),b15_boundary:{status:forbidden.length?'FAIL':'PASS',blocking:true,source:'git diff boundary',findings:forbidden},rc1_boundary:{status:forbidden.length?'FAIL':'PASS',blocking:true,source:'git diff boundary',findings:forbidden},package_ascii_root:{status:'FAIL',blocking:true,source:'not packaged'},package_no_macosx:{status:'FAIL',blocking:true,source:'not packaged'},package_utf8:{status:'FAIL',blocking:true,source:'not packaged'},package_mojibake:{status:'FAIL',blocking:true,source:'not packaged'},package_dual_reader:{status:'FAIL',blocking:true,source:'not packaged'},package_inventory:{status:'FAIL',blocking:true,source:'not packaged'},files_sha256:{status:'FAIL',blocking:true,source:'not packaged'},package_integrity:{status:'FAIL',blocking:true,source:'not packaged'},secret_scan:{status:'FAIL',blocking:true,source:'not packaged'}}
const report={schema:'b14-r3-gate-results',batch:'B14-R3',repairBaselineCommit:R3_REPAIR_BASELINE,sourceCommit,evidenceSourceCommit:sourceCommit,sourceCommitAfter:git(['rev-parse','HEAD']),workingTreeDirtyBefore:dirtyBefore,workingTreeDirtyAfter:dirtyAfter,excludedUncommittedPaths:[],commands,historicalRestore,gates,nextBatchAllowed:false};report.status=Object.values(gates).every(item=>item.status==='PASS')?'PASS':'FAIL';fs.writeFileSync(path.join(evidenceDir,'gate-results-r3.json'),`${JSON.stringify(report,null,2)}\n`);const acceptance=generateB14R3Acceptance(evidenceDir);console.log(`B14_R3_EVIDENCE_RESULT sourceCommit=${sourceCommit} status=${acceptance.status} evidenceDir=${evidenceDir}`);if(acceptance.status!=='PASS')process.exitCode=1
