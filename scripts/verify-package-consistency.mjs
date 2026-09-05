import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const checksumRelative = 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt'
const forbiddenPattern = /(?:^|\/)(?:node_modules|target|dist|coverage|\.git)(?:\/|$)|(?:^|\/)(?:\.env(?:\.|$)|.*\.db(?:-shm|-wal)?$|core\.[0-9]+$)/

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function walkFiles(directory) {
  const files = []
  const walk = current => {
    for (const name of fs.readdirSync(current).sort((left, right) => left.localeCompare(right))) {
      const file = path.join(current, name)
      if (fs.statSync(file).isDirectory()) walk(file)
      else files.push(file)
    }
  }
  walk(directory)
  return files
}

function relativeFile(stageRoot, file) {
  return path.relative(stageRoot, file).split(path.sep).join('/')
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'))
  } catch {
    return null
  }
}

function checkInternalSums(stageRoot) {
  const sumsPath = path.join(stageRoot, checksumRelative)
  const files = walkFiles(stageRoot)
  const ordinary = files
    .map(file => relativeFile(stageRoot, file))
    .filter(relative => relative !== checksumRelative)
    .sort((left, right) => left.localeCompare(right))
  if (!fs.existsSync(sumsPath)) {
    return { status: 'FAIL', entryCount: 0, expectedCount: ordinary.length, mismatches: ['checksum file missing'] }
  }
  const entries = fs.readFileSync(sumsPath, 'utf8').split(/\r?\n/).filter(Boolean).map(line => {
    const match = line.match(/^([a-f0-9]{64})  (.+)$/)
    return match ? { digest: match[1], relative: match[2] } : { digest: null, relative: line }
  })
  const byRelative = new Map(entries.map(entry => [entry.relative, entry]))
  const mismatches = []
  for (const relative of ordinary) {
    const entry = byRelative.get(relative)
    if (!entry) mismatches.push(`missing checksum: ${relative}`)
    else if (entry.digest !== sha256(path.join(stageRoot, relative))) mismatches.push(`digest mismatch: ${relative}`)
  }
  for (const entry of entries) {
    if (!ordinary.includes(entry.relative)) mismatches.push(`unexpected checksum entry: ${entry.relative}`)
  }
  return {
    status: mismatches.length === 0 && entries.length === ordinary.length ? 'PASS' : 'FAIL',
    entryCount: entries.length,
    expectedCount: ordinary.length,
    mismatches,
  }
}

export function validatePackageTree(stageRoot, expectedSourceCommit = null) {
  const resolvedRoot = path.resolve(stageRoot)
  const failures = []
  const allFiles = fs.existsSync(resolvedRoot) ? walkFiles(resolvedRoot) : []
  const relativeFiles = allFiles.map(file => relativeFile(resolvedRoot, file)).sort((left, right) => left.localeCompare(right))
  const ordinary = relativeFiles.filter(relative => relative !== checksumRelative)
  const forbiddenFiles = relativeFiles.filter(relative => forbiddenPattern.test(relative))
  const rootName = path.basename(resolvedRoot)
  const utf8Names = relativeFiles.filter(relative => relative.includes('\ufffd'))
  if (!/^[\x00-\x7F]+$/.test(rootName)) failures.push('package root is not ASCII')
  if (utf8Names.length > 0) failures.push('replacement character in package filename')
  if (forbiddenFiles.length > 0) failures.push('forbidden package files present')

  const inventoryPath = path.join(resolvedRoot, 'PACKAGE_MANIFEST/PACKAGE_INVENTORY.json')
  const inventory = readJson(inventoryPath)
  const sourceBinding = readJson(path.join(resolvedRoot, 'PACKAGE_MANIFEST/SOURCE_BINDING.json'))
  const forbiddenScope = readJson(path.join(resolvedRoot, 'PACKAGE_MANIFEST/FORBIDDEN_SCOPE_DIFF.json'))
  const utf8Validation = readJson(path.join(resolvedRoot, 'PACKAGE_MANIFEST/PACKAGE_UTF8_VALIDATION.json'))
  const batch = fs.existsSync(path.join(resolvedRoot, 'EVIDENCE/B13_R3')) ? 'B13_R3' : 'B13_R2'
  if (!inventory || typeof inventory.countingRule !== 'string') failures.push('inventory counting rule missing')
  const count = prefix => ordinary.filter(relative => relative.startsWith(prefix)).length
  const expectedInventory = {
    totalFilesExcludingChecksums: ordinary.length,
    sourceFiles: count('SOURCE/'),
    frozenInputFiles: count('FROZEN_INPUTS/'),
    evidenceFiles: count('EVIDENCE/'),
    manifestFiles: count('PACKAGE_MANIFEST/'),
    manualBrowserFiles: count(`EVIDENCE/${batch}/MANUAL_BROWSER/`),
    mutationFiles: count(`EVIDENCE/${batch}/MUTATIONS/`),
    performanceFiles: count(`EVIDENCE/${batch}/PERFORMANCE/`),
    forbiddenFilesFound: forbiddenFiles.length,
  }
  for (const [key, value] of Object.entries(expectedInventory)) {
    if (inventory?.[key] !== value) failures.push(`inventory mismatch: ${key}`)
  }
  if (forbiddenScope?.status !== 'PASS' || !Array.isArray(forbiddenScope?.findings) || forbiddenScope.findings.length !== 0) failures.push('forbidden scope manifest is not PASS with empty findings')
  if (utf8Validation?.status !== 'PASS' || utf8Validation?.rootAscii !== true) failures.push('UTF-8 validation manifest is not PASS')
  if (batch === 'B13_R3') {
    const requiredReaderFiles = [
      'reader-a-version.log', 'reader-a-list.log', 'reader-a-extract.log',
      'reader-b-version.log', 'reader-b-list.log', 'reader-b-extract.log',
      'reader-content-compare.json',
    ]
    for (const file of requiredReaderFiles) {
      if (!fs.existsSync(path.join(resolvedRoot, 'EVIDENCE/B13_R3/PACKAGE_READERS', file))) failures.push(`R3 reader evidence missing: ${file}`)
    }
    if (utf8Validation?.readerA !== 'python-zipfile'
      || typeof utf8Validation?.readerB !== 'string'
      || utf8Validation.readerB === 'python-zipfile'
      || utf8Validation?.entrySetsEqual !== true
      || utf8Validation?.contentHashesEqual !== true
      || utf8Validation?.utf8FlagFailures !== 0
      || utf8Validation?.mojibakeFindings !== 0) failures.push('R3 dual-reader UTF-8 validation is incomplete')
  }
  if (expectedSourceCommit && sourceBinding?.sourceCommit !== expectedSourceCommit) failures.push('source binding commit mismatch')
  if (sourceBinding?.sourceCommit !== sourceBinding?.evidenceSourceCommit || sourceBinding?.sourceCommit !== sourceBinding?.sourceCommitAfter) failures.push('source binding fields disagree')
  if (sourceBinding?.workingTreeDirtyBefore !== false || sourceBinding?.workingTreeDirtyAfter !== false) failures.push('source binding reports dirty tree')

  const checksums = checkInternalSums(resolvedRoot)
  if (checksums.status !== 'PASS') failures.push(...checksums.mismatches)
  const result = {
    schema: batch === 'B13_R3' ? 'b13-r3-package-consistency' : 'b13-r2-package-consistency',
    root: `${rootName}/`,
    rootAscii: /^[\x00-\x7F]+$/.test(rootName),
    inventory: expectedInventory,
    checksum: checksums,
    forbiddenScope: { status: forbiddenScope?.status ?? 'FAIL', findings: forbiddenScope?.findings ?? null },
    sourceCommit: sourceBinding?.sourceCommit ?? null,
    expectedSourceCommit,
    utf8: { status: utf8Validation?.status ?? 'FAIL', replacementCharacterEntries: utf8Names, batch },
    failures,
    status: failures.length === 0 ? 'PASS' : 'FAIL',
  }
  return result
}

