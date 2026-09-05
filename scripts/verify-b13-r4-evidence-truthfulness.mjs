import fs from 'node:fs'
import path from 'node:path'

const REQUIRED_READERS = [
  'reader-a-version.log', 'reader-a-list.log', 'reader-a-extract.log',
  'reader-b-version.log', 'reader-b-list.log', 'reader-b-test.log', 'reader-b-extract.log',
  'reader-content-compare.json',
]

function readJson(root, relative) {
  try { return JSON.parse(fs.readFileSync(path.join(root, relative), 'utf8')) } catch { return null }
}

function exists(root, relative) {
  return fs.existsSync(path.join(root, relative))
}

function count(value, key) {
  return Array.isArray(value?.[key]) ? value[key].length : null
}

function rawPathMarkers(root, relative) {
  if (!exists(root, relative)) return { exists: false, cp437: null, known: null, replacement: null }
  const text = fs.readFileSync(path.join(root, relative), 'utf8')
  return {
    exists: true,
    cp437: (text.match(/CP437_MOJIBAKE_CANDIDATE/g) || []).length,
    known: (text.match(/KNOWN_MOJIBAKE_MARKER/g) || []).length,
    replacement: (text.match(/REPLACEMENT_CHARACTER/g) || []).length,
  }
}

function sourceBound(value, sourceCommit, baselineCommit) {
  return Boolean(value)
    && value.sourceCommit === sourceCommit
    && (value.repairBaselineCommit === baselineCommit || value.baselineCommit === baselineCommit)
}

function add(failures, condition, message) {
  if (!condition) failures.push(message)
}

