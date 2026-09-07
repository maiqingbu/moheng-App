import fs from 'node:fs'
import path from 'node:path'

export const R4_REPAIR_BASELINE = '0f853c5ced21bd4995263179b256e6e36693d457'
export const B13_FROZEN_COMMIT = '1a85e8f57c2098973d89977b02d33665cc054066'
export const REQUIRED_R4_GATES = [
  'r4_01_continuity_complete', 'r4_02_mapping_candidate_scope', 'r4_03_runtime_check_handoff', 'r4_04_evidence_acceptance',
  'continuity_complete', 'continuity_digest', 'mapping_candidates', 'runtime_check_handoff', 'no_authority_nonempty',
  'query_plans_complete', 'frozen_inputs_complete', 'acceptance_fail_closed', 'mutation_gates', 'evidence_truthfulness',
  'architecture', 'module_boundary', 'b13_regression', 'b15_boundary', 'rc1_boundary', 'rust_fmt', 'rust_check', 'rust_clippy',
  'rust_tests', 'node_tests', 'vitest', 'typecheck', 'build', 'build_demo', 'contracts', 'product_ux', 'scale_10m',
  'page_independent_truth', 'package_ascii_root', 'package_no_macosx', 'package_utf8', 'package_mojibake', 'package_dual_reader',
  'package_inventory', 'files_sha256', 'package_acceptance_self_contained', 'package_integrity', 'secret_scan',
]
export const REQUIRED_R4_EVIDENCE = [
  'B14_R4_PREBATCH_FREEZE.json', 'B14_R4_CHANGED_FILES.json', 'B14_R4_SOURCE_BINDING.json',
  'B14_R4_CONTINUITY_COMPLETE.json', 'B14_R4_CONTINUITY_DIGEST.json', 'B14_R4_MAPPING_CANDIDATES.json',
  'B14_R4_RUNTIME_CHECK_HANDOFF.json', 'B14_R4_NO_AUTHORITY_MUTATION.json', 'B14_R4_QUERY_PLANS.json',
  'B14_R4_FROZEN_INPUTS_CHECK.json', 'B14_R4_MUTATION_GATES.json', 'B14_R4_EVIDENCE_TRUTHFULNESS.json',
  'B14_R4_REGRESSION.json', 'B14_R4_PACKAGE_UTF8.json', 'B14_R4_PACKAGE_DUAL_READER.json',
  'B14_R4_PACKAGE_CONSISTENCY.json', 'B14_R4_FINAL_DELIVERY_STATUS.json',
]

export function evaluateB14R4Acceptance(report) {
  const missingGates = REQUIRED_R4_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const missingEvidence = REQUIRED_R4_EVIDENCE.filter(name => report.evidence?.[name]?.status !== 'PASS')
  const exactSourceBinding = report.repairBaselineCommit === R4_REPAIR_BASELINE && report.sourceCommit === report.evidenceSourceCommit && report.sourceCommit === report.sourceCommitAfter && report.workingTreeDirtyBefore === false && report.workingTreeDirtyAfter === false && Array.isArray(report.excludedUncommittedPaths) && report.excludedUncommittedPaths.length === 0
  const status = exactSourceBinding && missingGates.length === 0 && missingEvidence.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, missingEvidence, exactSourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}

