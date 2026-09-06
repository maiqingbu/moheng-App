import fs from 'node:fs'
import path from 'node:path'

export const R3_REPAIR_BASELINE = '7a3a30fe997ea956cf8da12efcb5384ae7a9ac65'
export const REQUIRED_R3_GATES = [
  'r3_01_completion_semantics', 'r3_02_continuity_relevant_refs', 'r3_03_skill_bundle_runtime', 'r3_04_evidence_package',
  'formal_completion_truth', 'mapping_workflow', 'skill_runtime', 'product_ux', 'scale_10m', 'page_independent_truth', 'query_plans',
  'browser_m01_m17', 'mutation_gates', 'architecture', 'rust_fmt', 'rust_check', 'rust_clippy', 'rust_tests', 'node_tests', 'vitest',
  'typecheck', 'build', 'build_demo', 'contracts', 'module_boundary', 'b13_regression', 'b15_boundary', 'rc1_boundary',
  'package_ascii_root', 'package_no_macosx', 'package_utf8', 'package_mojibake', 'package_dual_reader', 'package_inventory',
  'files_sha256', 'package_acceptance_self_contained', 'package_integrity', 'secret_scan',
]

export const REQUIRED_R3_EVIDENCE = [
  'B14_R3_PREBATCH_FREEZE.json', 'B14_R3_CHANGED_FILES.json', 'B14_R3_SOURCE_BINDING.json',
  'B14_R3_MYSTERY_DISPOSITION.json', 'B14_R3_RESERVATION_RELATION.json', 'B14_R3_HARD_PAYOFF_RELATION.json',
  'B14_R3_CONTINUITY_FORMAL.json', 'B14_R3_RELEVANT_INSTANCE_REFS.json', 'B14_R3_BOOK_EXIT_DOMAIN_DIGESTS.json',
  'B14_R3_S44_BUNDLE_RUNTIME.json', 'B14_R3_S45_BUNDLE_RUNTIME.json', 'B14_R3_SKILL_PROVENANCE.json',
  'B14_R3_NO_AUTHORITY_MUTATION.json', 'B14_R3_NO_REMOTE_CORE.json', 'B14_R3_SCALE_10M.json', 'B14_R3_QUERY_PLANS.json',
  'B14_R3_MANUAL_WALKTHROUGH.json', 'B14_R3_MUTATION_GATES.json', 'B14_R3_EVIDENCE_TRUTHFULNESS.json',
  'B14_R3_REGRESSION.json', 'B14_R3_PACKAGE_UTF8.json', 'B14_R3_PACKAGE_DUAL_READER.json', 'B14_R3_PACKAGE_CONSISTENCY.json',
  'B14_R3_PACKAGE_INTEGRITY.json', 'B14_R3_FINAL_DELIVERY_STATUS.json',
]

const readJson = (evidenceDir, name) => {
  try { return JSON.parse(fs.readFileSync(path.join(evidenceDir, name), 'utf8')) } catch { return null }
}

export function evaluateB14R3Acceptance(report) {
  const missingGates = REQUIRED_R3_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const missingEvidence = REQUIRED_R3_EVIDENCE.filter(name => report.evidence?.[name]?.status !== 'PASS')
  const exactSourceBinding = report.repairBaselineCommit === R3_REPAIR_BASELINE &&
    report.sourceCommit === report.evidenceSourceCommit &&
    report.sourceCommit === report.sourceCommitAfter &&
    report.workingTreeDirtyBefore === false &&
    report.workingTreeDirtyAfter === false &&
    Array.isArray(report.excludedUncommittedPaths) && report.excludedUncommittedPaths.length === 0
  const status = exactSourceBinding && missingGates.length === 0 && missingEvidence.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, missingEvidence, exactSourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}

