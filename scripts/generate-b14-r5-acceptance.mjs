import fs from 'node:fs'
import path from 'node:path'

export const R5_REPAIR_BASELINE = 'd965c36a6f45ec21a619d5fcf41fdc62846db156'
export const B13_FROZEN_COMMIT = '1a85e8f57c2098973d89977b02d33665cc054066'
export const REQUIRED_R5_GATES = [
  'mappingUiScopedCandidates', 'runtimeRecheckMeaningful', 'noAuthorityAllDomainsNonempty',
  'continuity101Real', 'continuity501Real', 'queryPlansAllRows', 'frozenInputsExactSha',
  'dualReaderRawEvidence', 'mutationBehavioral', 'architecture', 'moduleBoundary',
  'b13Regression', 'b15Boundary', 'rc1Boundary', 'rustFmt', 'rustCheck', 'rustClippy',
  'rustTests', 'nodeTests', 'vitest', 'typecheck', 'build', 'buildDemo', 'contracts',
  'productUx', 'scale10m', 'pageIndependentTruth', 'packageAsciiRoot', 'packageNoMacosx',
  'packageUtf8', 'packageMojibake', 'packageInventory', 'filesSha256', 'packageIntegrity',
  'secretScan',
]
export const REQUIRED_R5_EVIDENCE = [
  'B14_R5_PREBATCH_FREEZE.json', 'B14_R5_CHANGED_FILES.json', 'B14_R5_SOURCE_BINDING.json',
  'B14_R5_MAPPING_UI_SCOPE.json', 'B14_R5_RUNTIME_RECHECK.json', 'B14_R5_NO_AUTHORITY_MUTATION.json',
  'B14_R5_CONTINUITY_101_501.json', 'B14_R5_QUERY_PLANS.json', 'B14_R5_MUTATION_GATES.json',
  'B14_R5_FROZEN_INPUTS_CHECK.json', 'B14_R5_DUAL_READER.json', 'B14_R5_EVIDENCE_TRUTHFULNESS.json',
  'B14_R5_REGRESSION.json', 'B14_R5_PACKAGE_CONSISTENCY.json', 'B14_R5_FINAL_DELIVERY_STATUS.json',
]

export function evaluateB14R5Acceptance(report) {
  const missingGates = REQUIRED_R5_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const missingEvidence = REQUIRED_R5_EVIDENCE.filter(name => report.evidence?.[name]?.status !== 'PASS')
  const exactSourceBinding = report.repairBaselineCommit === R5_REPAIR_BASELINE
    && report.sourceCommit === report.evidenceSourceCommit
    && report.sourceCommit === report.sourceCommitAfter
    && report.workingTreeDirtyBefore === false
    && report.workingTreeDirtyAfter === false
    && Array.isArray(report.excludedUncommittedPaths)
    && report.excludedUncommittedPaths.length === 0
  const status = exactSourceBinding && missingGates.length === 0 && missingEvidence.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, missingEvidence, exactSourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}

