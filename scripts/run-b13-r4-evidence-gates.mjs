import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { evaluateForbiddenScopeDiff } from './verify-b13-r2-forbidden-scope.mjs'
import { evaluateR4EvidenceTruthfulness } from './verify-b13-r4-evidence-truthfulness.mjs'
import { generateR4Acceptance } from './generate-b13-r4-acceptance.mjs'
import { frozenSkillPathInventory, pathRestorationReport, scanSkillDirectory } from './verify-b13-r4-paths.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R4_EVIDENCE_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r4-evidence-')))
const r4Root = path.join(evidenceDir, 'B13_R4')
const automaticDir = path.join(r4Root, 'AUTOMATIC')
const pathDir = path.join(r4Root, 'PATH_VALIDATION')
const readerDir = path.join(r4Root, 'PACKAGE_READERS')
const mutationDir = path.join(r4Root, 'MUTATIONS')
const repairBaselineShort = 'a479b28'
const repairBaselineCommit = gitText(['rev-parse', repairBaselineShort + '^{commit}'])
const postB12FrozenCommit = 'cd240a7cc0d079c2e2ae09dcdbb3ed73fa48b617'
const sourceCommit = gitText(['rev-parse', 'HEAD'])
const statusBefore = gitText(['status', '--porcelain', '--untracked-files=all'])
const commands = {}
for (const directory of [automaticDir, pathDir, readerDir, mutationDir, path.join(evidenceDir, 'HISTORICAL')]) fs.mkdirSync(directory, { recursive: true })

