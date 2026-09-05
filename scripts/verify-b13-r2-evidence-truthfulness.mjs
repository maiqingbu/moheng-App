import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const INTERNAL_PATTERN = /(?:\b(?:chapter|volume|artifact|obligation|project|fact|desire|task|model-run|state):[^\s\u3000]+|\b(?:projectId|artifactId|obligationId|volumeId|volume_id|payloadHash|stateDigest)\b)/i
const FUTURE_PATTERN = /终局推进|完成度|可以完结|Endgame|Completion|BookExitState|CompletionGate|ending readiness|completion readiness/i

function readJson(root, relative) {
  try { return JSON.parse(fs.readFileSync(path.join(root, relative), 'utf8')) } catch { return null }
}

function add(failures, condition, message) {
  if (!condition) failures.push(message)
}

export function evaluateR2EvidenceTruthfulness(evidenceDir, expectedSourceCommit) {
  const root = path.resolve(evidenceDir)
  const failures = []
  const checks = {}
  const planning = readJson(root, 'B13_R2_PLANNING_SOURCE_BINDING.json')
  checks.formalPlanning = Boolean(planning)
    && planning.status === 'PASS'
    && planning.sourceCommit === expectedSourceCommit
    && Array.isArray(planning.compatibilityFieldsUsedForTruth)
    && planning.compatibilityFieldsUsedForTruth.length === 0
    && Array.isArray(planning.formalOwnerFunctions)
    && planning.formalOwnerFunctions.includes('B07::PlanningOwner')
    && planning.formalOwnerFunctions.includes('B09::PlanningStateProjectionOwner')
  add(failures, checks.formalPlanning, 'formal planning source binding is incomplete')

  const integration = readJson(root, 'B13_R2_H03_H04_INTEGRATION.json')
  checks.integration = Boolean(integration)
    && integration.status === 'PASS'
    && integration.sourceCommit === expectedSourceCommit
    && Array.isArray(integration.tests)
    && integration.tests.length >= 4
    && integration.tests.every(test => test.status === 'PASS')
  add(failures, checks.integration, 'formal H03/H04 integration evidence is incomplete')

  const nullTest = readJson(root, 'B13_R2_COMPATIBILITY_NULL_TEST.json')
  checks.compatibilityNull = nullTest?.status === 'PASS'
    && nullTest.sourceCommit === expectedSourceCommit
    && nullTest.compatibilityFieldsAreDefaults === true
    && nullTest.healthUsesFormalSources === true
  add(failures, checks.compatibilityNull, 'compatibility-null evidence is missing or not formal-source backed')

  const scale = readJson(root, 'B13_R2_FORMAL_OWNER_SCALE.json')
  checks.scale = scale?.status === 'PASS'
    && scale.sourceCommit === expectedSourceCommit
    && Number(scale.routeScopes) > 101
    && Number(scale.tasks) > 201
    && scale.lastRequiredBlocked === true
    && scale.pageIndependentTruth === true
  add(failures, checks.scale, 'formal owner scale evidence does not meet >101/>201 thresholds')

  const authority = readJson(root, 'B13_R2_NO_AUTHORITY_MUTATION.json')
  const domains = Array.isArray(authority?.domains) ? authority.domains : []
  checks.authority = authority?.status === 'PASS'
    && authority.sourceCommit === expectedSourceCommit
    && domains.length >= 8
    && domains.every(domain => domain.beforeCount > 0
      && domain.beforeCount === domain.afterCount
      && typeof domain.beforeDigest === 'string'
      && domain.beforeDigest === domain.afterDigest
      && domain.unchanged === true)
    && authority.projectionChanges === 0
  add(failures, checks.authority, 'non-empty authority count/digest evidence is incomplete')

  const remote = readJson(root, 'B13_R2_NO_REMOTE_CALL.json')
  checks.noRemote = remote?.status === 'PASS'
    && remote.sourceCommit === expectedSourceCommit
    && remote.modelRunsBefore === remote.modelRunsAfter
  add(failures, checks.noRemote, 'no-remote-call evidence is incomplete')

  const mutations = readJson(root, 'B13_R2_MUTATION_GATES.json')
  checks.mutations = mutations?.status === 'PASS'
    && mutations.sourceCommit === expectedSourceCommit
    && Array.isArray(mutations.mutations)
    && mutations.mutations.length >= 2
    && mutations.mutations.every(item => item.status === 'PASS' && item.introducedStatus === 'FAIL' && item.revertedStatus === 'PASS' && item.reverted === true)
  add(failures, checks.mutations, 'R2 mutation evidence does not prove fail then revert')

  const forbidden = readJson(root, 'B13_R2_FORBIDDEN_SCOPE.json')
  checks.forbiddenScope = forbidden?.status === 'PASS'
    && forbidden.sourceCommit === expectedSourceCommit
    && Array.isArray(forbidden.findings)
    && forbidden.findings.length === 0
  add(failures, checks.forbiddenScope, 'forbidden-scope evidence is not PASS with empty findings')

  const packageConsistency = readJson(root, 'B13_R2_PACKAGE_CONSISTENCY.json')
  checks.packageConsistency = packageConsistency?.status === 'PASS'
    && packageConsistency.sourceCommit === expectedSourceCommit
  add(failures, checks.packageConsistency, 'package consistency evidence is incomplete')

  const manual = readJson(root, 'B13_R2_MANUAL_BROWSER.json')
  const requiredManual = ['M02', 'M03', 'M10', 'M11', 'M12']
  checks.manualBrowser = manual?.status === 'PASS'
    && manual.sourceCommit === expectedSourceCommit
    && Array.isArray(manual.items)
    && requiredManual.every((id, index) => {
      const item = manual.items[index]
      const screenshot = item && path.join(root, item.screenshotPath ?? '')
      const text = item?.actualVisibleText ?? ''
      return item?.id === id
        && item.result === 'PASS'
        && item.actualVisibleTextSource === 'playwright-dom-innerText'
        && /^[a-f0-9]{64}$/.test(item.domTextSha256 ?? '')
        && crypto.createHash('sha256').update(text, 'utf8').digest('hex') === item.domTextSha256
        && item.internalLeakageCheck === true
        && item.b14LeakageCheck === true
        && !INTERNAL_PATTERN.test(text)
        && !FUTURE_PATTERN.test(text)
        && fs.existsSync(screenshot)
        && fs.readFileSync(screenshot).subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex'))
    })
  add(failures, checks.manualBrowser, 'required R2 browser evidence is missing or not DOM sourced')

  const result = {
    schema: 'b13-r2-evidence-truthfulness',
    sourceCommit: expectedSourceCommit,
    checks,
    failures,
    status: failures.length === 0 ? 'PASS' : 'FAIL',
  }
  return result
}

if (process.argv[1]?.endsWith('verify-b13-r2-evidence-truthfulness.mjs')) {
  const evidenceDir = process.env.B13_R2_EVIDENCE_DIR
  const sourceCommit = process.env.B13_R2_SOURCE_COMMIT
  if (!evidenceDir || !sourceCommit) {
    console.error('B13_R2_EVIDENCE_INPUT_REQUIRED')
    process.exitCode = 1
  } else {
    const result = evaluateR2EvidenceTruthfulness(evidenceDir, sourceCommit)
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
