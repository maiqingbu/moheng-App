import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const formalRoots = ['src', 'src-tauri/src', 'contracts', 'src-tauri/migrations']
const forbiddenPattern = /\b(?:B14|B15|RC1)::|EndgameOwner|CompletionGate|ExportReleaseOwner|BookExitState|ending readiness|completion readiness/i

function gitDiff(projectRoot, baseline) {
  try {
    return execFileSync('git', ['diff', '--unified=0', baseline, '--', ...formalRoots], {
      cwd: projectRoot,
      encoding: 'utf8',
    })
  } catch (error) {
    return error.stdout?.toString() ?? ''
  }
}

export function evaluateForbiddenScopeDiff(projectRoot = root, baseline = 'HEAD') {
  const diff = gitDiff(projectRoot, baseline)
  const findings = []
  let currentFile = null
  let currentLine = 0
  for (const line of diff.split('\n')) {
    const fileMatch = line.match(/^\+\+\+ b\/(.+)$/)
    if (fileMatch) {
      currentFile = fileMatch[1]
      continue
    }
    const hunk = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)/)
    if (hunk) {
      currentLine = Number(hunk[1])
      continue
    }
    if (!line.startsWith('+') || line.startsWith('+++')) continue
    const text = line.slice(1)
    if (forbiddenPattern.test(text)) {
      findings.push({ file: currentFile, line: currentLine, text: text.trim(), reason: 'future batch or owner marker added to formal source' })
    }
    currentLine += 1
  }
  return {
    schema: 'b13-r2-forbidden-scope-diff',
    baseline,
    scannedRoots: formalRoots,
    excludedFromScan: ['scripts/verify-b13-r2-forbidden-scope.mjs', 'tests/negative-fixtures', 'docs', 'FROZEN_INPUTS', 'EVIDENCE', 'mutation patches'],
    findings,
    status: findings.length === 0 ? 'PASS' : 'FAIL',
  }
}

if (process.argv[1]?.endsWith('verify-b13-r2-forbidden-scope.mjs')) {
  const baseline = process.env.B13_R2_REPAIR_BASELINE ?? process.argv[2] ?? 'HEAD'
  const result = evaluateForbiddenScopeDiff(root, baseline)
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}