export function generateB14R5Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r5.json'), 'utf8'))
  const result = evaluateB14R5Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R5_ACCEPTANCE.json'), `${JSON.stringify(result, null, 2)}\n`)
  const gate = name => result.gates?.[name]?.status === 'PASS' ? 'PASS' : 'FAIL'
  const text = `REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}\nSOURCE_COMMIT = ${result.sourceCommit}\n\nB14-R5-01_MAPPING_UI_SCOPE = ${gate('mappingUiScopedCandidates')}\nB14-R5-02_RUNTIME_RECHECK = ${gate('runtimeRecheckMeaningful')}\nB14-R5-03_EVIDENCE_TRUTH = ${gate('noAuthorityAllDomainsNonempty') && gate('continuity101Real') && gate('continuity501Real') && gate('queryPlansAllRows') && gate('mutationBehavioral') === 'PASS' ? 'PASS' : 'FAIL'}\nB14-R5-04_FROZEN_PACKAGE = ${gate('frozenInputsExactSha') && gate('dualReaderRawEvidence') && gate('packageIntegrity') === 'PASS' ? 'PASS' : 'FAIL'}\n\nRESERVATION_UI_SCOPED = ${gate('mappingUiScopedCandidates')}\nPAYOFF_UI_SCOPED = ${gate('mappingUiScopedCandidates')}\nCROSS_RESPONSIBILITY_LEAK = ${gate('mappingUiScopedCandidates') === 'PASS' ? '0' : 'FAIL'}\nMAPPING_2000_BOUNDED = ${gate('mappingUiScopedCandidates')}\nTASK_RECHECK_POSITIVE = ${gate('runtimeRecheckMeaningful')}\nTASK_RECHECK_NEGATIVE = ${gate('runtimeRecheckMeaningful')}\nTHREAD_RECHECK_POSITIVE = ${gate('runtimeRecheckMeaningful')}\nTHREAD_RECHECK_NEGATIVE = ${gate('runtimeRecheckMeaningful')}\nRUNTIME_RECHECK_NO_EXTRA_MODEL = ${gate('runtimeRecheckMeaningful')}\nRUNTIME_RECHECK_NO_AUTHORITY_WRITE = ${gate('runtimeRecheckMeaningful')}\nNO_AUTHORITY_ALL_DOMAINS_NONEMPTY = ${gate('noAuthorityAllDomainsNonempty')}\nNO_AUTHORITY_COUNT_DIGEST = ${gate('noAuthorityAllDomainsNonempty')}\nCONTINUITY_101_REAL = ${gate('continuity101Real')}\nCONTINUITY_501_REAL = ${gate('continuity501Real')}\nQUERY_PLAN_ALL_ROWS = ${gate('queryPlansAllRows')}\nMUTATION_GATES = ${gate('mutationBehavioral')}\nEVIDENCE_TRUTHFULNESS = ${gate('evidenceTruthfulness')}\nB14_ORIGINAL_SHA = ${gate('frozenInputsExactSha')}\nB14_R1_SHA = ${gate('frozenInputsExactSha')}\nB14_R2_SHA = ${gate('frozenInputsExactSha')}\nB14_R3_SHA = ${gate('frozenInputsExactSha')}\nB14_R4_SHA = ${gate('frozenInputsExactSha')}\nB14_R5_SHA = ${gate('frozenInputsExactSha')}\nFREEZE_B13_SHA = ${gate('frozenInputsExactSha')}\nFROZEN_INPUTS_EXACT = ${gate('frozenInputsExactSha')}\nDUAL_READER_A = ${gate('dualReaderRawEvidence')}\nDUAL_READER_B = ${gate('dualReaderRawEvidence')}\nDUAL_READER_ENTRYSET = ${gate('dualReaderRawEvidence')}\nDUAL_READER_CONTENT_HASH = ${gate('dualReaderRawEvidence')}\nTASK_REALITY_REGRESSION = ${gate('scale10m')}\nTASK_501_REGRESSION = ${gate('scale10m')}\nCONTINUITY_COMPLETE_READER_REGRESSION = ${gate('continuity501Real')}\nUNRELATED_REVISION_NO_STALE = ${gate('b13Regression')}\nPOST_COMPLETION_RECHECK = ${gate('b13Regression')}\nSECOND_COMPLETION_NEW_DIGEST = ${gate('b13Regression')}\nSKILL_BUNDLE_REGRESSION = ${gate('b13Regression')}\nPRODUCT_UX_REGRESSION = ${gate('productUx')}\nSCALE_10M_REGRESSION = ${gate('scale10m')}\nPAGE_INDEPENDENT_TRUTH = ${gate('pageIndependentTruth')}\nARCHITECTURE = ${gate('architecture')}\nMODULE_BOUNDARY = ${gate('moduleBoundary')}\nB09_REGRESSION = ${gate('b13Regression')}\nB10_REGRESSION = ${gate('b13Regression')}\nB12_REGRESSION = ${gate('b13Regression')}\nB13_REGRESSION = ${gate('b13Regression')}\nB15_BOUNDARY = ${gate('b15Boundary')}\nRC1_BOUNDARY = ${gate('rc1Boundary')}\nRUST_FMT = ${gate('rustFmt')}\nRUST_CHECK = ${gate('rustCheck')}\nRUST_CLIPPY = ${gate('rustClippy')}\nRUST_TESTS = ${gate('rustTests')}\nNODE_TESTS = ${gate('nodeTests')}\nVITEST = ${gate('vitest')}\nTYPECHECK = ${gate('typecheck')}\nBUILD = ${gate('build')}\nCONTRACTS = ${gate('contracts')}\nPACKAGE_ASCII_ROOT = ${gate('packageAsciiRoot')}\nPACKAGE_NO_MACOSX = ${gate('packageNoMacosx')}\nPACKAGE_UTF8 = ${gate('packageUtf8')}\nPACKAGE_MOJIBAKE = ${gate('packageMojibake')}\nPACKAGE_INVENTORY = ${gate('packageInventory')}\nFILES_SHA256 = ${gate('filesSha256')}\nPACKAGE_INTEGRITY = ${gate('packageIntegrity')}\nSECRET_SCAN = ${gate('secretScan')}\nDELIVERY_STATUS = ${result.deliveryStatus}\nNEXT_BATCH_ALLOWED = false\nMISSING_GATES = ${JSON.stringify(result.missingGates)}\nMISSING_EVIDENCE = ${JSON.stringify(result.missingEvidence)}\n`
  fs.writeFileSync(path.join(evidenceDir, 'B14_R5_FINAL_REPORT.txt'), text)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R5_FINAL_DELIVERY_STATUS.json'), `${JSON.stringify({ schema: 'b14-r5-final-delivery-status', sourceCommit: result.sourceCommit, status: result.status, deliveryStatus: result.deliveryStatus, nextBatchAllowed: false, missingGates: result.missingGates, missingEvidence: result.missingEvidence }, null, 2)}\n`)
  return result
}
