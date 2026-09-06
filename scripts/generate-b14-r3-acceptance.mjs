import fs from 'node:fs'
import path from 'node:path'

export const R3_REPAIR_BASELINE = '7a3a30fe997ea956cf8da12ef5384ae7a9ac65'
export const REQUIRED_R3_GATES = [
  'formal_completion_truth','mapping_workflow','skill_runtime','product_ux','scale_10m','page_independent_truth','browser_m01_m17','mutation_gates','architecture','rust_fmt','rust_check','rust_clippy','rust_tests','node_tests','typecheck','build','contracts','module_boundary','b15_boundary','rc1_boundary','package_ascii_root','package_no_macosx','package_utf8','package_mojibake','package_dual_reader','package_inventory','files_sha256','package_integrity','secret_scan',
]
export function evaluateB14R3Acceptance(report) {
  const missingGates = REQUIRED_R3_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const exactSourceBinding = report.repairBaselineCommit === R3_REPAIR_BASELINE && report.sourceCommit === report.evidenceSourceCommit && report.sourceCommit === report.sourceCommitAfter && report.workingTreeDirtyBefore === false && report.workingTreeDirtyAfter === false && Array.isArray(report.excludedUncommittedPaths) && report.excludedUncommittedPaths.length === 0
  const status = exactSourceBinding && missingGates.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, exactSourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}
export function generateB14R3Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir,'gate-results-r3.json'),'utf8'))
  const result = evaluateB14R3Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir,'B14_R3_ACCEPTANCE.json'),`${JSON.stringify(result,null,2)}\n`)
  fs.writeFileSync(path.join(evidenceDir,'B14_R3_FINAL_REPORT.txt'),`REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}\nSOURCE_COMMIT = ${result.sourceCommit}\nDELIVERY_STATUS = ${result.deliveryStatus}\nNEXT_BATCH_ALLOWED = false\nMISSING_GATES = ${JSON.stringify(result.missingGates)}\n`)
  return result
}
