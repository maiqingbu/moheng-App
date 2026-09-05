import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export const REQUIRED_R3_GATES = [
  'b13_r3_01_task_reality_complete_read',
  'b13_r3_02_product_ux_bookplan_volumeplan',
  'b13_r3_03_current_performance_dual_reader',
  'shared_chunked_task_reader',
  'global_task_limit_unchanged',
  'task_100_boundary',
  'task_101_boundary',
  'task_501_formal_reality',
  'b09_task_reality_complete',
  'h04_task_blockage',
  'page_independent_truth',
  'bookplan_user_visible',
  'volumeplan_user_visible',
  'm11_browser',
  'product_ux_gate',
  'product_ux_mutation',
  'current_h03_query_plan',
  'current_h04_query_plan',
  'task_chunk_query_plan',
  'reader_a',
  'reader_b',
  'dual_reader_entry_set',
  'dual_reader_content_hash',
  'no_authority_mutation',
  'no_remote_model_call',
  'architecture',
  'module_boundary',
  'b09_regression',
  'b10_boundary',
  'b12_boundary',
  'b14_boundary',
  'b15_boundary',
  'rc1_boundary',
  'rust_fmt',
  'rust_check',
  'rust_clippy',
  'rust_tests',
  'node_tests',
  'vitest',
  'typecheck',
  'build',
  'contracts',
  'package_integrity',
  'package_inventory',
  'files_sha256',
  'package_utf8',
  'secret_scan',
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

export function evaluateR3Acceptance(report = {}) {
  const gates = report.gates ?? {}
  const missingGates = REQUIRED_R3_GATES.filter(name => !gates[name])
  const failedGates = REQUIRED_R3_GATES.filter(name => gates[name] && !passed(gates[name]))
  const sourceBinding = sourceBindingPassed(report)
  const status = missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'PASS' : 'FAIL'
  return {
    schema: 'b13-r3-acceptance',
    batch: 'B13-R3',
    milestone: 'Long-form Health final closeout repair',
    status,
    deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    repairBaselineShort: report.repairBaselineShort ?? 'e6419ce',
    repairBaselineCommit: report.repairBaselineCommit ?? null,
    postB12FrozenCommit: report.postB12FrozenCommit ?? null,
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    startCommit: report.startCommit ?? report.sourceCommit ?? null,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    requiredBlockingGates: REQUIRED_R3_GATES,
    missingGates,
    failedGates,
    nextBatchAllowed: false,
  }
}

function gateStatus(acceptance, name) {
  return passed(acceptance.gates?.[name]) ? 'PASS' : 'FAIL'
}

export function fixedR3Report(acceptance) {
  const status = name => gateStatus(acceptance, name)
  return {
    REPAIR_BASELINE_SHORT: acceptance.repairBaselineShort,
    REPAIR_BASELINE_COMMIT: acceptance.repairBaselineCommit,
    SOURCE_COMMIT: acceptance.sourceCommit,
    'B13-R3-01_TASK_REALITY_COMPLETE_READ': status('b13_r3_01_task_reality_complete_read'),
    'B13-R3-02_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN': status('b13_r3_02_product_ux_bookplan_volumeplan'),
    'B13-R3-03_CURRENT_PERFORMANCE_AND_DUAL_READER_EVIDENCE': status('b13_r3_03_current_performance_dual_reader'),
    SHARED_CHUNKED_TASK_READER: status('shared_chunked_task_reader'),
    GLOBAL_TASK_LIMIT_UNCHANGED: status('global_task_limit_unchanged'),
    TASK_100_BOUNDARY: status('task_100_boundary'),
    TASK_101_BOUNDARY: status('task_101_boundary'),
    TASK_501_FORMAL_REALITY: status('task_501_formal_reality'),
    B09_TASK_REALITY_COMPLETE: status('b09_task_reality_complete'),
    H04_TASK_BLOCKAGE: status('h04_task_blockage'),
    PAGE_INDEPENDENT_TRUTH: status('page_independent_truth'),
    BOOKPLAN_USER_VISIBLE: status('bookplan_user_visible'),
    VOLUMEPLAN_USER_VISIBLE: status('volumeplan_user_visible'),
    M11_BROWSER: status('m11_browser'),
    PRODUCT_UX_GATE: status('product_ux_gate'),
    PRODUCT_UX_MUTATION: status('product_ux_mutation'),
    CURRENT_H03_QUERY_PLAN: status('current_h03_query_plan'),
    CURRENT_H04_QUERY_PLAN: status('current_h04_query_plan'),
    TASK_CHUNK_QUERY_PLAN: status('task_chunk_query_plan'),
    READER_A: status('reader_a'),
    READER_B: status('reader_b'),
    DUAL_READER_ENTRY_SET: status('dual_reader_entry_set'),
    DUAL_READER_CONTENT_HASH: status('dual_reader_content_hash'),
    NO_AUTHORITY_MUTATION: status('no_authority_mutation'),
    NO_REMOTE_MODEL_CALL: status('no_remote_model_call'),
    ARCHITECTURE: status('architecture'),
    MODULE_BOUNDARY: status('module_boundary'),
    B09_REGRESSION: status('b09_regression'),
    B10_BOUNDARY: status('b10_boundary'),
    B12_BOUNDARY: status('b12_boundary'),
    B14_BOUNDARY: status('b14_boundary'),
    B15_BOUNDARY: status('b15_boundary'),
    RC1_BOUNDARY: status('rc1_boundary'),
    RUST_FMT: status('rust_fmt'),
    RUST_CHECK: status('rust_check'),
    RUST_CLIPPY: status('rust_clippy'),
    RUST_TESTS: status('rust_tests'),
    NODE_TESTS: status('node_tests'),
    VITEST: status('vitest'),
    TYPECHECK: status('typecheck'),
    BUILD: status('build'),
    CONTRACTS: status('contracts'),
    PACKAGE_INTEGRITY: status('package_integrity'),
    PACKAGE_INVENTORY: status('package_inventory'),
    FILES_SHA256: status('files_sha256'),
    PACKAGE_UTF8: status('package_utf8'),
    SECRET_SCAN: status('secret_scan'),
    DELIVERY_STATUS: acceptance.deliveryStatus,
    NEXT_BATCH_ALLOWED: false,
  }
}

export function generateR3Acceptance(evidenceDir = process.env.B13_R3_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-r3-evidence-latest')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error('B13_R3_EVIDENCE_MISSING:' + reportPath)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateR3Acceptance(report),
    gates: report.gates ?? {},
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b13-r3-acceptance.mjs',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R3_ACCEPTANCE.json'), JSON.stringify(acceptance, null, 2) + '\n')
  fs.writeFileSync(path.join(evidenceDir, 'B13_R3_FINAL_REPORT.txt'), Object.entries(fixedR3Report(acceptance)).map(([key, value]) => key + ' = ' + (typeof value === 'string' ? value : JSON.stringify(value))).join('\n') + '\n')
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b13-r3-acceptance.mjs')) {
  const result = generateR3Acceptance()
  console.log('B13-R3 acceptance generated: ' + result.status + '; missing=' + result.missingGates.length + '; failed=' + result.failedGates.length + '; nextBatchAllowed=false')
  if (result.status !== 'PASS') process.exitCode = 1
}
