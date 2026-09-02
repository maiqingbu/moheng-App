import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const START_COMMIT = 'edadc96aba68fe89b8e1e453387eb6ffe3fd9968'
export const B12_BLOCKING_GATES = Array.from({ length: 61 }, (_, index) => `B12-A${String(index).padStart(2, '0')}`)
export const NEGATIVE_BLOCKING_GATES = Array.from({ length: 28 }, (_, index) => `NEG-${String(index + 1).padStart(2, '0')}`)
export const R2_GLOBAL_BLOCKING_GATES = [
  'second_authority',
  'second_production_path',
  'commit_bundle_v3',
  'narrative_runtime_delta',
  'sql_outside_approved_owner',
  'skill_direct_db_query',
  'harness_arbitrary_write',
  'p3_permanent_authority',
  'future_reservation_bypass',
  'plan_writes_reality',
  'candidate_visible_as_final',
  'ui_local_truth',
  'unbounded_project_context',
  'fake_provider_demo_formal_path',
  'future_batch_owner_early',
]
export const HISTORICAL_REGRESSION_GATES = [
  'b05_regression',
  'b06_h01_regression',
  'b07_h02_regression',
  'b08_h03_regression',
  'r2_alignment_regression',
  'p01_regression',
  'b09_h04_regression',
  'b10_regression',
  'b11_regression',
]
export const ENGINEERING_GATES = [
  'node',
  'vitest',
  'contracts',
  'typecheck',
  'build',
  'cargo_all_targets',
  'cargo_fmt',
  'cargo_check',
  'cargo_clippy',
  'architecture',
  'base_architecture',
  'module_boundary',
  'm0a',
]
export const B12_ENGINEERING_GATES = [
  'b12_change_retcon',
  'b12_h03_capability',
  'b12_product_ux',
  'b12_language_scan',
  'b12_scale_bound',
]
export const RELEASE_GATES = [
  'source_binding',
  'package_smoke',
  'fresh_package_architecture',
  'fresh_package_base_architecture',
  'fresh_package_module_boundary',
  'fresh_package_generator',
  'fresh_package_node',
  'fresh_package_language',
]
export const REQUIRED_BLOCKING_GATES = [
  ...B12_BLOCKING_GATES,
  ...NEGATIVE_BLOCKING_GATES,
  ...R2_GLOBAL_BLOCKING_GATES,
  ...HISTORICAL_REGRESSION_GATES,
  ...ENGINEERING_GATES,
  ...B12_ENGINEERING_GATES,
  ...RELEASE_GATES,
]

function gatePassed(gate) {
  return gate?.status === 'PASS' && (gate.exitCode === undefined || gate.exitCode === 0)
}

export function sourceBindingPass(report) {
  return report?.sourceBinding === 'PASS'
    && report.startCommit === START_COMMIT
    && report.startCommitIsAncestor === true
    && typeof report.sourceCommit === 'string'
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
}

export function evaluateAcceptance(report = {}, options = {}) {
  const gates = report.gates ?? {}
  const requiredGates = options.excludeSelfGate
    ? REQUIRED_BLOCKING_GATES.filter(name => name !== 'fresh_package_generator')
    : REQUIRED_BLOCKING_GATES
  const missingGates = requiredGates.filter(name => !gates[name])
  const failedGates = requiredGates.filter(name => gates[name] && !gatePassed(gates[name]))
  const sourceBinding = sourceBindingPass(report)
  return {
    schema: 'b12-acceptance-r1',
    batch: 'B12',
    milestone: 'Change / Retcon Runtime',
    status: missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'READY_FOR_ACCEPTANCE' : 'NOT_READY',
    startCommit: report.startCommit ?? null,
    startCommitIsAncestor: report.startCommitIsAncestor ?? false,
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    sourceBinding: sourceBinding ? 'PASS' : 'FAIL',
    requiredBlockingGates: requiredGates,
    failedGates,
    missingGates,
  }
}

export function generateAcceptance(evidenceDir = process.env.B12_EVIDENCE_DIR ?? path.join(root, 'evidence/B12')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B12_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateAcceptance(report, { excludeSelfGate: process.env.B12_GENERATOR_SELF_CHECK === '1' }),
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b12-acceptance.mjs',
  }
  fs.mkdirSync(evidenceDir, { recursive: true })
  fs.writeFileSync(path.join(evidenceDir, 'B12_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b12-acceptance.mjs')) {
  const result = generateAcceptance()
  console.log(`B12 acceptance generated: ${result.status}; missing=${result.missingGates.length}; failed=${result.failedGates.length}`)
  if (result.status !== 'READY_FOR_ACCEPTANCE') process.exitCode = 1
}