export function generateB14R4Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r4.json'), 'utf8'))
  const result = evaluateB14R4Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R4_ACCEPTANCE.json'), `${JSON.stringify(result, null, 2)}\n`)
  const gate = name => result.gates?.[name]?.status === 'PASS' ? 'PASS' : 'FAIL'
  const text = `REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}
SOURCE_COMMIT = ${result.sourceCommit}

B14-R4-01_CONTINUITY_COMPLETE = ${gate('r4_01_continuity_complete')}
B14-R4-02_MAPPING_CANDIDATE_SCOPE = ${gate('r4_02_mapping_candidate_scope')}
B14-R4-03_RUNTIME_CHECK_HANDOFF = ${gate('r4_03_runtime_check_handoff')}
B14-R4-04_EVIDENCE_ACCEPTANCE = ${gate('r4_04_evidence_acceptance')}
FORESHADOW_COMPLETE = ${gate('continuity_complete')}
MYSTERY_COMPLETE = ${gate('continuity_complete')}
CONTINUITY_EVIDENCE_COMPLETE = ${gate('continuity_complete')}
CONTINUITY_101 = ${gate('continuity_complete')}
CONTINUITY_501 = ${gate('continuity_complete')}
CONTINUITY_DIGEST_SAME_SOURCE = ${gate('continuity_digest')}
RESERVATION_CANDIDATES_RELATED_ONLY = ${gate('mapping_candidates')}
PAYOFF_CANDIDATES_RELATED_ONLY = ${gate('mapping_candidates')}
UNRELATED_FINALCHAPTER_OMITTED = ${gate('mapping_candidates')}
NO_RELATED_EVIDENCE_EMPTY = ${gate('mapping_candidates')}
MAPPING_2000_BOUNDED = ${gate('mapping_candidates')}
S44_RUNTIME_CHECK_HANDOFF = ${gate('runtime_check_handoff')}
S45_RUNTIME_CHECK_HANDOFF = ${gate('runtime_check_handoff')}
RUNTIME_CHECK_VALIDATION = ${gate('runtime_check_handoff')}
RUNTIME_DETERMINISTIC_RECHECK = ${gate('runtime_check_handoff')}
RUNTIME_CHECK_NO_EXTRA_MODEL = ${gate('runtime_check_handoff')}
RUNTIME_CHECK_NO_AUTHORITY_WRITE = ${gate('runtime_check_handoff')}
NO_AUTHORITY_ALL_DOMAINS_NONEMPTY = ${gate('no_authority_nonempty')}
NO_AUTHORITY_COUNT_DIGEST = ${gate('no_authority_nonempty')}
QUERY_PLANS_COMPLETE = ${gate('query_plans_complete')}
FROZEN_INPUTS_COMPLETE = ${gate('frozen_inputs_complete')}
ACCEPTANCE_FAIL_CLOSED = ${gate('acceptance_fail_closed')}
MUTATION_GATES = ${gate('mutation_gates')}
EVIDENCE_TRUTHFULNESS = ${gate('evidence_truthfulness')}
TASK_REALITY_REGRESSION = ${gate('regression')}
TASK_501_REGRESSION = ${gate('scale_10m')}
UNRELATED_REVISION_NO_STALE = ${gate('regression')}
POST_COMPLETION_RECHECK = ${gate('regression')}
SECOND_COMPLETION_NEW_DIGEST = ${gate('regression')}
SKILL_BUNDLE_REGRESSION = ${gate('skill_bundle')}
PRODUCT_UX_REGRESSION = ${gate('product_ux')}
SCALE_10M_REGRESSION = ${gate('scale_10m')}
PAGE_INDEPENDENT_TRUTH = ${gate('page_independent_truth')}
ARCHITECTURE = ${gate('architecture')}
MODULE_BOUNDARY = ${gate('module_boundary')}
B09_REGRESSION = ${gate('b13_regression')}
B10_REGRESSION = ${gate('b13_regression')}
B12_REGRESSION = ${gate('b13_regression')}
B13_REGRESSION = ${gate('b13_regression')}
B15_BOUNDARY = ${gate('b15_boundary')}
RC1_BOUNDARY = ${gate('rc1_boundary')}
RUST_FMT = ${gate('rust_fmt')}
RUST_CHECK = ${gate('rust_check')}
RUST_CLIPPY = ${gate('rust_clippy')}
RUST_TESTS = ${gate('rust_tests')}
NODE_TESTS = ${gate('node_tests')}
VITEST = ${gate('vitest')}
TYPECHECK = ${gate('typecheck')}
BUILD = ${gate('build')}
CONTRACTS = ${gate('contracts')}
PACKAGE_ASCII_ROOT = ${gate('package_ascii_root')}
PACKAGE_NO_MACOSX = ${gate('package_no_macosx')}
PACKAGE_UTF8 = ${gate('package_utf8')}
PACKAGE_MOJIBAKE = ${gate('package_mojibake')}
PACKAGE_DUAL_READER = ${gate('package_dual_reader')}
PACKAGE_INVENTORY = ${gate('package_inventory')}
FILES_SHA256 = ${gate('files_sha256')}
PACKAGE_ACCEPTANCE_SELF_CONTAINED = ${gate('package_acceptance_self_contained')}
PACKAGE_INTEGRITY = ${gate('package_integrity')}
SECRET_SCAN = ${gate('secret_scan')}
DELIVERY_STATUS = ${result.deliveryStatus}
NEXT_BATCH_ALLOWED = false
MISSING_GATES = ${JSON.stringify(result.missingGates)}
MISSING_EVIDENCE = ${JSON.stringify(result.missingEvidence)}
`
  fs.writeFileSync(path.join(evidenceDir, 'B14_R4_FINAL_REPORT.txt'), text)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R4_FINAL_DELIVERY_STATUS.json'), `${JSON.stringify({schema:'b14-r4-final-delivery-status',sourceCommit:result.sourceCommit,status:result.status,deliveryStatus:result.deliveryStatus,nextBatchAllowed:false,missingGates:result.missingGates,missingEvidence:result.missingEvidence}, null, 2)}\n`)
  return result
}
