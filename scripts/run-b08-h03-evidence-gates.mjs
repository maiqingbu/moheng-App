import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root,'src-tauri')
const evidence = process.env.B08_H03_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(),'moheng-b08-h03-evidence.'))
fs.mkdirSync(evidence,{recursive:true})
const startBaseline = '3177ff209331db1cc2d18787aef432c190110b57'
const generatedRegressionFiles = ['evidence/B01/registry_integrity.json','evidence/B01/schema_inventory.json']

function git(args, encoding='utf8') { return spawnSync('git',args,{cwd:root,encoding}) }
function run(name,command,args,cwd=root) {
  const result = spawnSync(command,args,{cwd,encoding:'utf8',env:process.env})
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  fs.writeFileSync(path.join(evidence,`${name}.log`),output)
  return {name,status:result.status===0?'PASS':'FAIL',exitCode:result.status ?? 1,output}
}
const beforeCommit = git(['rev-parse','HEAD']).stdout.trim()
const dirtyBefore = git(['status','--porcelain']).stdout.trim().length > 0
const basis = git(['merge-base','--is-ancestor',startBaseline,beforeCommit]).status === 0 && beforeCommit !== startBaseline

const targeted = run('b08_h03_targeted','cargo',['test','--test','b08_h03','--','--nocapture'],rustRoot)
const b02 = run('b02_atomic_regression','cargo',['test','--test','b02_commit_bundle'],rustRoot)
const b03 = run('b03_runtime_regression','cargo',['test','--test','b03_runtime'],rustRoot)
const b05 = run('b05_regression','cargo',['test','--test','b05_narrative'],rustRoot)
const b06 = run('b06_h01_regression','cargo',['test','--test','b06_h01'],rustRoot)
const b07 = run('b07_h02_regression','cargo',['test','--test','b07_h02','--test','b07_h02_semantic_negative'],rustRoot)
const architecture = run('architecture','node',['scripts/verify-b08-h03-architecture.mjs'])
const moduleBoundary = run('module_boundary','node',['scripts/verify-module-boundaries.mjs'])
const contracts = run('contracts','npm',['run','verify:contracts'])
const node = run('node','npm',['test'])
const vitest = run('vitest','npm',['run','test:store'])
const typecheck = run('typecheck','npm',['run','typecheck'])
const build = run('build','npm',['run','build'])
const m0 = run('m0_regression','node',['scripts/verify-m0a-evidence.mjs'])
const fmt = run('cargo_fmt','cargo',['fmt','--all','--','--check'],rustRoot)
const check = run('cargo_check','cargo',['check','--all-targets'],rustRoot)
const clippy = run('cargo_clippy','cargo',['clippy','--all-targets','--all-features','--','-D','warnings'],rustRoot)
const cargoAll = run('cargo_all_targets','cargo',['test','--all-targets'],rustRoot)

if (!dirtyBefore) {
  for (const file of generatedRegressionFiles) {
    if (git(['diff','--quiet','--',file]).status !== 0) {
      const baseline = git(['show',`${beforeCommit}:${file}`],'buffer')
      if (baseline.status === 0) fs.writeFileSync(path.join(root,file),baseline.stdout)
    }
  }
}

const packagePath = path.join(os.tmpdir(),`moheng-b08-h03-source-${beforeCommit.slice(0,7)}.zip`)
const archive = spawnSync('git',['archive','--format=zip','-o',packagePath,'HEAD'],{cwd:root,encoding:'utf8'})
const zipTest = archive.status === 0 ? spawnSync('unzip',['-t',packagePath],{encoding:'utf8'}) : {status:1,stdout:'',stderr:'archive failed'}
const zipList = archive.status === 0 ? spawnSync('unzip',['-Z1',packagePath],{encoding:'utf8'}) : {status:1,stdout:''}
const forbiddenPackage = /(^|\/)(target|node_modules|\.git)(\/|$)|\.db($|[-.])/m.test(zipList.stdout ?? '')
fs.writeFileSync(path.join(evidence,'package_smoke.log'),`${archive.stdout??''}${archive.stderr??''}${zipTest.stdout??''}${zipTest.stderr??''}`)
const packageSmoke = archive.status === 0 && zipTest.status === 0 && !forbiddenPackage

