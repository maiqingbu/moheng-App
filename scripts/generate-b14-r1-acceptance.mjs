import fs from 'node:fs'
import path from 'node:path'

export const REQUIRED_R1_GATES = [
  'b14_r1_01_completion_truth',
  'b14_r1_02_endgame_relevant_state',
  'b14_r1_03_post_completion_lifecycle',
  'b14_r1_04_s44_s45_integration',
  'b14_r1_05_product_ux',
  'b14_r1_06_evidence_package',
  'obligation_check', 'mystery_check', 'payoff_check', 'requested_ending_check',
  'change_stale_check', 'export_preflight_check', 'continuity_formal_source',
  'task_reality_current_state', 'future_reservation_formal_evidence', 'endgame_digest',
  'unrelated_revision_no_stale', 'post_completion_recheck', 'second_completion_new_digest',
  'book_exit_state_domain_digests', 's44_soft_only', 's45_soft_only',
  'no_authority_mutation', 'no_remote_model_call', 'completion_atomicity',
  'completion_idempotency', 'page_independent_truth', 'scale_10m', 'query_plans',
  'product_ux', 'mutation_gates', 'evidence_truthfulness', 'architecture',
  'module_boundary', 'b09_regression', 'b10_regression', 'b12_regression',
  'b13_regression', 'b15_boundary', 'rc1_boundary', 'rust_fmt', 'rust_check',
  'rust_clippy', 'rust_tests', 'node_tests', 'vitest', 'typecheck', 'build',
  'contracts', 'package_ascii_root', 'package_no_macosx', 'package_utf8',
  'package_mojibake', 'package_dual_reader', 'package_inventory', 'files_sha256',
  'package_integrity', 'secret_scan',
]
export const R1_REPAIR_BASELINE = 'eb259d170ef24c8fc2e28d2cc5af65449f9c295a'

export function evaluateB14R1Acceptance(report) {
  const missingGates = REQUIRED_R1_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const exactBinding = report.repairBaselineCommit === R1_REPAIR_BASELINE && Boolean(
    report.sourceCommit && report.sourceCommit === report.evidenceSourceCommit &&
    report.sourceCommit === report.sourceCommitAfter &&
    report.workingTreeDirtyBefore === false && report.workingTreeDirtyAfter === false &&
    Array.isArray(report.excludedUncommittedPaths) && report.excludedUncommittedPaths.length === 0,
  )
  const status = missingGates.length === 0 && exactBinding ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, exactSourceBinding: exactBinding, nextBatchAllowed: false, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL' }
}

export function generateB14R1Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r1.json'), 'utf8'))
  const acceptance = evaluateB14R1Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R1_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R1_FINAL_REPORT.txt'), `${Object.entries({ REPAIR_BASELINE_COMMIT: acceptance.repairBaselineCommit, SOURCE_COMMIT: acceptance.sourceCommit, DELIVERY_STATUS: acceptance.deliveryStatus, NEXT_BATCH_ALLOWED: false, MISSING_GATES: acceptance.missingGates }).map(([key, value]) => `${key} = ${typeof value === 'string' ? value : JSON.stringify(value)}`).join('\n')}\n`)
  return acceptance
}
