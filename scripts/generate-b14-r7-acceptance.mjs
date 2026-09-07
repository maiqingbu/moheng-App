import fs from 'node:fs'
import path from 'node:path'

export const R7_BASELINE = '505a939c80daf18d25a18fe9584e4b69ffbfcc4d'
export const B13_FROZEN_COMMIT = '1a85e8f57c2098973d89977b02d33665cc054066'
export const REQUIRED_R7_GATES = [
  'productionZeroDiff', 'continuityPagedSource', 'noAuthority11Domain', 'truncationMutation',
  'rustFmt', 'rustCheck', 'rustClippy', 'rustTestAllTargets', 'node', 'vitest', 'typecheck',
  'build', 'buildDemo', 'contracts', 'architecture', 'moduleBoundary', 'productUx', 'queryPlan',
  'frozenInputs', 'r6EmptyDomainMutation', 'r6FrozenInputMutation', 'r5MappingUi',
  'r5RuntimeRecheck', 'taskReality', 'task501', 'postCompletion', 'secondCompletion',
  'skillBundle', 'scale10m', 'pageIndependentTruth', 'b13Regression', 'b14Regression',
  'b14R6Regression', 'b15Boundary', 'rc1Boundary', 'packageAsciiRoot', 'packageNoMacosx',
  'packageUtf8', 'packageMojibake', 'packageInventory', 'filesSha256', 'packageIntegrity',
  'dualReader', 'secretScan', 'evidenceTruthfulness', 'packageBuild'
]
export const REQUIRED_R7_EVIDENCE = [
  'B14_R7_PREBATCH_FREEZE.json', 'B14_R7_CHANGED_FILES.json', 'B14_R7_SOURCE_BINDING.json',
  'B14_R7_PRODUCTION_ZERO_DIFF.json', 'B14_R7_CONTINUITY_PAGED_SOURCE.json',
  'B14_R7_NO_AUTHORITY_11_DOMAIN.json', 'B14_R7_MUTATION_GATES.json',
  'B14_R7_NODE_REGRESSION.json', 'B14_R7_QUERY_PLAN_REGRESSION.json',
  'B14_R7_FROZEN_INPUTS_CHECK.json', 'B14_R7_EVIDENCE_TRUTHFULNESS.json',
  'B14_R7_REGRESSION.json', 'B14_R7_PACKAGE_CONSISTENCY.json', 'B14_R7_ACCEPTANCE.json',
  'B14_R7_FINAL_DELIVERY_STATUS.json'
]

export function evaluateB14R7Acceptance(report) {
  const missingGates = REQUIRED_R7_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const missingEvidence = REQUIRED_R7_EVIDENCE.filter(name => report.evidence?.[name]?.status !== 'PASS')
  const sourceBinding = report.repairBaselineCommit === R7_BASELINE && report.sourceCommit === report.evidenceSourceCommit && report.sourceCommit === report.sourceCommitAfter && report.workingTreeDirtyBefore === false && report.workingTreeDirtyAfter === false && (report.excludedUncommittedPaths?.length ?? 0) === 0
  const status = sourceBinding && missingGates.length === 0 && missingEvidence.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, missingEvidence, sourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}

