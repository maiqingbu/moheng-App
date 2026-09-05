import test from 'node:test'
import assert from 'node:assert/strict'
import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { REQUIRED_R2_GATES, evaluateR2Acceptance } from '../scripts/generate-b13-r2-acceptance.mjs'
import { validatePackageTree } from '../scripts/verify-package-consistency.mjs'

test('B13-R2 acceptance fails closed when a blocking gate is missing', () => {
  const sourceCommit = 'a'.repeat(40)
  const report = {
    sourceCommit,
    evidenceSourceCommit: sourceCommit,
    sourceCommitAfter: sourceCommit,
    workingTreeDirtyBefore: false,
    workingTreeDirtyAfter: false,
    excludedUncommittedPaths: [],
    gates: Object.fromEntries(REQUIRED_R2_GATES.map(name => [name, { status: 'PASS', exitCode: 0 }])),
  }
  assert.equal(evaluateR2Acceptance(report).status, 'PASS')
  const missing = { ...report, gates: { ...report.gates } }
  delete missing.gates.formal_owner_integration
  assert.equal(evaluateR2Acceptance(missing).status, 'FAIL')
})

test('package consistency rejects an inventory count mutation', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'b13-r2-package-test-'))
  const manifest = path.join(root, 'PACKAGE_MANIFEST')
  fs.mkdirSync(path.join(root, 'SOURCE'), { recursive: true })
  fs.mkdirSync(path.join(root, 'FROZEN_INPUTS'), { recursive: true })
  fs.mkdirSync(path.join(root, 'EVIDENCE', 'B13_R2', 'MANUAL_BROWSER'), { recursive: true })
  fs.mkdirSync(manifest, { recursive: true })
  fs.writeFileSync(path.join(root, 'SOURCE', 'source.txt'), 'source\n')
  fs.writeFileSync(path.join(root, 'FROZEN_INPUTS', 'input.txt'), 'input\n')
  fs.writeFileSync(path.join(root, 'EVIDENCE', 'B13_R2', 'evidence.txt'), 'evidence\n')
  fs.writeFileSync(path.join(manifest, 'FORBIDDEN_SCOPE_DIFF.json'), JSON.stringify({ status: 'PASS', findings: [] }))
  fs.writeFileSync(path.join(manifest, 'PACKAGE_UTF8_VALIDATION.json'), JSON.stringify({ status: 'PASS', rootAscii: true }))
  const commit = 'b'.repeat(40)
  fs.writeFileSync(path.join(manifest, 'SOURCE_BINDING.json'), JSON.stringify({ sourceCommit: commit, evidenceSourceCommit: commit, sourceCommitAfter: commit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false }))
  const checksum = path.join(manifest, 'FILES_SHA256SUMS.txt')
  fs.writeFileSync(path.join(manifest, 'PACKAGE_INVENTORY.json'), '{}\n')
  const ordinary = []
  const walk = directory => {
    for (const name of fs.readdirSync(directory)) {
      const file = path.join(directory, name)
      if (fs.statSync(file).isDirectory()) walk(file)
      else if (file !== checksum) ordinary.push(file)
    }
  }
  walk(root)
  const rel = file => path.relative(root, file).split(path.sep).join('/')
  const count = prefix => ordinary.filter(file => rel(file).startsWith(prefix)).length
  fs.writeFileSync(path.join(manifest, 'PACKAGE_INVENTORY.json'), JSON.stringify({
    totalFilesExcludingChecksums: ordinary.length,
    sourceFiles: count('SOURCE/'), frozenInputFiles: count('FROZEN_INPUTS/'), evidenceFiles: count('EVIDENCE/'), manifestFiles: count('PACKAGE_MANIFEST/'), manualBrowserFiles: 0, mutationFiles: 0, performanceFiles: 0, forbiddenFilesFound: 0, countingRule: 'test'
  }))
  const lines = ordinary.sort((left, right) => rel(left).localeCompare(rel(right))).map(file => `${crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')}  ${rel(file)}`)
  fs.writeFileSync(checksum, `${lines.join('\n')}\n`)
  const good = validatePackageTree(root, commit)
  assert.equal(good.status, 'PASS', JSON.stringify(good))
  const inventoryPath = path.join(manifest, 'PACKAGE_INVENTORY.json')
  const inventory = JSON.parse(fs.readFileSync(inventoryPath, 'utf8'))
  inventory.totalFilesExcludingChecksums += 1
  fs.writeFileSync(inventoryPath, JSON.stringify(inventory))
  assert.equal(validatePackageTree(root, commit).status, 'FAIL')
})
