import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const REQUIRED_R2_GATES = [
  'b13_r2_01_formal_planning_data_chain',
  'b13_r2_02_no_authority_mutation_evidence',
  'b13_r2_03_package_manifest_consistency',
  'h01_obligation', 'h02_foreshadow', 'h03_route_drift', 'h04_task_blockage',
  'h05_context_growth', 'h06_model_cost', 'h07_reader_desire', 'h08_quality_drift',
  'formal_b07_b09_source_binding', 'formal_owner_integration', 'compatibility_null_test',
  'compatibility_conflict_test', 'formal_stale_reuse', 'formal_owner_scale_gt_101_201',
  'page_independent_truth', 'nonempty_authority_fixture', 'authority_count_digest_unchanged',
  'no_remote_model_call', 'forbidden_scope_diff', 'package_inventory', 'files_sha256',
  'package_consistency', 'package_utf8', 'secret_scan', 'architecture', 'module_boundary',
  'b07_b09_regression', 'b10_boundary', 'b12_boundary', 'product_ux', 'b14_boundary',
  'b15_boundary', 'rc1_boundary', 'manual_browser', 'real_mutations', 'rust_fmt',
  'rust_check', 'rust_clippy', 'rust_tests', 'node_tests', 'vitest', 'typecheck',
  'build', 'contracts',
]

function passed(gate) {
  return gate?.status === 'PASS' && (gate.exitCode === undefined || gate.exitCode === 0)
}

function sourceBindingPassed(report) {
  return report.sourceCommit
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
}

export function evaluateR2Acceptance(report = {}) {
  const gates = report.gates ?? {}
  const missingGates = REQUIRED_R2_GATES.filter(name => !gates[name])
  const failedGates = REQUIRED_R2_GATES.filter(name => gates[name] && !passed(gates[name]))
  const sourceBinding = sourceBindingPassed(report)
  const status = missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'PASS' : 'FAIL'
  return {
    schema: 'b13-r2-acceptance',
    batch: 'B13-R2',
    milestone: 'Long-form Health / 长篇健康治理 final integration repair',
    status,
    deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    repairBaselineShort: report.repairBaselineShort ?? '3a291f4',
    repairBaselineCommit: report.repairBaselineCommit ?? null,
    postB12FrozenCommit: report.postB12FrozenCommit ?? null,
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    startCommit: report.startCommit ?? report.sourceCommit ?? null,
    startCommitIsAncestor: report.startCommitIsAncestor ?? true,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    requiredBlockingGates: REQUIRED_R2_GATES,
    missingGates,
    failedGates,
    nextBatchAllowed: false,
  }
}

function gateStatus(acceptance, name) {
  return passed(acceptance.gates?.[name]) ? 'PASS' : 'FAIL'
}

function fixedReport(acceptance) {
  const status = name => gateStatus(acceptance, name)
  return {
    REPAIR_BASELINE_SHORT: acceptance.repairBaselineShort,
    REPAIR_BASELINE_COMMIT: acceptance.repairBaselineCommit,
    SOURCE_COMMIT: acceptance.sourceCommit,
    'B13-R2-01_FORMAL_PLANNING_DATA_CHAIN': status('b13_r2_01_formal_planning_data_chain'),
    'B13-R2-02_NO_AUTHORITY_MUTATION_EVIDENCE': status('b13_r2_02_no_authority_mutation_evidence'),
    'B13-R2-03_PACKAGE_MANIFEST_CONSISTENCY': status('b13_r2_03_package_manifest_consistency'),
    H01_OBLIGATION: status('h01_obligation'), H02_FORESHADOW: status('h02_foreshadow'),
    H03_ROUTE_DRIFT: status('h03_route_drift'), H04_TASK_BLOCKAGE: status('h04_task_blockage'),
    H05_CONTEXT_GROWTH: status('h05_context_growth'), H06_MODEL_COST: status('h06_model_cost'),
    H07_READER_DESIRE: status('h07_reader_desire'), H08_QUALITY_DRIFT: status('h08_quality_drift'),
    FORMAL_B07_B09_SOURCE_BINDING: status('formal_b07_b09_source_binding'),
    FORMAL_OWNER_INTEGRATION: status('formal_owner_integration'),
    COMPATIBILITY_NULL_TEST: status('compatibility_null_test'),
    COMPATIBILITY_CONFLICT_TEST: status('compatibility_conflict_test'),
    FORMAL_STALE_REUSE: status('formal_stale_reuse'),
    FORMAL_OWNER_SCALE_GT_101_201: status('formal_owner_scale_gt_101_201'),
    PAGE_INDEPENDENT_TRUTH: status('page_independent_truth'),
    NONEMPTY_AUTHORITY_FIXTURE: status('nonempty_authority_fixture'),
    AUTHORITY_COUNT_DIGEST_UNCHANGED: status('authority_count_digest_unchanged'),
    NO_REMOTE_MODEL_CALL: status('no_remote_model_call'),
    FORBIDDEN_SCOPE_DIFF: status('forbidden_scope_diff'), PACKAGE_INVENTORY: status('package_inventory'),
    FILES_SHA256: status('files_sha256'), PACKAGE_CONSISTENCY: status('package_consistency'),
    PACKAGE_UTF8: status('package_utf8'), SECRET_SCAN: status('secret_scan'),
    ARCHITECTURE: status('architecture'), MODULE_BOUNDARY: status('module_boundary'),
    B07_B09_REGRESSION: status('b07_b09_regression'), B10_BOUNDARY: status('b10_boundary'),
    B12_BOUNDARY: status('b12_boundary'), PRODUCT_UX: status('product_ux'),
    B14_BOUNDARY: status('b14_boundary'), B15_BOUNDARY: status('b15_boundary'), RC1_BOUNDARY: status('rc1_boundary'),
    RUST_FMT: status('rust_fmt'), RUST_CHECK: status('rust_check'), RUST_CLIPPY: status('rust_clippy'),
    RUST_TESTS: status('rust_tests'), NODE_TESTS: status('node_tests'), VITEST: status('vitest'),
    TYPECHECK: status('typecheck'), BUILD: status('build'), CONTRACTS: status('contracts'),
    DELIVERY_STATUS: acceptance.deliveryStatus,
    NEXT_BATCH_ALLOWED: false,
  }
}

export function generateR2Acceptance(evidenceDir = process.env.B13_R2_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-r2-evidence-latest')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B13_R2_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateR2Acceptance(report),
    gates: report.gates ?? {},
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b13-r2-acceptance.mjs',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R2_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B13_R2_FINAL_REPORT.txt'), `${Object.entries(fixedReport(acceptance)).map(([key, value]) => `${key} = ${typeof value === 'string' ? value : JSON.stringify(value)}`).join('\n')}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b13-r2-acceptance.mjs')) {
  const result = generateR2Acceptance()
  console.log(`B13-R2 acceptance generated: ${result.status}; missing=${result.missingGates.length}; failed=${result.failedGates.length}; nextBatchAllowed=false`)
  if (result.status !== 'PASS') process.exitCode = 1
}
