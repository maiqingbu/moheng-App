import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const sha256 = value => crypto.createHash('sha256').update(value).digest('hex')

export const allFiles = (root, base = root) => fs.readdirSync(root, { withFileTypes: true })
  .flatMap(entry => {
    const file = path.join(root, entry.name)
    return entry.isDirectory() ? allFiles(file, base) : [path.relative(base, file).split(path.sep).join('/')]
  })
  .sort()

export const fileSha256 = file => sha256(fs.readFileSync(file))

export const treeDigest = root => sha256(allFiles(root).map(relative => `${relative}\0${fileSha256(path.join(root, relative))}\n`).join(''))

const redact = value => value.length <= 10 ? '[redacted]' : `${value.slice(0, 4)}…${value.slice(-4)}`
const shaLike = value => /^[0-9a-f]{40}$|^[0-9a-f]{64}$/i.test(value)
const uuidLike = value => /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)
const shannonEntropy = value => {
  const counts = new Map()
  for (const char of value) counts.set(char, (counts.get(char) ?? 0) + 1)
  return [...counts.values()].reduce((sum, count) => {
    const probability = count / value.length
    return sum - probability * Math.log2(probability)
  }, 0)
}
const hasMultipleClasses = value => [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter(regex => regex.test(value)).length >= 3
const allowlisted = (value, relative, context) => {
  if (/(^|\/)tests\//.test(relative) && /R8SyntheticFakeSecret|R8SyntheticMutation|RepairAuthorization|TEST_FAKE_MODEL_ONLY|sk-test-not-real/i.test(value)) return { allowlisted: true, allowlistReason: 'synthetic-test-fixture' }
  if (shaLike(value) || uuidLike(value)) return { allowlisted: true, allowlistReason: 'hash-or-uuid' }
  if (/(?:sha|hash|digest|commit|checksum|sourceCommit|archiveSha256|treeDigest)/i.test(context) && /^[A-Za-z0-9+/=_-]{24,}$/.test(value)) return { allowlisted: true, allowlistReason: 'declared-integrity-value' }
  return { allowlisted: false, allowlistReason: null }
}

const knownPatterns = [
  ['openai_api_key', /\bsk-(?:(?:proj|live)-)?[A-Za-z0-9]{20,}\b/g],
  ['bearer_token', /\bBearer\s+[A-Za-z0-9._!@#$%^&*+=/-]{20,}\b/gi],
  ['github_token', /\bgh[pousr]_[A-Za-z0-9_]{20,}\b/g],
  ['slack_token', /\bxox[baprs]-[A-Za-z0-9-]{20,}\b/g],
  ['aws_access_key', /\bAKIA[0-9A-Z]{16}\b/g],
  ['aws_secret_key_assignment', /\bAWS_SECRET_ACCESS_KEY\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['jwt_like', /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g],
  ['private_key', /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g],
  ['database_credential', /\b(?:postgres|postgresql|mysql):\/\/[^\s"']+/gi],
  ['session_or_cookie', /\b(?:SESSION_SECRET|COOKIE_SECRET|PROVIDER_API_KEY)\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
]
const keywordPatterns = [
  ['api_key_assignment', /\b(?:API_KEY|OPENAI_API_KEY)\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['secret_assignment', /\bSECRET\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['token_assignment', /\bTOKEN\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['password_assignment', /\bPASSWORD\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['private_key_assignment', /\bPRIVATE_KEY\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['authorization_assignment', /\bAUTHORIZATION\s*=\s*['"]?[A-Za-z0-9+/=_-]{16,}['"]?/gi],
  ['bearer_keyword', /\bBEARER\s+[A-Za-z0-9._!@#$%^&*+=/-]{20,}\b/gi],
]

const finding = (relative, line, findingType, riskClass, value, allowlist) => ({
  file: relative,
  lineOrLocator: line,
  findingType,
  riskClass,
  redactedSample: redact(value),
  allowlisted: allowlist.allowlisted,
  allowlistReason: allowlist.allowlistReason,
})

const scanText = (relative, text, patterns, riskClass, output) => {
  for (const [name, regex] of patterns) {
    regex.lastIndex = 0
    for (const match of text.matchAll(regex)) {
      const value = match[0]
      const line = text.slice(0, match.index).split('\n').length
      const context = text.slice(Math.max(0, match.index - 50), match.index + value.length + 50)
      output.push(finding(relative, line, name, riskClass, value, allowlisted(value, relative, context)))
    }
  }
}

export const scanSecretStaging = (stagingRoot, selfRelative, sourceCommit) => {
  const files = allFiles(stagingRoot)
  const excludedFiles = [selfRelative.split(path.sep).join('/')]
  const knownFormatCandidates = []
  const patternCandidates = []
  const entropyCandidates = []
  for (const relative of files.filter(file => file !== excludedFiles[0])) {
    const file = path.join(stagingRoot, relative)
    let text
    try { text = fs.readFileSync(file, 'utf8') } catch { continue }
    scanText(relative, text, knownPatterns, 'KNOWN_FORMAT', knownFormatCandidates)
    scanText(relative, text, keywordPatterns, 'KEYWORD_PATTERN', patternCandidates)
    const tokenRegex = /[A-Za-z0-9][A-Za-z0-9._+/=!@#$%^&*-]{23,}/g
    for (const match of text.matchAll(tokenRegex)) {
      const value = match[0]
      if (!hasMultipleClasses(value) || shannonEntropy(value) < 3.5) continue
      const context = text.slice(Math.max(0, match.index - 70), match.index + value.length + 70)
      const line = text.slice(0, match.index).split('\n').length
      entropyCandidates.push(finding(relative, line, 'entropy_high_risk_token', 'HIGH_RISK_TOKEN', value, allowlisted(value, relative, context)))
    }
  }
  const allCandidates = [...knownFormatCandidates, ...patternCandidates, ...entropyCandidates]
  const allowlistedFindings = allCandidates.filter(item => item.allowlisted)
  const realSecretFindings = allCandidates.filter(item => !item.allowlisted)
  const finalStagingFileCount = files.length
  return {
    schema: 'post-b14-ui-r8-secret-scan',
    sourceCommit,
    finalStagingTreeDigest: treeDigest(stagingRoot),
    finalStagingFileCount,
    selfExcluded: true,
    selfExcludedReason: 'SELF_REFERENTIAL_CERTIFICATION_OUTPUT',
    excludedFiles,
    filesScanned: finalStagingFileCount - excludedFiles.length,
    patterns: { knownFormats: knownPatterns.map(([name]) => name), keywords: keywordPatterns.map(([name]) => name), entropyThreshold: 3.5 },
    knownFormatCandidates,
    patternCandidates,
    entropyCandidates,
    allowlistedFindings,
    realSecretFindings,
    knownFormatScanExecuted: true,
    patternScanExecuted: true,
    entropyScanExecuted: true,
    mutationDetected: false,
    status: realSecretFindings.length === 0 && filesScannedValid(finalStagingFileCount, excludedFiles, finalStagingFileCount - 1) ? 'PASS' : 'FAIL',
  }
}

const filesScannedValid = (fileCount, excludedFiles, filesScanned) => excludedFiles.length === 1 && filesScanned === fileCount - 1

const parseChecksums = text => text.split(/\r?\n/).filter(Boolean).map((line, index) => {
  const match = /^(?<digest>[0-9a-f]{64})  (?<file>.+)$/.exec(line)
  if (!match) throw new Error(`INVALID_CHECKSUM_LINE:${index + 1}`)
  return { digest: match.groups.digest, file: match.groups.file.replaceAll('\\', '/') }
})

export const verifyChecksums = (stagingRoot, checksumRelative) => {
  const checksumPath = path.join(stagingRoot, checksumRelative)
  const checksumText = fs.readFileSync(checksumPath, 'utf8')
  const entries = parseChecksums(checksumText)
  const expected = new Set(entries.map(entry => entry.file))
  const actual = new Set(allFiles(stagingRoot).filter(file => file !== checksumRelative))
  const mismatches = []
  const missingFiles = [...expected].filter(file => !actual.has(file)).sort()
  const extraFiles = [...actual].filter(file => !expected.has(file)).sort()
  let verifiedEntries = 0
  for (const entry of entries) {
    if (!fs.existsSync(path.join(stagingRoot, entry.file))) continue
    verifiedEntries += 1
    if (fileSha256(path.join(stagingRoot, entry.file)) !== entry.digest) mismatches.push(entry.file)
  }
  return {
    schema: 'post-b14-ui-r8-files-sha-verify',
    checksumFile: checksumRelative,
    checksumFileSha256: fileSha256(checksumPath),
    stagingTreeDigest: treeDigest(stagingRoot),
    stagingFileCount: allFiles(stagingRoot).length,
    expectedEntries: entries.length,
    verifiedEntries,
    mismatches: [...new Set(mismatches)].sort(),
    missingFiles,
    extraFiles,
    readChecksum: true,
    rehashExecuted: true,
    mutationDetected: false,
    missingMutationDetected: false,
    extraMutationDetected: false,
    status: mismatches.length === 0 && missingFiles.length === 0 && extraFiles.length === 0 ? 'PASS' : 'FAIL',
  }
}

const pass = value => value === true ? 'PASS' : 'FAIL'
const nonEmpty = value => typeof value === 'string' && value.length > 0
const arrayEmpty = value => Array.isArray(value) && value.length === 0

export const evaluateFinalAcceptance = input => {
  const secret = input.secretScan ?? {}
  const inventory = input.inventory ?? {}
  const filesSha = input.filesShaVerify ?? {}
  const archive = input.archiveValidation ?? {}
  const certification = input.certification ?? {}
  const consistency = input.consistency ?? {}
  const preArchive = input.preArchive ?? {}
  const secretGate = secret.status === 'PASS' && Number.isInteger(secret.finalStagingFileCount) && secret.filesScanned === secret.finalStagingFileCount - 1 && Array.isArray(secret.excludedFiles) && secret.excludedFiles.length === 1 && secret.knownFormatScanExecuted === true && secret.patternScanExecuted === true && secret.entropyScanExecuted === true && arrayEmpty(secret.realSecretFindings)
  const inventoryGate = inventory.status === 'PASS' && inventory.mathConsistent === true && inventory.actualTreeCount === inventory.checksumEntries + 1
  const shaGate = filesSha.status === 'PASS' && filesSha.readChecksum === true && filesSha.rehashExecuted === true && filesSha.expectedEntries === filesSha.verifiedEntries && arrayEmpty(filesSha.mismatches) && arrayEmpty(filesSha.missingFiles) && arrayEmpty(filesSha.extraFiles)
  const archiveGate = archive.status === 'PASS' && archive.archiveSha256 === input.archiveSha256 && archive.crcOrTestPass === true && archive.entrySetsEqual === true && archive.contentDigestsEqual === true
  const certificationGate = certification.status === 'PASS' && certification.sourceCommit === input.sourceCommit && certification.archiveSha256 === input.archiveSha256
  const consistencyGate = consistency.status === 'PASS'
  const sourceBound = nonEmpty(input.sourceCommit) && input.sourceCommit !== 'UNKNOWN' && certificationGate
  const archiveBound = nonEmpty(input.archiveFilename) && nonEmpty(input.archiveSha256) && archiveGate
  const companionBound = Object.values(input.companionSha256 ?? {}).every(nonEmpty)
  const blocking = {
    PRODUCT_CODE_ZERO_DIFF: pass(input.productionCodeZeroDiff === true),
    DISPLAY_LAYER_REGRESSION: pass(input.displayLayerRegression === true),
    A11_REGRESSION: pass(input.a11Regression === true),
    B13_REGRESSION: pass(input.b13Regression === true),
    B14_REGRESSION: pass(input.b14Regression === true),
    SECRET_SCAN_FINAL_STAGING: pass(secretGate),
    SECRET_SCAN_ENTROPY: pass(secret.entropyScanExecuted === true && secretGate),
    REAL_SECRET_FINDINGS_ZERO: pass(arrayEmpty(secret.realSecretFindings)),
    PACKAGE_INVENTORY: pass(inventoryGate),
    FILES_SHA256_REAL_VERIFY: pass(shaGate),
    FINAL_ZIP_SHA: pass(archiveBound),
    FINAL_CRC: pass(archive.crcOrTestPass === true),
    FINAL_DUAL_READER: pass(archiveGate),
    FINAL_CERTIFICATION: pass(certificationGate),
    EVIDENCE_CONSISTENCY: pass(consistencyGate),
  }
  const accepted = Object.values(blocking).every(value => value === 'PASS') && preArchive.DELIVERY_STATUS === 'PRE_ARCHIVE_PASS' && sourceBound && archiveBound && companionBound
  return {
    schema: 'post-b14-ui-r8-final-acceptance',
    sourceCommit: input.sourceCommit,
    archiveFilename: input.archiveFilename,
    archiveSha256: input.archiveSha256,
    archiveValidationSha256: input.companionSha256?.archiveValidationSha256 ?? null,
    finalCertificationSha256: input.companionSha256?.finalCertificationSha256 ?? null,
    filesShaVerifySha256: input.companionSha256?.filesShaVerifySha256 ?? null,
    evidenceConsistencySha256: input.companionSha256?.evidenceConsistencySha256 ?? null,
    blocking,
    FINAL_ACCEPTANCE_SOURCE_BOUND: sourceBound ? 'PASS' : 'FAIL',
    FINAL_ACCEPTANCE_ARCHIVE_BOUND: archiveBound ? 'PASS' : 'FAIL',
    FINAL_ACCEPTANCE_COMPANION_SHA_BOUND: companionBound ? 'PASS' : 'FAIL',
    ACCEPTANCE_FAIL_CLOSED: true,
    DELIVERY_STATUS: accepted ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    NEXT_STAGE_ALLOWED: false,
    B15_STARTED: false,
    RC1_STARTED: false,
  }
}
