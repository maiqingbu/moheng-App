import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const REQUIRED_GATES = ['ALIGN-A01','ALIGN-A02','ALIGN-A03','ALIGN-A04','ALIGN-A05','ALIGN-A06','ALIGN-A07','ALIGN-A08','migration_freeze','architecture','module_boundary','b05_regression','b06_h01_regression','b07_h02_regression','b08_h03_regression','node','vitest','contracts','typecheck','build','m0a','cargo_all_targets','cargo_fmt','cargo_check','cargo_clippy','source_binding','package_smoke']
export function evaluate(report) {
  const missing = REQUIRED_GATES.filter(id => report.gates?.[id]?.status !== 'PASS')
  return { schema: 'post-b08-h03-r2-alignment-acceptance-r1', status: missing.length === 0 && report.sourceBinding === 'PASS' ? 'PASS' : 'FAIL', sourceCommit: report.sourceCommit ?? null, evidenceSourceCommit: report.evidenceSourceCommit ?? null, sourceCommitAfter: report.sourceCommitAfter ?? null, sourceBinding: report.sourceBinding === 'PASS', requiredGates: REQUIRED_GATES, missingGates: missing }
}
if (process.argv[1]?.endsWith('generate-r2-alignment-acceptance.mjs')) {
  const evidence = process.env.R2_ALIGNMENT_EVIDENCE_DIR ?? path.join(root, 'evidence/ALIGN')
  const report = JSON.parse(fs.readFileSync(path.join(evidence, 'alignment-gates.json'), 'utf8'))
  const acceptance = { ...evaluate(report), generatedAt: new Date().toISOString() }
  fs.writeFileSync(path.join(evidence, 'acceptance.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  console.log(`R2 alignment acceptance: ${acceptance.status}; missing=${acceptance.missingGates.length}`)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