const has = (result,name) => result.status === 'PASS' && result.output.includes(name)
const gate = (status,source) => ({status:status?'PASS':'FAIL',source,blocking:true})
const t = name => has(targeted,name)
const gates = {
  'B08-F01':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-F02':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-F03':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-F04':gate(has(b02,'b02_each_delta_channel_failure_rolls_back_the_entire_bundle'),'b02_atomic_regression.log'),
  'B08-F05':gate(t('b08_cross_volume_promise_keeps_old_evidence_and_adds_actual_evidence'),'b08_h03_targeted.log'),
  'B08-F06':gate(t('b08_flashback_knowledge_persists_provenance_and_future_contradiction_blocks'),'b08_h03_targeted.log'),
  'B08-F07':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-F08':gate(t('b08_production_commits_final_and_never_promotes_candidate') && b03.status==='PASS','b08_h03_targeted.log+b03_runtime_regression.log'),
  'B08-F09':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-F10':gate(node.status==='PASS' && architecture.status==='PASS','node.log+architecture.log'),
  'B08-F11':gate(t('b08_revalidates_relevant_state_after_render_and_blocks_stale_final_commit'),'b08_h03_targeted.log'),
  'B08-F12':gate([b02,b03,b05,b06,b07,m0].every(item=>item.status==='PASS'),'regression logs'),
  'B08-I01':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-I02':gate(t('b08_long_form_reuses_exact_current_plan_and_bound_context_in_same_runtime'),'b08_h03_targeted.log'),
  'B08-I03':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-I04':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-I05':gate(t('b08_cross_volume_promise_keeps_old_evidence_and_adds_actual_evidence') && t('b08_flashback_knowledge_persists_provenance_and_future_contradiction_blocks'),'b08_h03_targeted.log'),
  'B08-I06':gate(t('b08_production_commits_final_and_never_promotes_candidate'),'b08_h03_targeted.log'),
  'B08-I07':gate(t('b08_revalidates_relevant_state_after_render_and_blocks_stale_final_commit'),'b08_h03_targeted.log'),
  'B08-I08':gate(architecture.status==='PASS','architecture.log'),
  'B08-I09':gate(node.status==='PASS' && typecheck.status==='PASS' && architecture.status==='PASS','node.log+typecheck.log+architecture.log'),
  'H03-A01':gate(t('h03_authorization_is_revision_and_digest_bound_and_one_shot'),'b08_h03_targeted.log'),
  'H03-A02':gate(t('h03_execute_without_authorization_and_stale_authorization_fail_before_owner'),'b08_h03_targeted.log'),
  'H03-A03':gate(t('h03_authorization_is_revision_and_digest_bound_and_one_shot'),'b08_h03_targeted.log'),
  'H03-A04':gate(t('h03_execute_without_authorization_and_stale_authorization_fail_before_owner'),'b08_h03_targeted.log'),
  'H03-A05':gate(t('h03_owner_failure_consumes_authorization_and_persists_failed_receipt'),'b08_h03_targeted.log'),
  'H03-A06':gate(t('h03_stage_matrix_rejects_b09_route_and_keeps_change_request_proposal_only') && architecture.status==='PASS','b08_h03_targeted.log+architecture.log'),
  'H03-A07':gate(t('h03_recompile_uses_b07_owner_and_b06_stale_requery'),'b08_h03_targeted.log'),
  'H03-A08':gate(t('h03_recompile_uses_b07_owner_and_b06_stale_requery'),'b08_h03_targeted.log'),
  'H03-A09':gate(t('h03_postverify_requery_failure_is_receipted_and_not_reported_as_repaired'),'b08_h03_targeted.log'),
  'H03-A10':gate(t('h03_stage_matrix_rejects_b09_route_and_keeps_change_request_proposal_only'),'b08_h03_targeted.log'),
  'H03-A11':gate(t('h03_stage_matrix_rejects_b09_route_and_keeps_change_request_proposal_only'),'b08_h03_targeted.log'),
  'H03-I01':gate(t('h03_support_schema_is_fresh_reopen_upgrade_and_copy_safe') && t('h03_owner_failure_consumes_authorization_and_persists_failed_receipt'),'b08_h03_targeted.log'),
  'H03-I02':gate(node.status==='PASS' && typecheck.status==='PASS' && architecture.status==='PASS','node.log+typecheck.log+architecture.log'),
  'H03-I03':gate(t('h03_recompile_uses_b07_owner_and_b06_stale_requery'),'b08_h03_targeted.log'),
  'H03-I04':gate(t('h03_recompile_uses_b07_owner_and_b06_stale_requery') && architecture.status==='PASS','b08_h03_targeted.log+architecture.log'),
  basis:gate(basis,'git'), migration:gate(t('h03_support_schema_is_fresh_reopen_upgrade_and_copy_safe'),'b08_h03_targeted.log'),
  architecture:gate(architecture.status==='PASS','architecture.log'), module_boundary:gate(moduleBoundary.status==='PASS','module_boundary.log'),
  contracts:gate(contracts.status==='PASS','contracts.log'), node:gate(node.status==='PASS','node.log'), vitest:gate(vitest.status==='PASS','vitest.log'),
  typecheck:gate(typecheck.status==='PASS','typecheck.log'), build:gate(build.status==='PASS','build.log'),
  b07_h02_regression:gate(b07.status==='PASS','b07_h02_regression.log'), b06_h01_regression:gate(b06.status==='PASS','b06_h01_regression.log'),
  b05_regression:gate(b05.status==='PASS','b05_regression.log'), m0_regression:gate(m0.status==='PASS'&&b02.status==='PASS'&&b03.status==='PASS','m0_regression.log+b02+b03'),
  cargo_all_targets:gate(cargoAll.status==='PASS','cargo_all_targets.log'), cargo_fmt:gate(fmt.status==='PASS','cargo_fmt.log'),
  cargo_check:gate(check.status==='PASS','cargo_check.log'), cargo_clippy:gate(clippy.status==='PASS','cargo_clippy.log'),
  source_binding:gate(false,'git'), package_smoke:gate(packageSmoke,'package_smoke.log'),
}
const afterCommit = git(['rev-parse','HEAD']).stdout.trim()
const dirtyAfter = git(['status','--porcelain']).stdout.trim().length > 0
const sourceBinding = beforeCommit===afterCommit && !dirtyBefore && !dirtyAfter
gates.source_binding = gate(sourceBinding,'git')
const report = {schema:'b08-h03-gate-results-r2',batch:'B08_H03',sourceCommit:beforeCommit,sourceCommitAfter:afterCommit,workingTreeDirtyBefore:dirtyBefore,workingTreeDirtyAfter:dirtyAfter,sourceBinding:sourceBinding?'PASS':'FAIL',scale:{required:false,reason:'No bounded high-volume query contract changed in B08/H03.'},gates}
fs.writeFileSync(path.join(evidence,'gate-results.json'),`${JSON.stringify(report,null,2)}\n`)
console.log(`B08_H03 gates: ${Object.values(gates).filter(item=>item.status==='PASS').length}/${Object.keys(gates).length} PASS; sourceBinding=${report.sourceBinding}; evidence=${evidence}`)
if (Object.values(gates).some(item=>item.status!=='PASS')) process.exitCode=1
