import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
export const REQUIRED_BLOCKING_GATES = [
  'b07_bookplan_maturity', 'b07_volume_route', 'b07_chapter_plan_gates', 'b07_plan_stale',
  'b07_plan_reality_separation', 'b07_frontend_route', 'h02_registry', 'h02_plan_route',
  'h02_constraints', 'h02_truncation', 'h02_read_only', 'b06_h01_regression',
  'architecture', 'module_boundary', 'source_binding', 'package_smoke',
]

export function evaluateAcceptance(gates) {
  const entries = gates.gates ?? {}
  const missing = REQUIRED_BLOCKING_GATES.filter(name => entries[name]?.status !== 'PASS')
  const sourceBinding = gates.sourceBinding === 'PASS'
  const status = missing.length === 0 && sourceBinding ? 'PASS' : 'FAIL'
  return { batch: 'B07_H02', status, sourceCommit: gates.sourceCommit ?? null, sourceCommitAfter: gates.sourceCommitAfter ?? null, sourceBinding, missingBlockingGates: missing, requiredBlockingGates: REQUIRED_BLOCKING_GATES }
}

export function generateAcceptance(evidenceDir = process.env.B07_H02_EVIDENCE_DIR ?? resolve(root, 'evidence/B07_H02')) {
  const gatesPath = resolve(evidenceDir, 'gate-results.json')
  if (!existsSync(gatesPath)) throw new Error(`B07_H02_EVIDENCE_MISSING:${gatesPath}`)
  const gates = JSON.parse(readFileSync(gatesPath, 'utf8'))
  const acceptance = { ...evaluateAcceptance(gates), generatedAt: new Date().toISOString(), generator: 'scripts/generate-b07-h02-acceptance.mjs' }
  writeFileSync(resolve(evidenceDir, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const result = generateAcceptance()
  console.log(`B07_H02 acceptance generated: ${result.status}`)
  if (result.status !== 'PASS') process.exitCode = 1
}