export function evaluateR4EvidenceTruthfulness(evidenceDir, expectedSourceCommit, expectedBaselineCommit, options = {}) {
  const root = path.resolve(evidenceDir)
  const failures = []
  const checks = {}
  const binding = readJson(root, 'B13_R4_SOURCE_BINDING.json')
  checks.sourceBinding = sourceBound(binding, expectedSourceCommit, expectedBaselineCommit)
    && binding.sourceCommitAfter === expectedSourceCommit
    && binding.workingTreeDirtyBefore === false
    && binding.workingTreeDirtyAfter === false
    && Array.isArray(binding.excludedUncommittedPaths)
    && binding.excludedUncommittedPaths.length === 0
  add(failures, checks.sourceBinding, 'R4 source binding is not clean and exact')

  const pathRestoration = readJson(root, 'B13_R4_PATH_RESTORATION.json')
  checks.pathRestoration = sourceBound(pathRestoration, expectedSourceCommit, expectedBaselineCommit)
    && pathRestoration.status === 'PASS'
    && pathRestoration.expectedRenameCount === 20
    && pathRestoration.actualRenameCount === 20
    && pathRestoration.contentChanges === 0
    && pathRestoration.pathMatchesFrozen === 0
    && pathRestoration.nfcNonNormalized === 0
    && Array.isArray(pathRestoration.items)
    && pathRestoration.items.length === 20
    && pathRestoration.items.every(item => item.contentUnchanged === true && item.pathMatchesFrozen === true && item.nfcNormalized === true)
  add(failures, checks.pathRestoration, 'frozen path restoration is incomplete')

  const inventory = readJson(root, 'B13_R4_FROZEN_SKILL_PATH_INVENTORY.json')
  checks.inventory = sourceBound(inventory, expectedSourceCommit, expectedBaselineCommit)
    && inventory.status === 'PASS'
    && inventory.totalFiles === 676
    && inventory.mojibakeCount === 0
    && inventory.nonNfcCount === 0
    && inventory.pathDigest === inventory.expectedPathDigest
    && inventory.contentDigest === inventory.expectedContentDigest
  add(failures, checks.inventory, 'frozen skill path inventory is not exact')

  const mojibake = readJson(root, 'B13_R4_MOJIBAKE_SCAN.json')
  const sourceRaw = rawPathMarkers(root, 'B13_R4/PATH_VALIDATION/source-path-scan.log')
  const candidateRaw = rawPathMarkers(root, 'B13_R4/PATH_VALIDATION/candidate-zip-path-scan.log')
  const finalRaw = rawPathMarkers(root, 'B13_R4/PATH_VALIDATION/final-zip-path-scan.log')
  const scanPass = (scan, raw) => scan?.status === 'PASS'
    && raw.exists
    && count(scan, 'cp437MojibakeFindings') === raw.cp437
    && count(scan, 'knownMojibakeFindings') === raw.known
    && count(scan, 'replacementFindings') === raw.replacement
  checks.mojibake = sourceBound(mojibake, expectedSourceCommit, expectedBaselineCommit)
    && mojibake.status === 'PASS'
    && mojibake.source.status === 'PASS'
    && mojibake.candidate.status === 'PASS'
    && mojibake.final.status === 'PASS'
    && scanPass(mojibake.source, sourceRaw)
    && scanPass(mojibake.candidate, candidateRaw)
    && scanPass(mojibake.final, finalRaw)
    && count(mojibake.source, 'cp437MojibakeFindings') === 0
    && count(mojibake.candidate, 'cp437MojibakeFindings') === 0
    && count(mojibake.final, 'cp437MojibakeFindings') === 0
  add(failures, checks.mojibake, 'mojibake scan does not match raw logs')

  const dual = readJson(root, 'B13_R4_DUAL_READER_UTF8.json')
  const compare = readJson(root, 'B13_R4/PACKAGE_READERS/reader-content-compare.json')
  const readerFiles = REQUIRED_READERS.every(file => exists(root, 'B13_R4/PACKAGE_READERS/' + file))
  const correctA = Array.isArray(dual?.correctPathsA) ? dual.correctPathsA.length : dual?.correctPathCountA
  const correctB = Array.isArray(dual?.correctPathsB) ? dual.correctPathsB.length : dual?.correctPathCountB
  checks.dualReader = sourceBound(dual, expectedSourceCommit, expectedBaselineCommit)
    && dual.status === 'PASS'
    && dual.readerA === 'python-zipfile'
    && typeof dual.readerB === 'string'
    && dual.readerB !== 'python-zipfile'
    && dual.entrySetsEqual === true
    && dual.contentHashesEqual === true
    && dual.utf8FlagFailures === 0
    && dual.mojibakeFindings === 0
    && dual.cp437MojibakeFindings === 0
    && dual.frozenPathMismatches === 0
    && dual.nonNfcPaths === 0
    && correctA === 20
    && correctB === 20
    && dual.oldMojibakePathsA === 0
    && dual.oldMojibakePathsB === 0
    && compare?.entrySetsEqual === true
    && compare?.contentHashesEqual === true
    && readerFiles
  add(failures, checks.dualReader, 'dual reader evidence is incomplete or not source-bound')

  const packageConsistency = readJson(root, 'B13_R4_PACKAGE_CONSISTENCY.json')
  checks.packageConsistency = sourceBound(packageConsistency, expectedSourceCommit, expectedBaselineCommit)
    && packageConsistency.status === 'PASS'
  add(failures, checks.packageConsistency, 'package consistency evidence is incomplete')

  const mutations = readJson(root, 'B13_R4_MUTATION_GATES.json')
  if (options.requireMutations !== false) {
    const required = ['MUT-R4-01', 'MUT-R4-02', 'MUT-R4-EVIDENCE-TRUTHFULNESS']
    checks.mutations = sourceBound(mutations, expectedSourceCommit, expectedBaselineCommit)
      && mutations.status === 'PASS'
      && required.every(id => {
        const item = mutations.mutations?.find(candidate => candidate.id === id)
        return item?.status === 'PASS' && item.introducedStatus === 'FAIL' && item.revertedStatus === 'PASS' && item.reverted === true
      })
    add(failures, checks.mutations, 'R4 mutation evidence does not prove fail then revert')
  } else checks.mutations = true

  const regression = readJson(root, 'B13_R4_FUNCTIONAL_REGRESSION.json')
  checks.regression = sourceBound(regression, expectedSourceCommit, expectedBaselineCommit) && regression.status === 'PASS'
  add(failures, checks.regression, 'R4 regression evidence is incomplete')
  const changed = readJson(root, 'B13_R4_CHANGED_FILES.json')
  checks.changedFiles = sourceBound(changed, expectedSourceCommit, expectedBaselineCommit)
    && changed.status === 'PASS'
    && Array.isArray(changed.outOfScope) && changed.outOfScope.length === 0
    && Array.isArray(changed.paths) && changed.paths.length >= 20
  add(failures, checks.changedFiles, 'R4 changed-file scope is incomplete')
  const architecture = readJson(root, 'B13_R4_ARCHITECTURE_REGRESSION.json')
  checks.architecture = sourceBound(architecture, expectedSourceCommit, expectedBaselineCommit) && architecture.status === 'PASS'
  add(failures, checks.architecture, 'R4 architecture regression is incomplete')
  const secretScan = readJson(root, 'B13_R4_SECRET_SCAN.json')
  checks.secretScan = !secretScan || (sourceBound(secretScan, expectedSourceCommit, expectedBaselineCommit) && secretScan.status === 'PASS' && secretScan.findings.length === 0)
  add(failures, checks.secretScan, 'R4 secret scan is not clean')

  return {
    schema: 'b13-r4-evidence-truthfulness',
    sourceCommit: expectedSourceCommit,
    repairBaselineCommit: expectedBaselineCommit,
    checks,
    rawPathLogs: { source: sourceRaw, candidate: candidateRaw, final: finalRaw },
    failures,
    status: failures.length === 0 ? 'PASS' : 'FAIL',
  }
}

if (process.argv[1]?.endsWith('verify-b13-r4-evidence-truthfulness.mjs')) {
  const evidenceDir = process.env.B13_R4_EVIDENCE_DIR
  const sourceCommit = process.env.B13_R4_SOURCE_COMMIT
  const baselineCommit = process.env.B13_R4_REPAIR_BASELINE_COMMIT
  if (!evidenceDir || !sourceCommit || !baselineCommit) {
    console.error('B13_R4_EVIDENCE_INPUT_REQUIRED')
    process.exitCode = 1
  } else {
    const result = evaluateR4EvidenceTruthfulness(evidenceDir, sourceCommit, baselineCommit, {
      requireMutations: process.env.B13_R4_REQUIRE_MUTATIONS !== 'false',
    })
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
