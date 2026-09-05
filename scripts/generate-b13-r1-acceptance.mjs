import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const REQUIRED_R1_GATES = [
  'b13_r1_01_chapter_identity',
  'b13_r1_02_plan_stale_semantics',
  'b13_r1_03_global_truth_scale',
  'b13_r1_04_product_identity',
  'b13_r1_05_evidence_truthfulness',
  'architecture',
  'module_boundary',
  'product_ux',
  'B10_boundary',
  'B12_boundary',
  'B14_boundary',
  'B15_boundary',
  'RC1_boundary',
  'rust_fmt',
  'rust_check',
  'rust_clippy',
  'rust_tests',
  'node_tests',
  'vitest',
  'typecheck',
  'build',
  'contracts',
  'package_preflight',
  'secret_scan',
  'source_binding',
  'manual_browser',
  'real_mutations',
  'no_remote_model_call',
  'no_authority_mutation',
  'query_plans',
]

function gatePassed(gate) {
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

export function evaluateR1Acceptance(report = {}) {
  const gates = report.gates ?? {}
  const missingGates = REQUIRED_R1_GATES.filter(name => !gates[name])
  const failedGates = REQUIRED_R1_GATES.filter(name => gates[name] && !gatePassed(gates[name]))
  const sourceBinding = sourceBindingPassed(report)
  const status = missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'PASS' : 'FAIL'
  return {
    schema: 'b13-r1-acceptance',
    batch: 'B13-R1',
    milestone: 'Long-form Health / 长篇健康治理',
    status,
    deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    repairBaselineCommit: report.repairBaselineCommit ?? null,
    postB12FrozenCommit: report.postB12FrozenCommit ?? null,
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    startCommit: report.sourceCommit ?? null,
    startCommitIsAncestor: report.sourceCommit !== null,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    requiredBlockingGates: REQUIRED_R1_GATES,
    missingGates,
    failedGates,
    nextBatchAllowed: false,
  }
}

function gateStatus(report, name) {
  return gatePassed(report.gates?.[name]) ? 'PASS' : 'FAIL'
}

function fixedReport(acceptance) {
  const status = name => gateStatus(acceptance, name)
  return {
    REPAIR_BASELINE_COMMIT: acceptance.repairBaselineCommit,
    SOURCE_COMMIT: acceptance.sourceCommit,
    'B13-R1-01_CHAPTER_IDENTITY': status('b13_r1_01_chapter_identity'),
    'B13-R1-02_PLAN_STALE_SEMANTICS': status('b13_r1_02_plan_stale_semantics'),
    'B13-R1-03_GLOBAL_TRUTH_SCALE': status('b13_r1_03_global_truth_scale'),
    'B13-R1-04_PRODUCT_IDENTITY': status('b13_r1_04_product_identity'),
    'B13-R1-05_EVIDENCE_TRUTHFULNESS': status('b13_r1_05_evidence_truthfulness'),
    H01_OBLIGATION: status('b13_r1_01_chapter_identity'),
    H02_FORESHADOW: status('b13_r1_01_chapter_identity'),
    H03_ROUTE_DRIFT: status('b13_r1_02_plan_stale_semantics'),
    H04_TASK_BLOCKAGE: status('b13_r1_03_global_truth_scale'),
    H05_CONTEXT_GROWTH: status('rust_tests'),
    H06_MODEL_COST: status('rust_tests'),
    H07_READER_DESIRE: status('b13_r1_01_chapter_identity'),
    H08_QUALITY_DRIFT: status('rust_tests'),
    CHAPTER_OPAQUE_ID: status('b13_r1_01_chapter_identity'),
    PLAN_STALE_OWNER_REUSE: status('b13_r1_02_plan_stale_semantics'),
    PAGE_INDEPENDENT_TRUTH: status('b13_r1_03_global_truth_scale'),
    SCALE_GT_51_101_201: status('b13_r1_03_global_truth_scale'),
    NO_RAW_VOLUME_ID: status('b13_r1_04_product_identity'),
    MANUAL_M01_M12: status('manual_browser'),
    MUTATION_REAL_EXECUTION: status('real_mutations'),
    NO_REMOTE_MODEL_CALL: status('no_remote_model_call'),
    NO_AUTHORITY_MUTATION: status('no_authority_mutation'),
    QUERY_PLANS: status('query_plans'),
    EVIDENCE_TRUTHFULNESS_GATE: status('b13_r1_05_evidence_truthfulness'),
    ARCHITECTURE: status('architecture'),
    MODULE_BOUNDARY: status('module_boundary'),
    PRODUCT_UX: status('product_ux'),
    B10_BOUNDARY: status('B10_boundary'),
    B12_BOUNDARY: status('B12_boundary'),
    B14_BOUNDARY: status('B14_boundary'),
    B15_BOUNDARY: status('B15_boundary'),
    RC1_BOUNDARY: status('RC1_boundary'),
    RUST_FMT: status('rust_fmt'),
    RUST_CHECK: status('rust_check'),
    RUST_CLIPPY: status('rust_clippy'),
    RUST_TESTS: status('rust_tests'),
    NODE_TESTS: status('node_tests'),
    VITEST: status('vitest'),
    TYPECHECK: status('typecheck'),
    BUILD: status('build'),
    CONTRACTS: status('contracts'),
    PACKAGE_INTEGRITY: status('package_preflight'),
    PACKAGE_UTF8: status('package_preflight'),
    SECRET_SCAN: status('secret_scan'),
    DELIVERY_STATUS: acceptance.deliveryStatus,
    NEXT_BATCH_ALLOWED: false,
  }
}

export function generateR1Acceptance(evidenceDir = process.env.B13_R1_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-r1-evidence-latest')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B13_R1_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateR1Acceptance(report),
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b13-r1-acceptance.mjs',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R1_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B13_R1_FINAL_REPORT.txt'), `${Object.entries(fixedReport({ ...report, ...acceptance })).map(([key, value]) => `${key} = ${typeof value === 'string' ? value : JSON.stringify(value)}`).join('\n')}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b13-r1-acceptance.mjs')) {
  const result = generateR1Acceptance()
  console.log(`B13-R1 acceptance generated: ${result.status}; missing=${result.missingGates.length}; failed=${result.failedGates.length}; nextBatchAllowed=false`)
  if (result.status !== 'PASS') process.exitCode = 1
}