function validateZip(zipPath, expectedSourceCommit = null) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r2-zip-'))
  const extracted = path.join(temp, 'extracted')
  fs.mkdirSync(extracted, { recursive: true })
  const python = `import json, os, sys, zipfile\nzip_path=sys.argv[1]\nout=sys.argv[2]\nwith zipfile.ZipFile(zip_path) as archive:\n    names=[name for name in archive.namelist() if not name.endswith('/') ]\n    roots={name.split('/',1)[0] for name in names}\n    archive.extractall(out)\n    root=next(iter(roots)) if len(roots)==1 else ''\n    print(json.dumps({'entryCount':len(names),'root':root,'utf8Flags':sum(1 for info in archive.infolist() if not info.is_dir() and any(ord(ch)>127 for ch in info.filename) and not (info.flag_bits & 0x800))}))\n`
  const result = spawnSync('python3', ['-c', python, zipPath, extracted], { encoding: 'utf8' })
  if (result.status !== 0) return { status: 'FAIL', error: result.stderr.trim() }
  const lines = result.stdout.trim().split('\n')
  const archive = JSON.parse(lines[0])
  const stageRoot = path.join(extracted, archive.root)
  const tree = validatePackageTree(stageRoot, expectedSourceCommit)
  const actualEntries = archive.entryCount
  const expectedEntries = tree.inventory.totalFilesExcludingChecksums + 1
  const failures = [...tree.failures]
  if (archive.utf8Flags !== 0) failures.push('archive has non-UTF8 filename flags')
  if (archive.root !== path.basename(stageRoot)) failures.push('archive root is ambiguous')
  if (actualEntries !== expectedEntries) failures.push(`zip entry count ${actualEntries} != inventory plus checksum ${expectedEntries}`)
  return {
    status: failures.length === 0 ? 'PASS' : 'FAIL',
    archive,
    tree,
    actualEntries,
    expectedEntries,
    failures,
  }
}

export function validatePackageTarget(target, expectedSourceCommit = null) {
  const resolved = path.resolve(target)
  if (fs.statSync(resolved).isDirectory()) return validatePackageTree(resolved, expectedSourceCommit)
  return validateZip(resolved, expectedSourceCommit)
}

if (process.argv[1]?.endsWith('verify-package-consistency.mjs')) {
  const targetIndex = process.argv.findIndex(argument => argument === '--dir' || argument === '--zip')
  const target = targetIndex >= 0 ? process.argv[targetIndex + 1] : process.argv[2]
  const expectedSourceCommit = process.env.B13_R2_SOURCE_COMMIT ?? null
  if (!target) {
    console.error('PACKAGE_CONSISTENCY_TARGET_REQUIRED')
    process.exitCode = 1
  } else {
    const result = validatePackageTarget(target, expectedSourceCommit)
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
