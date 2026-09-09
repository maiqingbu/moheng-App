import test from 'node:test'
import assert from 'node:assert/strict'
import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import {
  evaluateFinalAcceptance,
  scanSecretStaging,
  verifyChecksums,
} from '../../scripts/post-b14-ui-r8-certification.mjs'

const sha = value => crypto.createHash('sha256').update(value).digest('hex')
const makeTempStage = () => fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r8-test-'))
const write = (root, relative, value) => {
  const file = path.join(root, relative)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, value)
}

test('R8 secret scan covers staging and catches a synthetic credential mutation', () => {
  const stage = makeTempStage()
  write(stage, 'README.md', '本地认证说明，不包含真实凭据。\n')
  write(stage, 'EVIDENCE/SECRET_SCAN_R8.json', '{}\n')

  const clean = scanSecretStaging(stage, 'EVIDENCE/SECRET_SCAN_R8.json', 'source')
  assert.equal(clean.filesScanned, clean.finalStagingFileCount - 1)
  assert.deepEqual(clean.excludedFiles, ['EVIDENCE/SECRET_SCAN_R8.json'])
  assert.equal(clean.realSecretFindings.length, 0)
  assert.equal(clean.status, 'PASS')

  write(stage, 'EVIDENCE/MUTATION.txt', 'Authorization: Bearer R8SyntheticFakeSecret-9aB7cD8eF1gH2jK3\n')
  const mutation = scanSecretStaging(stage, 'EVIDENCE/SECRET_SCAN_R8.json', 'source')
  assert.ok(mutation.realSecretFindings.length > 0)
  assert.equal(mutation.status, 'FAIL')
})

test('R8 checksum verifier reads checksum entries and rejects mismatch, missing, and extra files', () => {
  const stage = makeTempStage()
  write(stage, 'a.txt', 'alpha')
  write(stage, 'nested/b.txt', 'bravo')
  const checksumFile = path.join(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
  write(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt', `${sha('alpha')}  a.txt\n${sha('bravo')}  nested/b.txt\n`)

  const clean = verifyChecksums(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
  assert.equal(clean.status, 'PASS')
  assert.equal(clean.expectedEntries, 2)
  assert.equal(clean.verifiedEntries, 2)
  assert.deepEqual(clean.mismatches, [])

  write(stage, 'a.txt', 'changed')
  const mismatch = verifyChecksums(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
  assert.equal(mismatch.status, 'FAIL')
  assert.deepEqual(mismatch.missingFiles, [])
  assert.ok(mismatch.mismatches.includes('a.txt'))

  fs.writeFileSync(path.join(stage, 'a.txt'), 'alpha')
  fs.unlinkSync(path.join(stage, 'nested/b.txt'))
  const missing = verifyChecksums(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
  assert.equal(missing.status, 'FAIL')
  assert.deepEqual(missing.missingFiles, ['nested/b.txt'])

  write(stage, 'nested/b.txt', 'bravo')
  write(stage, 'extra.txt', 'extra')
  const extra = verifyChecksums(stage, 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
  assert.equal(extra.status, 'FAIL')
  assert.deepEqual(extra.extraFiles, ['extra.txt'])
  assert.ok(fs.existsSync(checksumFile))
})

test('R8 final acceptance is fail-closed and source/archive/companion bound', () => {
  const common = {
    sourceCommit: 'final-source',
    archiveFilename: 'final.zip',
    archiveSha256: 'archive-sha',
    productionCodeZeroDiff: true,
    displayLayerRegression: true,
    a11Regression: true,
    b13Regression: true,
    b14Regression: true,
    archiveValidation: { status: 'PASS', archiveSha256: 'archive-sha', crcOrTestPass: true, entrySetsEqual: true, contentDigestsEqual: true },
    certification: { status: 'PASS', sourceCommit: 'final-source', archiveSha256: 'archive-sha' },
    consistency: { status: 'PASS' },
    secretScan: { status: 'PASS', finalStagingFileCount: 3, filesScanned: 2, excludedFiles: ['EVIDENCE/SECRET_SCAN_R8.json'], knownFormatScanExecuted: true, patternScanExecuted: true, entropyScanExecuted: true, realSecretFindings: [] },
    inventory: { status: 'PASS', actualTreeCount: 3, checksumEntries: 2, mathConsistent: true },
    filesShaVerify: { status: 'PASS', expectedEntries: 2, verifiedEntries: 2, mismatches: [], missingFiles: [], extraFiles: [], readChecksum: true, rehashExecuted: true },
    preArchive: { DELIVERY_STATUS: 'PRE_ARCHIVE_PASS' },
    companionSha256: { archiveValidationSha256: 'validation-sha', finalCertificationSha256: 'cert-sha', filesShaVerifySha256: 'files-sha', evidenceConsistencySha256: 'consistency-sha' },
  }
  const accepted = evaluateFinalAcceptance(common)
  assert.equal(accepted.DELIVERY_STATUS, 'READY_FOR_FORMAL_ACCEPTANCE')
  assert.equal(accepted.NEXT_STAGE_ALLOWED, false)

  const rejected = evaluateFinalAcceptance({ ...common, filesShaVerify: { ...common.filesShaVerify, mismatches: ['a.txt'] } })
  assert.equal(rejected.DELIVERY_STATUS, 'FAIL')
  assert.equal(rejected.NEXT_STAGE_ALLOWED, false)
})
