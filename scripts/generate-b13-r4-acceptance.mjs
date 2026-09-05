import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export const REQUIRED_R4_GATES = [
  'b13_r4_01_path_restoration',
  'b13_r4_02_mojibake_package_gate',
  'expected_rename_paths',
  'creative_skill_content',
  'frozen_path_mismatches',
  'source_mojibake',
  'candidate_mojibake',
  'final_mojibake',
  'non_nfc',
  'cp437_mutation',
  'frozen_path_mutation',
  'mojibake_gate',
  'evidence_truthfulness',
  'reader_a',
  'reader_b',
  'dual_reader_entry_set',
  'dual_reader_content_hash',
  'b13_r3_regression',
  'b10_skill_regression',
  'architecture',
  'module_boundary',
  'product_ux',
  'b12_boundary',
  'b14_boundary',
  'b15_boundary',
  'rc1_boundary',
  'rust_check',
  'rust_tests',
  'node_tests',
  'typecheck',
  'build',
  'contracts',
  'package_inventory',
  'files_sha256',
  'package_utf8',
  'package_integrity',
  'secret_scan',
]

function passed(gate) {
  return gate?.status === 'PASS' && (gate.exitCode === undefined || gate.exitCode === 0)
}

function sourceBindingPassed(report) {
  return Boolean(report.sourceCommit)
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
}

export function evaluateR4Acceptance(report = {}) {
  const gates = report.gates ?? {}
  const missingGates = REQUIRED_R4_GATES.filter(name => !gates[name])
  const failedGates = REQUIRED_R4_GATES.filter(name => gates[name] && !passed(gates[name]))
  const sourceBinding = sourceBindingPassed(report)
  const status = missingGates.length === 0 && failedGates.length === 0 && sourceBinding ? 'PASS' : 'FAIL'
  return {
    schema: 'b13-r4-acceptance',
    batch: 'B13-R4',
    milestone: 'Creative Skills path encoding and package Gate final micro patch',
    status,
    deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL',
    repairBaselineShort: report.repairBaselineShort ?? 'a479b28',
    repairBaselineCommit: report.repairBaselineCommit ?? null,
    sourceCommit: report.sourceCommit ?? null,
    evidenceSourceCommit: report.evidenceSourceCommit ?? null,
    sourceCommitAfter: report.sourceCommitAfter ?? null,
    startCommit: report.startCommit ?? report.sourceCommit ?? null,
    workingTreeDirtyBefore: report.workingTreeDirtyBefore ?? null,
    workingTreeDirtyAfter: report.workingTreeDirtyAfter ?? null,
    excludedUncommittedPaths: report.excludedUncommittedPaths ?? [],
    requiredBlockingGates: REQUIRED_R4_GATES,
    missingGates,
    failedGates,
    expectedRenamePaths: report.expectedRenamePaths ?? 20,
    actualRenamePaths: report.actualRenamePaths ?? 0,
    creativeSkillContentChanges: report.creativeSkillContentChanges ?? 'FAIL',
    frozenPathMismatches: report.frozenPathMismatches ?? 'FAIL',
    sourceMojibakeFindings: report.sourceMojibakeFindings ?? 'FAIL',
    candidateZipMojibakeFindings: report.candidateZipMojibakeFindings ?? 'FAIL',
    finalZipMojibakeFindings: report.finalZipMojibakeFindings ?? 'FAIL',
    nonNfcPaths: report.nonNfcPaths ?? 'FAIL',
    nextBatchAllowed: false,
  }
}

function gateStatus(acceptance, name) {
  return passed(acceptance.gates?.[name]) ? 'PASS' : 'FAIL'
}

