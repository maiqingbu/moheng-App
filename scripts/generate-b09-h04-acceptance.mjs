import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const B09_BLOCKING_GATES = Array.from({ length: 22 }, (_, index) => `B09-A${String(index + 1).padStart(2, '0')}`)
export const H04_BLOCKING_GATES = Array.from({ length: 12 }, (_, index) => `H04-A${String(index + 1).padStart(2, '0')}`)
export const GLOBAL_BLOCKING_GATES = [
  'baseline', 'no_second_engine', 'migration_freeze', 'architecture', 'module_boundary',
  'contracts', 'node', 'vitest', 'typecheck', 'build', 'cargo_all_targets', 'cargo_fmt',
  'cargo_check', 'cargo_clippy', 'b02_regression', 'b03_regression', 'b05_regression',
  'b06_h01_regression', 'b07_h02_regression', 'b08_h03_regression', 'p01_regression',
  'source_binding', 'package_smoke',
]
export const REQUIRED_BLOCKING_GATES = [...B09_BLOCKING_GATES, ...H04_BLOCKING_GATES, ...GLOBAL_BLOCKING_GATES]

function sourceBindingPass(report) {
  const binding = report.sourceBinding
  return binding === 'PASS'
    && typeof report.sourceCommit === 'string'
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
}

export function evaluateAcceptance(report) {
  const gates = report?.gates ?? {}
  const missingGates = REQUIRED_BLOCKING_GATES.filter(name => gates[name]?.status !== 'PASS')
  const sourceBinding = sourceBindingPass(report ?? {})
  return {
    schema: 'b09-h04-acceptance-r1',
    batch: 'B09_H04',
    milestone: 'M0E Cross-volume lifecycle + H04 certification',
    status: missingGates.length === 0 && sourceBinding ? 'READY_FOR_ACCEPTANCE' : 'NOT_READY',
    sourceCommit: report?.sourceCommit ?? null,
    evidenceSourceCommit: report?.evidenceSourceCommit ?? null,
    sourceCommitAfter: report?.sourceCommitAfter ?? null,
    sourceBinding,
    requiredBlockingGates: REQUIRED_BLOCKING_GATES,
    missingGates,
    excludedUncommittedPaths: report?.excludedUncommittedPaths ?? [],
  }
}

export function generateAcceptance(evidenceDir = process.env.B09_H04_EVIDENCE_DIR ?? path.join(root, 'evidence/B09_H04')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error(`B09_H04_EVIDENCE_MISSING:${reportPath}`)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateAcceptance(report),
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b09-h04-acceptance.mjs',
  }
  fs.mkdirSync(evidenceDir, { recursive: true })
  fs.writeFileSync(path.join(evidenceDir, 'B09_H04_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b09-h04-acceptance.mjs')) {
  const result = generateAcceptance()
  console.log(`B09_H04 acceptance generated: ${result.status}; missing=${result.missingGates.length}`)
  if (result.status !== 'READY_FOR_ACCEPTANCE') process.exitCode = 1
}
