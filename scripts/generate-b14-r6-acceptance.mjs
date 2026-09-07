import fs from 'node:fs'
import path from 'node:path'

export const R6_BASELINE = 'e4d7dff16d79a83cfe77f42c55d2b10690777114'
export const B13_FROZEN_COMMIT = '1a85e8f57c2098973d89977b02d33665cc054066'
export const REQUIRED_R6_GATES = ['productionCodeZeroDiff', 'continuityPagedBoundary', 'noAuthority11Domain', 'emptyDomainMutation', 'frozenInputMutation', 'r5ProductionRegression', 'queryPlanRegression', 'frozenInputsExact', 'packageBuild']
export const REQUIRED_R6_EVIDENCE = ['B14_R6_PRODUCTION_CODE_ZERO_DIFF.json', 'B14_R6_CONTINUITY_PAGED_BOUNDARY.json', 'B14_R6_NO_AUTHORITY_MUTATION.json', 'B14_R6_QUERY_PLAN_REGRESSION.json', 'B14_R6_MUTATION_GATES.json', 'B14_R6_FROZEN_INPUTS_CHECK.json', 'B14_R6_EVIDENCE_TRUTHFULNESS.json', 'B14_R6_REGRESSION.json', 'B14_R6_PACKAGE_CONSISTENCY.json', 'B14_R6_FINAL_DELIVERY_STATUS.json']
export function evaluateB14R6Acceptance(report) {
  const missingGates = REQUIRED_R6_GATES.filter(name => report.gates?.[name]?.status !== 'PASS')
  const missingEvidence = REQUIRED_R6_EVIDENCE.filter(name => report.evidence?.[name]?.status !== 'PASS')
  const sourceBinding = report.sourceCommit === report.evidenceSourceCommit && report.sourceCommit === report.sourceCommitAfter && report.workingTreeDirtyBefore === false && report.workingTreeDirtyAfter === false && report.excludedUncommittedPaths?.length === 0
  const status = report.repairBaselineCommit === R6_BASELINE && sourceBinding && missingGates.length === 0 && missingEvidence.length === 0 ? 'PASS' : 'FAIL'
  return { ...report, status, missingGates, missingEvidence, sourceBinding, deliveryStatus: status === 'PASS' ? 'READY_FOR_FORMAL_ACCEPTANCE' : 'FAIL', nextBatchAllowed: false }
}
export function generateB14R6Acceptance(evidenceDir) {
  const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r6.json'), 'utf8'))
  const result = evaluateB14R6Acceptance(report)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R6_ACCEPTANCE.json'), `${JSON.stringify(result, null, 2)}\n`)
  const gate = name => result.gates?.[name]?.status === 'PASS' ? 'PASS' : 'FAIL'
  const text = `REPAIR_BASELINE_COMMIT = ${result.repairBaselineCommit}\nSOURCE_COMMIT = ${result.sourceCommit}\n\nEVIDENCE_ONLY = true\nPRODUCTION_CODE_ZERO_DIFF = ${gate('productionCodeZeroDiff')}\nR6-E01_CONTINUITY_PAGED_BOUNDARY = ${gate('continuityPagedBoundary')}\nCONTINUITY_101_SOURCE_CLASS = ${result.evidence?.['B14_R6_CONTINUITY_PAGED_BOUNDARY.json']?.case101?.sourceClass ?? 'FAIL'}\nCONTINUITY_101_SOURCE_ORDINAL = ${result.evidence?.['B14_R6_CONTINUITY_PAGED_BOUNDARY.json']?.case101?.sourceOrdinal ?? 'FAIL'}\nCONTINUITY_101_REAL_BLOCKER = ${gate('continuityPagedBoundary')}\nCONTINUITY_501_SOURCE_CLASS = ${result.evidence?.['B14_R6_CONTINUITY_PAGED_BOUNDARY.json']?.case501?.sourceClass ?? 'FAIL'}\nCONTINUITY_501_SOURCE_ORDINAL = ${result.evidence?.['B14_R6_CONTINUITY_PAGED_BOUNDARY.json']?.case501?.sourceOrdinal ?? 'FAIL'}\nCONTINUITY_501_REAL_BLOCKER = ${gate('continuityPagedBoundary')}\nR6-E02_NO_AUTHORITY_11_DOMAIN = ${gate('noAuthority11Domain')}\nNO_AUTHORITY_DOMAIN_COUNT = ${result.evidence?.['B14_R6_NO_AUTHORITY_MUTATION.json']?.domainCount ?? 'FAIL'}\nCONTINUITY_DOMAIN_NONEMPTY = ${gate('noAuthority11Domain')}\nNO_AUTHORITY_ALL_BEFORE_POSITIVE = ${gate('noAuthority11Domain')}\nNO_AUTHORITY_COUNT_DIGEST = ${gate('noAuthority11Domain')}\nR6-E03_EMPTY_DOMAIN_MUTATION = ${gate('emptyDomainMutation')}\nMUT_R6_01_FAILURE_CLASS = ${result.evidence?.['B14_R6_MUTATION_GATES.json']?.mutR601FailureClass ?? 'FAIL'}\nMUT_R6_01_PATCH_SHA = ${gate('emptyDomainMutation')}\nR6-E04_FROZEN_INPUT_MUTATION = ${gate('frozenInputMutation')}\nMUT_R6_02_FAILURE_CLASS = ${result.evidence?.['B14_R6_MUTATION_GATES.json']?.mutR602FailureClass ?? 'FAIL'}\nMUT_R6_02_PATCH_SHA = ${gate('frozenInputMutation')}\nFROZEN_INPUTS_EXACT = ${gate('frozenInputsExact')}\nQUERY_PLAN_ALL_ROWS_REGRESSION = ${gate('queryPlanRegression')}\nR5_PRODUCTION_REGRESSION = ${gate('r5ProductionRegression')}\nPACKAGE_BUILD = ${gate('packageBuild')}\nDELIVERY_STATUS = ${result.deliveryStatus}\nNEXT_BATCH_ALLOWED = false\nMISSING_GATES = ${JSON.stringify(result.missingGates)}\nMISSING_EVIDENCE = ${JSON.stringify(result.missingEvidence)}\n`
  fs.writeFileSync(path.join(evidenceDir, 'B14_R6_FINAL_REPORT.txt'), text)
  fs.writeFileSync(path.join(evidenceDir, 'B14_R6_FINAL_DELIVERY_STATUS.json'), `${JSON.stringify({ schema: 'b14-r6-final-delivery-status', sourceCommit: result.sourceCommit, status: result.status, deliveryStatus: result.deliveryStatus, nextBatchAllowed: false, missingGates: result.missingGates, missingEvidence: result.missingEvidence }, null, 2)}\n`)
  return result
}
