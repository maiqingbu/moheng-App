import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const REQUIRED_BLOCKING_GATES = [
  ...Array.from({length:12},(_,index)=>`B08-F${String(index+1).padStart(2,'0')}`),
  ...Array.from({length:9},(_,index)=>`B08-I${String(index+1).padStart(2,'0')}`),
  ...Array.from({length:11},(_,index)=>`H03-A${String(index+1).padStart(2,'0')}`),
  ...Array.from({length:4},(_,index)=>`H03-I${String(index+1).padStart(2,'0')}`),
  'basis','migration','architecture','module_boundary','contracts','node','vitest','typecheck','build',
  'b07_h02_regression','b06_h01_regression','b05_regression','m0_regression',
  'cargo_all_targets','cargo_fmt','cargo_check','cargo_clippy','source_binding','package_smoke',
]

export function evaluateAcceptance(report) {
  const gates = report.gates ?? {}
  const missing = REQUIRED_BLOCKING_GATES.filter(id => gates[id]?.status !== 'PASS')
  const sourceBinding = report.sourceBinding === 'PASS'
  return {
    schema:'b08-h03-acceptance-r2', batch:'B08_H03',
    status:missing.length === 0 && sourceBinding ? 'PASS' : 'FAIL',
    sourceCommit:report.sourceCommit ?? null, sourceCommitAfter:report.sourceCommitAfter ?? null,
    sourceBinding, requiredBlockingGates:REQUIRED_BLOCKING_GATES,
    missingBlockingGates:missing,
  }
}

export function generateAcceptance(evidenceDir = process.env.B08_H03_EVIDENCE_DIR ?? path.join(root,'evidence/B08_H03')) {
  const reportPath = path.join(evidenceDir,'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B08_H03_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath,'utf8'))
  const acceptance = {...evaluateAcceptance(report),generatedAt:new Date().toISOString(),generator:'scripts/generate-b08-h03-acceptance.mjs'}
  fs.writeFileSync(path.join(evidenceDir,'acceptance.json'),`${JSON.stringify(acceptance,null,2)}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b08-h03-acceptance.mjs')) {
  const result = generateAcceptance()
  console.log(`B08_H03 acceptance generated: ${result.status}; missing=${result.missingBlockingGates.length}`)
  if (result.status !== 'PASS') process.exitCode = 1
}
