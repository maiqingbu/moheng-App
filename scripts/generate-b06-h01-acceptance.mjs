import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
export const REQUIRED_BLOCKING_GATES = [
  'b06_scale_topology',
  'contracts', 'node_tests', 'frontend_vitest', 'typecheck', 'build', 'architecture', 'b06_h01_architecture', 'module_boundary', 'm0a', 'b05_regression', 'b06_h01_targeted', 'cargo_fmt', 'cargo_check', 'cargo_clippy', 'cargo_all_targets', 'migration_0008',
  'b06_current_state', 'b06_narrative_time', 'b06_continuity', 'b06_resume_condition', 'b06_cross_volume_promise', 'b06_volume_progress', 'b06_context_manifest', 'b06_state_digest', 'b06_bounded_retrieval', 'b06_frontend_projection', 'b06_deterministic_digest', 'b06_bounded_truncation', 'b06_compiler_performance', 'h01_project_scope', 'h01_capability_registry', 'h01_support_planner', 'h01_executor', 'h01_sql_negative', 'h01_read_only_boundary', 'h01_diagnostics', 'h01_evidence_ledger', 'h01_checkpoint_resume', 'h01_checkpoint_stale', 'h01_checkpoint_replan', 'h01_bounded_replan', 'h01_session_receipt', 'h01_grounded_evidence', 'h01_bounded_query', 'source_binding',
]
export function evaluateAcceptance({ gateResults, requiredEvidence = [] }) {
  const byName = new Map((gateResults?.gates ?? []).map(gate => [gate.name, gate]))
  const gates = REQUIRED_BLOCKING_GATES.map(name => byName.get(name) ?? { name, status: 'FAIL', exitCode: 1, blocking: true, log: '' })
  const missingEvidence = requiredEvidence.filter(name => !gateResults?.evidence?.includes(name))
  const binding = gateResults?.sourceBinding?.status === 'PASS' ? 'PASS' : 'FAIL'
  return { status: gates.every(gate => gate.status === 'PASS' && gate.exitCode === 0) && binding === 'PASS' && missingEvidence.length === 0 ? 'PASS' : 'FAIL', gates, missingEvidence, sourceBinding: binding }
}
function digest(path) { return createHash('sha256').update(readFileSync(path)).digest('hex') }
export function generateAcceptance(evidenceDir = process.env.B06_H01_EVIDENCE_DIR ?? resolve(root, 'evidence/B06_H01')) {
  const gateResults = JSON.parse(readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8'))
  const files = readdirSync(evidenceDir)
  const result = evaluateAcceptance({ gateResults, requiredEvidence: ['B06/basis.json', 'H01/basis.json', 'B06_H01/source-binding.json'] })
  const acceptance = { batch: 'B06_H01', milestone: 'M1 Narrative Runtime Core / Project Support Harness Core', status: result.status, sourceCommit: gateResults.sourceCommit, sourceCommitAfter: gateResults.sourceCommitAfter, workingTreeDirtyBefore: gateResults.workingTreeDirtyBefore, workingTreeDirtyAfter: gateResults.workingTreeDirtyAfter, sourceBinding: result.sourceBinding, gates: result.gates, missingEvidence: result.missingEvidence, evidence: files.sort(), generatedAt: new Date().toISOString(), generator: 'scripts/generate-b06-h01-acceptance.mjs', generatorDigest: digest(fileURLToPath(import.meta.url)), basis: { b05SourceCommit: '315f21351e3f388d37717431732ac9e0ece9f831', b05ArchiveSha256: '555784bb9ecdd8f1caa83212b37dff8d4a106bf4d9a51da379513f58bc5856ee', b05FinalPassReportSha256: 'd89d52027fa1c5dadedb659a5f8dfe4c1f5a18eee0360cd93ff5be59bc9b8106', h00PackageSha256: '598e2d85d76a840a12c5824eb64e64532c0f472343aca8081226eb61d6acdaa3', creativeSkillsV120Sha256: 'a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f', jointInstructionSha256: 'a7af3d68e1e15af286828c4e0b2d8c85e34a903b6a907022874334336933bd0e', narrativeArchitectureSha256: '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b', narrativePrdSha256: '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44', narrativeBatchPlanSha256: '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206' }, evidenceGenerator: { path: 'scripts/generate-b06-h01-acceptance.mjs', digest: digest(fileURLToPath(import.meta.url)) }, gateRunner: { path: 'scripts/run-b06-h01-evidence-gates.mjs', digest: digest(resolve(root, 'scripts/run-b06-h01-evidence-gates.mjs')) } }
  writeFileSync(resolve(evidenceDir, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const acceptance = generateAcceptance()
  console.log(`B06_H01 acceptance generated: ${acceptance.status}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
