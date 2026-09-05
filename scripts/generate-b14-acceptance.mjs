import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

export const REQUIRED_B14_GATES = [
  'b14_deterministic_gate',
  'b14_atomic_completion',
  'b14_contracts',
  'b14_architecture',
  'b14_ui_surface',
  'b14_rust_quality',
  'b01_b13_regression',
  'b14_historical_evidence',
  'b14_source_binding',
  'b15_boundary',
]

function clean(value) { return value === false || value === 0 ? value : value }

export function evaluateB14Acceptance(report) {
  const missingGates = REQUIRED_B14_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const exactBinding = report.sourceCommit
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
  const status = missingGates.length === 0 && exactBinding ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, exactSourceBinding: Boolean(exactBinding), nextBatchAllowed: false, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL' }
}

export function fixedB14Report(acceptance) {
  return {
    BATCH: 'B14',
    STATUS: acceptance.status,
    SOURCE_COMMIT: acceptance.sourceCommit ?? null,
    EVIDENCE_SOURCE_COMMIT: acceptance.evidenceSourceCommit ?? null,
    SOURCE_COMMIT_AFTER: acceptance.sourceCommitAfter ?? null,
    WORKING_TREE_DIRTY_BEFORE: clean(acceptance.workingTreeDirtyBefore),
    WORKING_TREE_DIRTY_AFTER: clean(acceptance.workingTreeDirtyAfter),
    MISSING_GATES: acceptance.missingGates,
    NEXT_BATCH_ALLOWED: false,
  }
}

function rootDir() { return path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..') }
function git(root, args) { return spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim() }

export function generateB14Acceptance(evidenceDir = process.env.B14_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b14-evidence-latest')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = evaluateB14Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_ACCEPTANCE.json'), `${JSON.stringify(acceptance, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B14_FINAL_REPORT.txt'), `${Object.entries(fixedB14Report(acceptance)).map(([key, value]) => `${key} = ${typeof value === 'string' ? value : JSON.stringify(value)}`).join('\n')}\n`)
  return acceptance
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const evidenceDir = process.env.B14_EVIDENCE_DIR
  if (!evidenceDir) throw new Error('B14_EVIDENCE_DIR_REQUIRED')
  const acceptance = generateB14Acceptance(evidenceDir)
  console.log(JSON.stringify(acceptance, null, 2))
  if (acceptance.status !== 'PASS') process.exitCode = 1
}