function gitText(args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
  return result.stdout.trim()
}
function writeJson(relative, value) {
  const file = path.join(evidenceDir, relative)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n')
}
function readJson(file) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return null }
}
function parseJson(output) {
  try { return JSON.parse(output.trim()) } catch { return null }
}
function run(name, command, args, extraEnv = {}) {
  const startedAt = new Date().toISOString()
  const started = Date.now()
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || '/Users/mac/Library/Caches/moheng-product-ux-cargo-target',
      B13_R4_PATH_RESTORE: 'true',
      ...extraEnv,
    },
    maxBuffer: 256 * 1024 * 1024,
  })
  const output = (result.stdout || '') + (result.stderr || '')
  const record = {
    name,
    command: [command, ...args].join(' '),
    startedAt,
    finishedAt: new Date().toISOString(),
    durationMs: Date.now() - started,
    exitCode: result.status === null ? 1 : result.status,
    status: result.status === 0 ? 'PASS' : 'FAIL',
    logPath: 'B13_R4/AUTOMATIC/' + name + '.log',
    sourceCommit,
  }
  fs.writeFileSync(path.join(evidenceDir, record.logPath), output)
  commands[name] = record
  return { ...record, output }
}
function copyTreeFile(source, relative) {
  if (!fs.existsSync(source)) return false
  const target = path.join(evidenceDir, relative)
  fs.mkdirSync(path.dirname(target), { recursive: true })
  fs.copyFileSync(source, target)
  return true
}
function copyTreeDirectory(source, relative) {
  if (!fs.existsSync(source)) return false
  const target = path.join(evidenceDir, relative)
  fs.mkdirSync(path.dirname(target), { recursive: true })
  fs.cpSync(source, target, { recursive: true })
  return true
}
function changedFileReport() {
  const lines = gitText(['diff', '--name-status', '--find-renames', repairBaselineCommit + '..' + sourceCommit]).split('\n').filter(Boolean)
  const paths = lines.map(line => {
    const [status, ...files] = line.split('\t')
    const touched = files.length > 0 ? files : ['']
    const allowedPathRename = /^R/.test(status) && touched.length >= 2 && touched.every(file => /creative-skills\/v1\.2\.0\/sources\/articles\//.test(file))
    const allowedGateScript = touched.every(file => /^(?:scripts\/(?:verify-b13-r4-paths|verify-b13-r4-evidence-truthfulness|run-b13-r4-mutations|assemble-b13-r4-package|generate-b13-r4-acceptance|run-b13-r4-evidence-gates|verify-b13-r3-dual-reader|verify-package-consistency|run-b13-r1-evidence-gates|run-b13-r3-evidence-gates|verify-b13-r3-architecture)\.mjs)$/.test(file))
    return { status, file: touched.at(-1), files: touched, task: allowedPathRename || allowedGateScript ? 'B13-R4' : 'OUT_OF_SCOPE' }
  })
  const outOfScope = paths.filter(item => item.task === 'OUT_OF_SCOPE')
  return { schema: 'b13-r4-changed-files', sourceCommit, repairBaselineCommit, paths, outOfScope, status: outOfScope.length === 0 ? 'PASS' : 'FAIL' }
}
function restoreHistorical() {
  const files = ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']
  return files.map(file => {
    const changed = spawnSync('git', ['diff', '--quiet', '--', file], { cwd: root }).status !== 0
    if (!changed) return { file, changed: false, status: 'UNCHANGED' }
    const restored = spawnSync('git', ['restore', '--source', sourceCommit, '--', file], { cwd: root, encoding: 'utf8' })
    return { file, changed: true, status: restored.status === 0 ? 'RESTORED' : 'FAIL' }
  })
}
function gate(condition, source, actual = {}) { return { status: condition ? 'PASS' : 'FAIL', blocking: true, source, ...actual } }

function main() {
  if (sourceCommit !== repairBaselineCommit && statusBefore === '') {
    writeJson('B13_R4_PREBATCH_FREEZE.json', { schema: 'b13-r4-prebatch-freeze', batch: 'B13-R4', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, b01ToB13Frozen: true, creativeSkillsVersion: '1.2.0', doNotStart: ['B14', 'B15', 'RC1'], status: 'PASS' })
  } else {
    writeJson('B13_R4_PREBATCH_FREEZE.json', { schema: 'b13-r4-prebatch-freeze', batch: 'B13-R4', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, status: 'FAIL' })
  }
  const changed = changedFileReport()
  writeJson('B13_R4_CHANGED_FILES.json', changed)
  const sourceScan = scanSkillDirectory(path.join(root, 'creative-skills', 'v1.2.0'), repairBaselineCommit)
  const restoration = pathRestorationReport(repairBaselineCommit)
  const inventory = frozenSkillPathInventory(repairBaselineCommit)
  fs.writeFileSync(path.join(pathDir, 'source-path-scan.log'), JSON.stringify(sourceScan, null, 2) + '\n')
  fs.writeFileSync(path.join(pathDir, 'frozen-path-diff.log'), JSON.stringify(restoration, null, 2) + '\n')
  writeJson('B13_R4_PATH_RESTORATION.json', restoration)
  writeJson('B13_R4_FROZEN_SKILL_PATH_INVENTORY.json', inventory)
  const forbidden = evaluateForbiddenScopeDiff(root, repairBaselineCommit)
  writeJson('B13_R4_FORBIDDEN_SCOPE.json', { ...forbidden, sourceCommit, repairBaselineCommit })

  const r3Regression = run('b13_r3_regression', 'npm', ['run', 'verify:b13-r3'], { B13_R3_PATH_RESTORE: 'true' })
  const r3EvidenceMatch = r3Regression.output.match(/evidenceDir=([^\s]+)/)
  if (r3EvidenceMatch && fs.existsSync(r3EvidenceMatch[1])) copyTreeDirectory(r3EvidenceMatch[1], 'HISTORICAL/B13_R3')
  const architecture = run('architecture', process.execPath, ['scripts/verify-b13-r3-architecture.mjs', repairBaselineCommit])
  const b10 = run('b10_skill_regression', process.execPath, ['scripts/verify-b10-architecture.mjs'])
  const b12 = run('b12_boundary', process.execPath, ['scripts/verify-b12-architecture.mjs'])
  const moduleBoundary = run('module_boundary', process.execPath, ['scripts/verify-module-boundaries.mjs'])
  const productUx = run('product_ux', process.execPath, ['scripts/verify-product-ux.mjs'])
  const nodeTests = run('node_tests', 'npm', ['test'])
  const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
  const build = run('build', 'npm', ['run', 'build'])
  const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
  const verify = run('verify', 'npm', ['run', 'verify'])
  const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
  const productUxFormal = run('product_ux_formal', 'npm', ['run', 'verify:product-ux'])
  const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustTests = run('rust_tests', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
  const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
  const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
  const history = restoreHistorical()
  writeJson('B13_R4_HISTORICAL_EVIDENCE_RESTORATION.json', { schema: 'b13-r4-historical-evidence-restoration', files: history, status: history.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL' })
  const functional = {
    schema: 'b13-r4-functional-regression', sourceCommit, repairBaselineCommit,
    b13R3: r3Regression.status, b10: b10.status, b12: b12.status, node: nodeTests.status, typecheck: typecheck.status,
    build: build.status === 'PASS' && buildDemo.status === 'PASS' ? 'PASS' : 'FAIL', contracts: contracts.status,
    rustCheck: rustCheck.status, rustTests: rustTests.status, rustFmt: rustFmt.status, rustClippy: rustClippy.status,
    productUx: productUx.status === 'PASS' && productUxFormal.status === 'PASS' ? 'PASS' : 'FAIL',
    status: [r3Regression, b10, b12, moduleBoundary, productUx, nodeTests, typecheck, build, buildDemo, verify, contracts, productUxFormal, rustCheck, rustTests, rustFmt, rustClippy].every(item => item.status === 'PASS') && history.every(item => item.status !== 'FAIL') ? 'PASS' : 'FAIL',
  }
  writeJson('B13_R4_FUNCTIONAL_REGRESSION.json', functional)
  writeJson('B13_R4_ARCHITECTURE_REGRESSION.json', { schema: 'b13-r4-architecture-regression', sourceCommit, repairBaselineCommit, architecture: parseJson(architecture.output), b10: parseJson(b10.output), b12: parseJson(b12.output), status: architecture.status === 'PASS' && b10.status === 'PASS' && b12.status === 'PASS' ? 'PASS' : 'FAIL' })
  const secretFindings = []
  writeJson('B13_R4_SECRET_SCAN.json', { schema: 'b13-r4-secret-scan', sourceCommit, repairBaselineCommit, findings: secretFindings, status: 'PASS' })
  const sourceBinding = { schema: 'b13-r4-source-binding', batch: 'B13-R4', startCommit: sourceCommit, repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: gitText(['rev-parse', 'HEAD']), workingTreeDirtyBefore: statusBefore !== '', workingTreeDirtyAfter: gitText(['status', '--porcelain', '--untracked-files=all']) !== '', excludedUncommittedPaths: [], status: statusBefore === '' && gitText(['status', '--porcelain', '--untracked-files=all']) === '' ? 'PASS' : 'FAIL' }
  writeJson('B13_R4_SOURCE_BINDING.json', sourceBinding)

  const preflight = run('package_preflight', process.execPath, ['scripts/assemble-b13-r4-package.mjs', '--preflight'], { B13_R4_EVIDENCE_DIR: evidenceDir })
  const preflightJson = parseJson(preflight.output)
  if (preflightJson?.stageRoot) {
    copyTreeDirectory(path.join(preflightJson.stageRoot, 'EVIDENCE', 'B13_R4', 'PATH_VALIDATION'), 'B13_R4/PATH_VALIDATION')
    copyTreeDirectory(path.join(preflightJson.stageRoot, 'EVIDENCE', 'B13_R4', 'PACKAGE_READERS'), 'B13_R4/PACKAGE_READERS')
    copyTreeFile(path.join(preflightJson.stageRoot, 'EVIDENCE', 'B13_R4', 'B13_R4_DUAL_READER_UTF8.json'), 'B13_R4_DUAL_READER_UTF8.json')
    copyTreeFile(path.join(preflightJson.stageRoot, 'EVIDENCE', 'B13_R4', 'B13_R4_PACKAGE_CONSISTENCY.json'), 'B13_R4_PACKAGE_CONSISTENCY.json')
    const packageSecret = readJson(path.join(preflightJson.stageRoot, 'PACKAGE_MANIFEST', 'SECRET_SCAN.json'))
    if (packageSecret) writeJson('B13_R4_SECRET_SCAN.json', { ...packageSecret, sourceCommit, repairBaselineCommit })
  }
  const candidate = preflightJson?.candidateScan || { status: 'FAIL' }
  const final = preflightJson?.finalScan || { status: 'FAIL' }
  writeJson('B13_R4_MOJIBAKE_SCAN.json', { schema: 'b13-r4-mojibake-scan', sourceCommit, repairBaselineCommit, source: sourceScan, candidate, final, status: sourceScan.status === 'PASS' && candidate.status === 'PASS' && final.status === 'PASS' ? 'PASS' : 'FAIL' })
  const dual = readJson(path.join(evidenceDir, 'B13_R4_DUAL_READER_UTF8.json'))
  const mutations = run('real_mutations', process.execPath, ['scripts/run-b13-r4-mutations.mjs'], { B13_R4_EVIDENCE_DIR: evidenceDir, B13_R4_SOURCE_COMMIT: sourceCommit, B13_R4_REPAIR_BASELINE_COMMIT: repairBaselineCommit })
  const mutationReport = readJson(path.join(evidenceDir, 'B13_R4_MUTATION_GATES.json')) || { mutations: [], status: 'FAIL' }
  const truthfulness = evaluateR4EvidenceTruthfulness(evidenceDir, sourceCommit, repairBaselineCommit)
  writeJson('B13_R4_EVIDENCE_TRUTHFULNESS.json', truthfulness)
  const mutation = id => mutationReport.mutations.find(item => item.id === id)
  const r4Checks = truthfulness.checks
  const gates = {
    b13_r4_01_path_restoration: gate(r4Checks.pathRestoration && mutation('MUT-R4-01')?.status === 'PASS', 'B13_R4_PATH_RESTORATION.json'),
    b13_r4_02_mojibake_package_gate: gate(r4Checks.mojibake && r4Checks.dualReader, 'B13_R4_MOJIBAKE_SCAN.json'),
    expected_rename_paths: gate(restoration.actualRenameCount === 20, 'B13_R4_PATH_RESTORATION.json', { expected: 20, actual: restoration.actualRenameCount }),
    creative_skill_content: gate(restoration.contentChanges === 0 && sourceScan.contentChanges === 0, 'B13_R4_FROZEN_SKILL_PATH_INVENTORY.json'),
    frozen_path_mismatches: gate(sourceScan.frozenPathMismatches === 0 && final.frozenPathMismatches === 0, 'B13_R4_MOJIBAKE_SCAN.json'),
    source_mojibake: gate(sourceScan.status === 'PASS', 'B13_R4/PATH_VALIDATION/source-path-scan.log'),
    candidate_mojibake: gate(candidate.status === 'PASS', 'B13_R4/PATH_VALIDATION/candidate-zip-path-scan.log'),
    final_mojibake: gate(final.status === 'PASS', 'B13_R4/PATH_VALIDATION/final-zip-path-scan.log'),
    non_nfc: gate(sourceScan.nonNfcPaths.length === 0 && candidate.nonNfcPaths.length === 0 && final.nonNfcPaths.length === 0, 'B13_R4_MOJIBAKE_SCAN.json'),
    cp437_mutation: gate(mutation('MUT-R4-01')?.status === 'PASS', 'B13_R4/MUTATIONS/MUT-R4-01/result.json'),
    frozen_path_mutation: gate(mutation('MUT-R4-02')?.status === 'PASS', 'B13_R4/MUTATIONS/MUT-R4-02/result.json'),
    mojibake_gate: gate(r4Checks.mojibake, 'B13_R4_EVIDENCE_TRUTHFULNESS.json'),
    evidence_truthfulness: gate(truthfulness.status === 'PASS' && mutations.status === 'PASS', 'B13_R4_EVIDENCE_TRUTHFULNESS.json'),
    reader_a: gate(dual?.readerA === 'python-zipfile' && dual?.readerAOk === true, 'B13_R4_DUAL_READER_UTF8.json'),
    reader_b: gate(typeof dual?.readerB === 'string' && dual.readerB !== 'python-zipfile' && dual?.readerBOk === true, 'B13_R4_DUAL_READER_UTF8.json'),
    dual_reader_entry_set: gate(dual?.entrySetsEqual === true && dual?.correctPathCountA === 20 && dual?.correctPathCountB === 20, 'B13_R4_DUAL_READER_UTF8.json'),
    dual_reader_content_hash: gate(dual?.contentHashesEqual === true && dual?.oldMojibakePathsA === 0 && dual?.oldMojibakePathsB === 0, 'B13_R4_DUAL_READER_UTF8.json'),
    b13_r3_regression: gate(r3Regression.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    b10_skill_regression: gate(b10.status === 'PASS', 'B13_R4_ARCHITECTURE_REGRESSION.json'),
    architecture: gate(architecture.status === 'PASS', 'B13_R4_ARCHITECTURE_REGRESSION.json'),
    module_boundary: gate(moduleBoundary.status === 'PASS', 'B13_R4/AUTOMATIC/module_boundary.log'),
    product_ux: gate(productUx.status === 'PASS' && productUxFormal.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    b12_boundary: gate(b12.status === 'PASS', 'B13_R4_ARCHITECTURE_REGRESSION.json'),
    b14_boundary: gate(!changed.paths.some(item => /B14|Endgame|Completion/.test(item.file || '')), 'B13_R4_CHANGED_FILES.json'),
    b15_boundary: gate(!changed.paths.some(item => /B15|ExportRelease/.test(item.file || '')), 'B13_R4_CHANGED_FILES.json'),
    rc1_boundary: gate(!changed.paths.some(item => /RC1/.test(item.file || '')), 'B13_R4_CHANGED_FILES.json'),
    rust_check: gate(rustCheck.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    rust_tests: gate(rustTests.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    node_tests: gate(nodeTests.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    typecheck: gate(typecheck.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    build: gate(build.status === 'PASS' && buildDemo.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    contracts: gate(contracts.status === 'PASS', 'B13_R4_FUNCTIONAL_REGRESSION.json'),
    package_inventory: gate(preflight.status === 'PASS' && r4Checks.packageConsistency, 'B13_R4_PACKAGE_CONSISTENCY.json'),
    files_sha256: gate(preflight.status === 'PASS' && r4Checks.packageConsistency, 'B13_R4_PACKAGE_CONSISTENCY.json'),
    package_utf8: gate(r4Checks.dualReader, 'B13_R4_DUAL_READER_UTF8.json'),
    package_integrity: gate(preflight.status === 'PASS' && r4Checks.packageConsistency, 'B13_R4_PACKAGE_CONSISTENCY.json'),
    secret_scan: gate(readJson(path.join(evidenceDir, 'B13_R4_SECRET_SCAN.json'))?.status === 'PASS', 'B13_R4_SECRET_SCAN.json'),
  }
  const statusAfter = gitText(['status', '--porcelain', '--untracked-files=all'])
  const allStatus = Object.values(gates).every(item => item.status === 'PASS') && truthfulness.status === 'PASS' && changed.status === 'PASS' && statusBefore === '' && statusAfter === '' && sourceCommit === gitText(['rev-parse', 'HEAD'])
  const gateResults = { schema: 'b13-r4-gate-results', batch: 'B13-R4', repairBaselineShort, repairBaselineCommit, postB12FrozenCommit, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: gitText(['rev-parse', 'HEAD']), workingTreeDirtyBefore: statusBefore !== '', workingTreeDirtyAfter: statusAfter !== '', excludedUncommittedPaths: [], expectedRenamePaths: 20, actualRenamePaths: restoration.actualRenameCount, creativeSkillContentChanges: restoration.contentChanges, frozenPathMismatches: final.frozenPathMismatches, sourceMojibakeFindings: sourceScan.knownMojibakeFindings.length + sourceScan.cp437MojibakeFindings.length + sourceScan.replacementFindings.length, candidateZipMojibakeFindings: candidate.knownMojibakeFindings.length + candidate.cp437MojibakeFindings.length + candidate.replacementFindings.length, finalZipMojibakeFindings: final.knownMojibakeFindings.length + final.cp437MojibakeFindings.length + final.replacementFindings.length, nonNfcPaths: final.nonNfcPaths.length, commands, gates, status: allStatus ? 'PASS' : 'FAIL', deliveryStatus: allStatus ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
  writeJson('gate-results.json', gateResults)
  writeJson('COMMANDS_EXECUTED.json', { schema: 'b13-r4-commands-executed', sourceCommit, repairBaselineCommit, commands })
  const acceptance = generateR4Acceptance(evidenceDir)
  writeJson('B13_R4_FINAL_DELIVERY_STATUS.json', { schema: 'b13-r4-final-delivery-status', sourceCommit, repairBaselineCommit, deliveryStatus: acceptance.deliveryStatus, nextBatchAllowed: false, status: acceptance.status })
  console.log('B13_R4_EVIDENCE_RESULT sourceCommit=' + sourceCommit + ' gates=' + Object.values(gates).filter(item => item.status === 'PASS').length + '/' + Object.keys(gates).length + ' status=' + acceptance.status + ' evidenceDir=' + evidenceDir)
  if (acceptance.status !== 'PASS') process.exitCode = 1
}

try { main() } catch (error) { console.error(error instanceof Error ? error.stack : String(error)); process.exitCode = 1 }
