import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const REQUIRED_BLOCKING_GATES = [
  'b13_contract_policy', 'b13_deterministic_runtime', 'b13_projection_rebuild', 'b13_backend_query',
  'b13_formal_ui', 'b13_product_ux', 'b13_scale', 'b13_evidence',
  'factor_h01_obligation', 'factor_h02_foreshadow', 'factor_h03_route', 'factor_h04_task',
  'factor_h05_context', 'factor_h06_model_cost', 'factor_h07_reader_desire', 'factor_h08_quality',
  'architecture', 'module_boundary', 'no_authority_mutation', 'no_remote_model_call', 'determinism',
  'pagination', 'scale', 'product_ux', 'capability_parity', 'b14_boundary', 'b15_boundary', 'rc1_boundary',
  'rust_tests', 'node_tests', 'vitest', 'typecheck', 'build', 'contracts', 'package_integrity', 'package_utf8',
  'secret_scan', 'source_binding',
]

export function gatePassed(gate) {
  return gate?.status === 'PASS' && (gate.exitCode === undefined || gate.exitCode === 0)
}
export function sourceBindingPass(report) {
  return report?.sourceBinding === 'PASS'
    && typeof report.startCommit === 'string'
    && typeof report.sourceCommit === 'string'
    && report.startCommitIsAncestor === true
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
}

export function evaluateAcceptance(report = {}) {
  const gates = report.gates ?? {}
  const missingGates = REQUIRED_BLOCKING_GATES.filter(name => !gatePassed(gates[name]))
  const failedGates = REQUIRED_BLOCKING_GATES.filter(name => gates[name]?.status === 'FAIL')
  const sourceBinding = sourceBindingPass(report)
  return {
    schema: 'b13-acceptance-r1',
    batch: 'B13',
    milestone: 'Long-form Health / 长篇健康治理',
    status: missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'PASS' : 'FAIL',
    sourceBinding: sourceBinding ? 'PASS' : 'FAIL',
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    startCommit: report.startCommit ?? null,
    startCommitIsAncestor: report.startCommitIsAncestor ?? false,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    requiredBlockingGates: REQUIRED_BLOCKING_GATES,
    missingGates,
    failedGates,
    nextBatchAllowed: false,
  }
}

export function generateAcceptance(evidenceDir = process.env.B13_EVIDENCE_DIR ?? path.join(root, 'evidence/B13')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B13_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateAcceptance(report),
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b13-acceptance.mjs',
    deliveryStatus: evaluateAcceptance(report).status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    nextBatchAllowed: false,
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b13-acceptance.mjs')) {
  const result = generateAcceptance()
  console.log(`B13 acceptance generated: ${result.status}; missing=${result.missingGates.length}; failed=${result.failedGates.length}; nextBatchAllowed=false`)
  if (result.status !== 'PASS') process.exitCode = 1
}