export function fixedR4Report(acceptance) {
  const status = name => gateStatus(acceptance, name)
  const values = {
    REPAIR_BASELINE_SHORT: acceptance.repairBaselineShort,
    REPAIR_BASELINE_COMMIT: acceptance.repairBaselineCommit,
    SOURCE_COMMIT: acceptance.sourceCommit,
    'B13-R4-01_FROZEN_PATH_RESTORATION': status('b13_r4_01_path_restoration'),
    'B13-R4-02_MOJIBAKE_AND_PACKAGE_GATE': status('b13_r4_02_mojibake_package_gate'),
    EXPECTED_RENAME_PATHS: acceptance.expectedRenamePaths ?? 20,
    ACTUAL_RENAME_PATHS: acceptance.actualRenamePaths ?? 0,
    CREATIVE_SKILL_CONTENT_CHANGES: acceptance.creativeSkillContentChanges ?? 'FAIL',
    FROZEN_PATH_MISMATCHES: acceptance.frozenPathMismatches ?? 'FAIL',
    SOURCE_MOJIBAKE_FINDINGS: acceptance.sourceMojibakeFindings ?? 'FAIL',
    CANDIDATE_ZIP_MOJIBAKE_FINDINGS: acceptance.candidateZipMojibakeFindings ?? 'FAIL',
    FINAL_ZIP_MOJIBAKE_FINDINGS: acceptance.finalZipMojibakeFindings ?? 'FAIL',
    NON_NFC_PATHS: acceptance.nonNfcPaths ?? 'FAIL',
    CP437_MUTATION: status('cp437_mutation'),
    FROZEN_PATH_MUTATION: status('frozen_path_mutation'),
    MOJIBAKE_GATE: status('mojibake_gate'),
    EVIDENCE_TRUTHFULNESS: status('evidence_truthfulness'),
    READER_A: status('reader_a'),
    READER_B: status('reader_b'),
    DUAL_READER_ENTRY_SET: status('dual_reader_entry_set'),
    DUAL_READER_CONTENT_HASH: status('dual_reader_content_hash'),
    B13_R3_REGRESSION: status('b13_r3_regression'),
    B10_SKILL_REGRESSION: status('b10_skill_regression'),
    ARCHITECTURE: status('architecture'),
    MODULE_BOUNDARY: status('module_boundary'),
    PRODUCT_UX: status('product_ux'),
    B12_BOUNDARY: status('b12_boundary'),
    B14_BOUNDARY: status('b14_boundary'),
    B15_BOUNDARY: status('b15_boundary'),
    RC1_BOUNDARY: status('rc1_boundary'),
    RUST_CHECK: status('rust_check'),
    RUST_TESTS: status('rust_tests'),
    NODE_TESTS: status('node_tests'),
    TYPECHECK: status('typecheck'),
    BUILD: status('build'),
    CONTRACTS: status('contracts'),
    PACKAGE_INVENTORY: status('package_inventory'),
    FILES_SHA256: status('files_sha256'),
    PACKAGE_UTF8: status('package_utf8'),
    PACKAGE_INTEGRITY: status('package_integrity'),
    SECRET_SCAN: status('secret_scan'),
    DELIVERY_STATUS: acceptance.deliveryStatus,
    NEXT_BATCH_ALLOWED: false,
  }
  return values
}

export function generateR4Acceptance(evidenceDir = process.env.B13_R4_EVIDENCE_DIR ?? path.join(os.tmpdir(), 'moheng-b13-r4-evidence-latest')) {
  const reportPath = path.join(evidenceDir, 'gate-results.json')
  if (!fs.existsSync(reportPath)) throw new Error('B13_R4_EVIDENCE_MISSING:' + reportPath)
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  const acceptance = {
    ...evaluateR4Acceptance(report),
    gates: report.gates ?? {},
    generatedAt: new Date().toISOString(),
    generator: 'scripts/generate-b13-r4-acceptance.mjs',
    expectedRenamePaths: report.expectedRenamePaths ?? 20,
    actualRenamePaths: report.actualRenamePaths ?? 0,
    creativeSkillContentChanges: report.creativeSkillContentChanges ?? 'FAIL',
    frozenPathMismatches: report.frozenPathMismatches ?? 'FAIL',
    sourceMojibakeFindings: report.sourceMojibakeFindings ?? 'FAIL',
    candidateZipMojibakeFindings: report.candidateZipMojibakeFindings ?? 'FAIL',
    finalZipMojibakeFindings: report.finalZipMojibakeFindings ?? 'FAIL',
    nonNfcPaths: report.nonNfcPaths ?? 'FAIL',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R4_ACCEPTANCE.json'), JSON.stringify(acceptance, null, 2) + '\n')
  fs.writeFileSync(path.join(evidenceDir, 'B13_R4_FINAL_REPORT.txt'), Object.entries(fixedR4Report(acceptance)).map(([key, value]) => key + ' = ' + (typeof value === 'string' ? value : JSON.stringify(value))).join('\n') + '\n')
  return acceptance
}

if (process.argv[1]?.endsWith('generate-b13-r4-acceptance.mjs')) {
  const result = generateR4Acceptance()
  console.log('B13_R4 acceptance generated: ' + result.status + '; missing=' + result.missingGates.length + '; failed=' + result.failedGates.length + '; nextBatchAllowed=false')
  if (result.status !== 'PASS') process.exitCode = 1
}