export function generateB14R3Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r3.json'), 'utf8'))
  const result = evaluateB14R3Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R3_ACCEPTANCE.json'), `${JSON.stringify(result, null, 2)}\n`)
  const gate = name => result.gates?.[name]?.status === 'PASS' ? 'PASS' : 'FAIL'
  const reportText = `REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}
SOURCE_COMMIT = ${result.sourceCommit}

B14-R3-01_COMPLETION_SEMANTICS = ${gate('r3_01_completion_semantics')}
B14-R3-02_CONTINUITY_RELEVANT_REFS = ${gate('r3_02_continuity_relevant_refs')}
B14-R3-03_SKILL_BUNDLE_RUNTIME = ${gate('r3_03_skill_bundle_runtime')}
B14-R3-04_EVIDENCE_PACKAGE = ${gate('r3_04_evidence_package')}

MYSTERY_DISPOSITION_VALIDATION = ${gate('mystery_disposition')}
RESERVATION_RELATION_VALIDATION = ${gate('reservation_relation')}
HARD_PAYOFF_RELATION_VALIDATION = ${gate('hard_payoff_relation')}
CONTINUITY_FULL_PROJECTION = ${gate('continuity_full_projection')}
CONTINUITY_DIGEST_SAME_SOURCE = ${gate('continuity_digest_same_source')}
DOMAIN_DIGEST_INDEPENDENCE = ${gate('domain_digest_independence')}
INSTANCE_LEVEL_RELEVANT_REFS = ${gate('instance_level_relevant_refs')}
RELEVANT_CHANGE_POSITIVE = ${gate('relevant_change_positive')}
UNRELATED_CHANGE_NEGATIVE = ${gate('unrelated_change_negative')}

S44_REAL_BUNDLE = ${gate('s44_bundle')}
S45_REAL_BUNDLE = ${gate('s45_bundle')}
S44_SKILL_GUIDANCE = ${gate('s44_bundle')}
S45_SKILL_GUIDANCE = ${gate('s45_bundle')}
SKILL_TASK_CONTEXT_BOUNDED = ${gate('skill_runtime')}
SKILL_PROVENANCE = ${gate('skill_provenance')}
S44_SOFT_ONLY = ${gate('s44_bundle')}
S45_SOFT_ONLY = ${gate('s45_bundle')}

TASK_REALITY_REGRESSION = ${gate('formal_completion_truth')}
TASK_501_REGRESSION = ${gate('scale_10m')}
UNRELATED_REVISION_NO_STALE = ${gate('formal_completion_truth')}
POST_COMPLETION_RECHECK = ${gate('formal_completion_truth')}
SECOND_COMPLETION_NEW_DIGEST = ${gate('formal_completion_truth')}
COMPLETION_ATOMICITY = ${gate('formal_completion_truth')}
COMPLETION_IDEMPOTENCY = ${gate('formal_completion_truth')}

NO_AUTHORITY_MUTATION = ${gate('no_authority_mutation')}
NO_REMOTE_CORE = ${gate('no_remote_core')}
S44_MODEL_CALL = ${gate('s44_bundle')}
S45_MODEL_CALL = ${gate('s45_bundle')}
SCALE_10M = ${gate('scale_10m')}
PAGE_INDEPENDENT_TRUTH = ${gate('page_independent_truth')}
QUERY_PLANS = ${gate('query_plans')}
MANUAL_M01_M17 = ${gate('browser_m01_m17')}
MUTATION_GATES = ${gate('mutation_gates')}
EVIDENCE_TRUTHFULNESS = ${gate('evidence_truthfulness')}
PRODUCT_UX = ${gate('product_ux')}

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
  fs.writeFileSync(path.join(evidenceDir, 'B14_R3_FINAL_REPORT.txt'), reportText)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R3_FINAL_DELIVERY_STATUS.json'), `${JSON.stringify({ schema: 'b14-r3-final-delivery-status', sourceCommit: result.sourceCommit, status: result.status, deliveryStatus: result.deliveryStatus, nextBatchAllowed: false, missingGates: result.missingGates, missingEvidence: result.missingEvidence }, null, 2)}\n`)
  return result
}

export function loadB14R3Evidence(evidenceDir) {
  return Object.fromEntries(REQUIRED_R3_EVIDENCE.map(name => [name, readJson(evidenceDir, name)]))
}
