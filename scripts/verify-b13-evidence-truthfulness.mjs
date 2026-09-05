import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const INTERNAL_ID_PATTERN = /(?:\b(?:chapter|volume|artifact|obligation|project|fact|desire|task|model-run|state):[^\s\u3000]+|\b(?:projectId|artifactId|obligationId|volumeId|volume_id|payloadHash|stateDigest)\b)/i
const B14_PATTERN = /终局推进|完成度|可以完结|Endgame|Completion|BookExitState|CompletionGate|ending readiness/i

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'))
  } catch {
    return null
  }
}

function pathInEvidence(evidenceDir, relative) {
  if (typeof relative !== 'string' || !relative || path.isAbsolute(relative)) return null
  const resolved = path.resolve(evidenceDir, relative)
  const root = `${path.resolve(evidenceDir)}${path.sep}`
  return resolved === path.resolve(evidenceDir) || resolved.startsWith(root) ? resolved : null
}

function existsFile(evidenceDir, relative) {
  const file = pathInEvidence(evidenceDir, relative)
  return Boolean(file && fs.existsSync(file) && fs.statSync(file).isFile())
}

function addFailure(failures, condition, message) {
  if (!condition) failures.push(message)
}

export function evaluateEvidenceTruthfulness(evidenceDir, expectedSourceCommit) {
  const root = path.resolve(evidenceDir)
  const failures = []
  const checks = {}
  const manual = readJson(path.join(root, 'B13_R1_MANUAL_WALKTHROUGH.json'))
  const manualItems = Array.isArray(manual?.items) ? manual.items : []
  const expectedManualIds = Array.from({ length: 12 }, (_, index) => `M${String(index + 1).padStart(2, '0')}`)
  const manualValid = manual?.sourceCommit === expectedSourceCommit
    && manual?.status === 'PASS'
    && manualItems.length === expectedManualIds.length
    && manualItems.every((item, index) => {
      const screenshot = pathInEvidence(root, item.screenshotPath)
      const actualText = typeof item.actualVisibleText === 'string' ? item.actualVisibleText.trim() : ''
      const screenshotBytes = screenshot && fs.existsSync(screenshot) ? fs.readFileSync(screenshot) : Buffer.alloc(0)
      return item.id === expectedManualIds[index]
        && typeof item.route === 'string' && item.route.length > 0
        && typeof item.fixture === 'string' && item.fixture.length > 0
        && typeof item.trigger === 'string' && item.trigger.length > 0
        && typeof item.expectedState === 'string' && item.expectedState.length > 0
        && actualText.length > 0
        && item.actualVisibleTextSource === 'playwright-dom-innerText'
        && /^[a-f0-9]{64}$/.test(item.domTextSha256 ?? '')
        && item.internalLeakageCheck === true
        && item.b14LeakageCheck === true
        && item.result === 'PASS'
        && Boolean(screenshot && screenshotBytes.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex')))
        && !INTERNAL_ID_PATTERN.test(actualText)
        && !B14_PATTERN.test(actualText)
    })
  checks.manualBrowser = manualValid
  addFailure(failures, manualValid, 'manual browser evidence is incomplete, not DOM-sourced, or contains a leak')
  for (const item of manualItems) {
    if (!existsFile(root, item.screenshotPath)) {
      failures.push(`manual browser screenshot missing: ${item.id ?? 'unknown'} (${item.screenshotPath ?? 'unknown path'})`)
    }
  }

  const mutationIds = ['MUT-R1-01', 'MUT-R1-02', 'MUT-R1-03', 'MUT-R1-04']
  const mutationValid = mutationIds.every(id => {
    const directory = path.join(root, 'MUTATIONS', id)
    const result = readJson(path.join(directory, 'result.json'))
    return fs.existsSync(path.join(directory, 'patch.diff'))
      && fs.statSync(path.join(directory, 'patch.diff')).size > 0
      && fs.existsSync(path.join(directory, 'introduced.log'))
      && fs.existsSync(path.join(directory, 'reverted.log'))
      && result?.sourceCommit === expectedSourceCommit
      && result.patchApplied === true
      && Number.isInteger(result.introducedExitCode)
      && result.introducedExitCode !== 0
      && result.introducedStatus === 'FAIL'
      && result.revertedExitCode === 0
      && result.revertedStatus === 'PASS'
      && result.reverted === true
      && result.status === 'PASS'
  })
  checks.realMutations = mutationValid
  addFailure(failures, mutationValid, 'mutation evidence does not prove a real failing introduction and successful revert')

  const noRemote = readJson(path.join(root, 'B13_R1_NO_REMOTE_CALL.json'))
  const transportAssertionValid = noRemote?.transportCounter === 'NOT_APPLICABLE'
    || (Number.isInteger(noRemote?.transportCallsBefore)
      && Number.isInteger(noRemote?.transportCallsAfter)
      && noRemote.transportCallsBefore === noRemote.transportCallsAfter)
  const noRemoteValid = noRemote?.sourceCommit === expectedSourceCommit
    && Number.isInteger(noRemote.modelRunsBefore)
    && Number.isInteger(noRemote.modelRunsAfter)
    && noRemote.modelRunsBefore === noRemote.modelRunsAfter
    && transportAssertionValid
    && noRemote.status === 'PASS'
    && existsFile(root, noRemote.logPath)
  checks.noRemoteModelCall = noRemoteValid
  addFailure(failures, noRemoteValid, 'no-remote-call evidence lacks real before/after assertions')

  const noAuthority = readJson(path.join(root, 'B13_R1_NO_AUTHORITY_MUTATION.json'))
  const noAuthorityValid = noAuthority?.sourceCommit === expectedSourceCommit
    && noAuthority.unchanged === true
    && noAuthority.status === 'PASS'
    && noAuthority.before && noAuthority.after
    && JSON.stringify(noAuthority.before) === JSON.stringify(noAuthority.after)
    && existsFile(root, noAuthority.logPath)
  checks.noAuthorityMutation = noAuthorityValid
  addFailure(failures, noAuthorityValid, 'no-authority evidence lacks real before/after equality')

  const scale = readJson(path.join(root, 'B13_R1_SCALE.json'))
  const scaleValid = scale?.sourceCommit === expectedSourceCommit
    && scale?.status === 'PASS'
    && scale.words >= 10_000_000
    && scale.chapters >= 4000
    && scale.obligations > 51
    && scale.routeScopes > 101
    && scale.tasks > 201
    && scale.readerDesires > 50
    && scale.contextRuns >= 16
    && scale.costRuns >= 8
    && scale.qualitySamples >= 20
  checks.scaleFixture = scaleValid
  addFailure(failures, scaleValid, 'scale evidence does not meet the B13 R1 materialized thresholds')

  const planNames = ['H01', 'H02', 'H03', 'H04', 'H07', 'ISSUES']
  const queryPlansValid = planNames.every(name => {
    const file = path.join(root, 'PERFORMANCE', `${name}.explain.txt`)
    return fs.existsSync(file)
      && fs.statSync(file).size > 0
      && /EXPLAIN QUERY PLAN/.test(fs.readFileSync(file, 'utf8'))
  })
  checks.queryPlans = queryPlansValid
  addFailure(failures, queryPlansValid, 'query plan evidence is missing or not raw EXPLAIN output')

  const staleCache = readJson(path.join(root, 'B13_R1_STALE_CACHE.json'))
  const staleCacheValid = staleCache?.sourceCommit === expectedSourceCommit
    && staleCache.cacheMode === 'ON_DEMAND'
    && staleCache.staleCacheTest === 'NOT_APPLICABLE'
    && typeof staleCache.reason === 'string'
    && staleCache.reason.length > 0
    && staleCache.staleSourceRevisionRejected !== true
  checks.staleCacheHonest = staleCacheValid
  addFailure(failures, staleCacheValid, 'stale cache evidence is a fabricated claim instead of an honest ON_DEMAND result')

  const sourceBinding = readJson(path.join(root, 'B13_R1_SOURCE_BINDING.json'))
  const sourceBindingValid = sourceBinding?.sourceCommit === expectedSourceCommit
    && sourceBinding.evidenceSourceCommit === expectedSourceCommit
    && sourceBinding.sourceCommitAfter === expectedSourceCommit
    && sourceBinding.workingTreeDirtyBefore === false
    && sourceBinding.workingTreeDirtyAfter === false
    && Array.isArray(sourceBinding.excludedUncommittedPaths)
    && sourceBinding.excludedUncommittedPaths.length === 0
    && sourceBinding.status === 'PASS'
  checks.sourceBinding = sourceBindingValid
  addFailure(failures, sourceBindingValid, 'evidence is not bound to the clean final source commit')

  return {
    schema: 'b13-r1-evidence-truthfulness-r1',
    sourceCommit: expectedSourceCommit,
    checks,
    failures,
    status: failures.length === 0 ? 'PASS' : 'FAIL',
  }
}

if (process.argv[1]?.endsWith('verify-b13-evidence-truthfulness.mjs')) {
  const evidenceDir = process.env.B13_R1_EVIDENCE_DIR
  const sourceCommit = process.env.B13_R1_SOURCE_COMMIT
  if (!evidenceDir || !sourceCommit) {
    console.error('B13_R1_EVIDENCE_INPUT_REQUIRED')
    process.exitCode = 1
  } else {
    const result = evaluateEvidenceTruthfulness(evidenceDir, sourceCommit)
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