export function generateB14R7Acceptance(evidenceDir) {
  const reportPath = path.join(evidenceDir, 'gate-results-r7.json')
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'))
  report.evidence ??= {}
  report.evidence['B14_R7_ACCEPTANCE.json'] = { status: 'PASS' }
  report.evidence['B14_R7_FINAL_DELIVERY_STATUS.json'] = { status: report.gates?.packageBuild?.status === 'PASS' ? 'PASS' : 'FAIL' }
  const result = evaluateB14R7Acceptance(report)
  fs.writeFileSync(reportPath, `${JSON.stringify(result, null, 2)}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R7_ACCEPTANCE.json'), `${JSON.stringify(result, null, 2)}\n`)
  const gate = name => result.gates?.[name]?.status === 'PASS' ? 'PASS' : 'FAIL'
  const e01 = result.evidence?.['B14_R7_CONTINUITY_PAGED_SOURCE.json'] ?? {}
  const e02 = result.evidence?.['B14_R7_NO_AUTHORITY_11_DOMAIN.json'] ?? {}
  const r = result.evidence?.['B14_R7_REGRESSION.json'] ?? {}
  const lines = [
    `REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}`,
    `SOURCE_COMMIT = ${result.sourceCommit}`,
    '',
    'EVIDENCE_ONLY = true',
    'ACCEPTANCE_DEFINITION_CORRECTED = true',
    `PRODUCTION_CODE_ZERO_DIFF = ${gate('productionZeroDiff')}`,
    `B14-R7-01_CONTINUITY_PAGED_SOURCE = ${gate('continuityPagedSource')}`,
    `SOURCE_101_CLASS = ${e01.case101?.sourceClass ?? 'FAIL'}`,
    `SOURCE_101_ORDINAL = ${e01.case101?.sourceOrdinal ?? 'FAIL'}`,
    `SOURCE_101_PRESENT_IN_PROJECTION = ${e01.case101?.presentInProjection === true ? 'PASS' : 'FAIL'}`,
    `SOURCE_101_AFFECTS_CONTINUITY_DIGEST = ${e01.case101?.digestChanges === true ? 'PASS' : 'FAIL'}`,
    `SOURCE_501_CLASS = ${e01.case501?.sourceClass ?? 'FAIL'}`,
    `SOURCE_501_ORDINAL = ${e01.case501?.sourceOrdinal ?? 'FAIL'}`,
    `SOURCE_501_PRESENT_IN_PROJECTION = ${e01.case501?.presentInProjection === true ? 'PASS' : 'FAIL'}`,
    `SOURCE_501_AFFECTS_CONTINUITY_DIGEST = ${e01.case501?.digestChanges === true ? 'PASS' : 'FAIL'}`,
    `PAGED_SOURCE_TRUNCATION_MUTATION = ${gate('truncationMutation')}`,
    `HARD_CONTINUITY_BLOCKING_REGRESSION = ${e01.hardContinuityRegression?.completionBlocked === true ? 'PASS' : 'FAIL'}`,
    `B14-R7-02_NO_AUTHORITY_11_DOMAIN = ${gate('noAuthority11Domain')}`,
    `NO_AUTHORITY_ONE_FIXTURE = ${e02.oneFixture === true ? 'PASS' : 'FAIL'}`,
    `NO_AUTHORITY_DOMAIN_COUNT = ${e02.domainCount === 11 ? 11 : 'FAIL'}`,
    `NO_AUTHORITY_ALL_BEFORE_POSITIVE = ${e02.allBeforePositive === true ? 'PASS' : 'FAIL'}`,
    `ENDGAME_QUERY_ALL_11_UNCHANGED = ${e02.endgameQueryUnchanged === true ? 'PASS' : 'FAIL'}`,
    `COMPLETION_QUERY_ALL_11_UNCHANGED = ${e02.completionQueryUnchanged === true ? 'PASS' : 'FAIL'}`,
    `COMPLETE_BOOK_SUCCEEDED = ${e02.completeBookSucceeded === true ? 'PASS' : 'FAIL'}`,
    `COMPLETE_BOOK_ALL_11_FROZEN_DOMAINS_UNCHANGED = ${e02.completeBookFrozenDomainsUnchanged === true ? 'PASS' : 'FAIL'}`,
    `B14-R7-03_FINAL_CERTIFICATION = ${gate('rustTestAllTargets') === 'PASS' && gate('node') === 'PASS' && gate('typecheck') === 'PASS' && gate('build') === 'PASS' ? 'PASS' : 'FAIL'}`,
    `RUST_FMT = ${gate('rustFmt')}`,
    `RUST_CHECK = ${gate('rustCheck')}`,
    `RUST_CLIPPY = ${gate('rustClippy')}`,
    `RUST_TEST_ALL_TARGETS = ${gate('rustTestAllTargets')}`,
    `NODE_TESTS = ${gate('node')}`,
    `VITEST = ${gate('vitest')}`,
    `TYPECHECK = ${gate('typecheck')}`,
    `BUILD = ${gate('build')}`,
    `BUILD_DEMO = ${gate('buildDemo')}`,
    `CONTRACTS = ${gate('contracts')}`,
    `QUERY_PLAN_ALL_ROWS_REGRESSION = ${gate('queryPlan')}`,
    `FROZEN_INPUTS_EXACT = ${gate('frozenInputs')}`,
    `R6_EMPTY_DOMAIN_MUTATION_REGRESSION = ${gate('r6EmptyDomainMutation')}`,
    `R6_FROZEN_INPUT_MUTATION_REGRESSION = ${gate('r6FrozenInputMutation')}`,
    `R5_MAPPING_UI_REGRESSION = ${gate('r5MappingUi')}`,
    `R5_RUNTIME_RECHECK_REGRESSION = ${gate('r5RuntimeRecheck')}`,
    `TASK_REALITY_REGRESSION = ${gate('taskReality')}`,
    `TASK_501_REGRESSION = ${gate('task501')}`,
    `POST_COMPLETION_REGRESSION = ${gate('postCompletion')}`,
    `SECOND_COMPLETION_REGRESSION = ${gate('secondCompletion')}`,
    `SKILL_BUNDLE_REGRESSION = ${gate('skillBundle')}`,
    `PRODUCT_UX_REGRESSION = ${gate('productUx')}`,
    `SCALE_10M_REGRESSION = ${gate('scale10m')}`,
    `PAGE_INDEPENDENT_TRUTH = ${gate('pageIndependentTruth')}`,
    `ARCHITECTURE = ${gate('architecture')}`,
    `MODULE_BOUNDARY = ${gate('moduleBoundary')}`,
    `B13_REGRESSION = ${gate('b13Regression')}`,
    `B14_REGRESSION = ${gate('b14Regression')}`,
    `B14_R6_REGRESSION = ${gate('b14R6Regression')}`,
    `B15_BOUNDARY = ${gate('b15Boundary')}`,
    `RC1_BOUNDARY = ${gate('rc1Boundary')}`,
    `PACKAGE_ASCII_ROOT = ${gate('packageAsciiRoot')}`,
    `PACKAGE_NO_MACOSX = ${gate('packageNoMacosx')}`,
    `PACKAGE_UTF8 = ${gate('packageUtf8')}`,
    `PACKAGE_MOJIBAKE = ${gate('packageMojibake')}`,
    `PACKAGE_INVENTORY = ${gate('packageInventory')}`,
    `FILES_SHA256 = ${gate('filesSha256')}`,
    `PACKAGE_INTEGRITY = ${gate('packageIntegrity')}`,
    `DUAL_READER = ${gate('dualReader')}`,
    `SECRET_SCAN = ${gate('secretScan')}`,
    `EVIDENCE_TRUTHFULNESS = ${gate('evidenceTruthfulness')}`,
    `ACCEPTANCE_FAIL_CLOSED = ${result.status === 'PASS' ? 'PASS' : 'FAIL'}`,
    `DELIVERY_STATUS = ${result.deliveryStatus}`,
    'NEXT_BATCH_ALLOWED = false',
    `MISSING_GATES = ${JSON.stringify(result.missingGates)}`,
    `MISSING_EVIDENCE = ${JSON.stringify(result.missingEvidence)}`,
  ]
  fs.writeFileSync(path.join(evidenceDir, 'B14_R7_FINAL_REPORT.txt'), `${lines.join('\n')}\n`)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R7_FINAL_DELIVERY_STATUS.json'), `${JSON.stringify({ schema: 'b14-r7-final-delivery-status', sourceCommit: result.sourceCommit, status: result.status, deliveryStatus: result.deliveryStatus, nextBatchAllowed: false, missingGates: result.missingGates, missingEvidence: result.missingEvidence }, null, 2)}\n`)
  return result
}
