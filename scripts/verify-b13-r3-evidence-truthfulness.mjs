import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const R3_INTERNAL_PLAN_PATTERN = /planning_state_ledger_index/i
const REQUIRED_PLANS = [
  'formal_route_source',
  'planning_state_projection_lookup',
  'volume_plan_payload_lookup',
  'task_state_chunk_le_100',
  'health_source_time_aggregate',
]
const REQUIRED_READER_FILES = [
  'reader-a-version.log',
  'reader-a-list.log',
  'reader-a-extract.log',
  'reader-b-version.log',
  'reader-b-list.log',
  'reader-b-extract.log',
  'reader-content-compare.json',
]

function readJson(root, relative) {
  try { return JSON.parse(fs.readFileSync(path.join(root, relative), 'utf8')) } catch { return null }
}

function sha256Json(value) {
  return crypto.createHash('sha256').update(JSON.stringify(value), 'utf8').digest('hex')
}

function add(failures, condition, message) {
  if (!condition) failures.push(message)
}

function sourceBound(value, expectedSourceCommit, expectedBaseline) {
  return Boolean(value)
    && value.sourceCommit === expectedSourceCommit
    && value.repairBaselineCommit === expectedBaseline
}

export function evaluateR3EvidenceTruthfulness(evidenceDir, expectedSourceCommit, expectedBaselineCommit, options = {}) {
  const root = path.resolve(evidenceDir)
  const failures = []
  const checks = {}
  const binding = readJson(root, 'B13_R3_SOURCE_BINDING.json')
  checks.sourceBinding = sourceBound(binding, expectedSourceCommit, expectedBaselineCommit)
    && binding.sourceCommitAfter === expectedSourceCommit
    && binding.workingTreeDirtyBefore === false
    && binding.workingTreeDirtyAfter === false
    && Array.isArray(binding.excludedUncommittedPaths)
    && binding.excludedUncommittedPaths.length === 0
  add(failures, checks.sourceBinding, 'R3 source binding is not clean and exact')

  const task = readJson(root, 'B13_R3_TASK_REALITY_COMPLETE_READ.json')
  checks.taskReality = sourceBound(task, expectedSourceCommit, expectedBaselineCommit)
    && task.status === 'PASS'
    && Number(task.requestedTaskIds) >= 501
    && Number(task.uniqueTaskIds) === Number(task.requestedTaskIds)
    && Number(task.chunkSize) <= 100
    && Number(task.chunkCount) === Math.ceil(Number(task.uniqueTaskIds) / Number(task.chunkSize))
    && Number(task.returnedTaskStates) === Number(task.uniqueTaskIds)
    && Number(task.blockedRequiredTasks) >= 1
    && task.h04Severity === 'risk'
    && Number(task.b09BlockedRequiredTasks) >= 1
    && Number(task.legacyReturnedStates) === 100
    && task.legacyIncludesTask501 === false
    && Number(task.task100Chunks) === 1
    && Number(task.task101Chunks) === 2
    && task.pageIndependent === true
  add(failures, checks.taskReality, 'complete Task Reality evidence is incomplete')

  const integration = readJson(root, 'B13_R3_501_FORMAL_INTEGRATION.json')
  checks.integration = sourceBound(integration, expectedSourceCommit, expectedBaselineCommit)
    && integration.status === 'PASS'
    && integration.persistence === 'CommitService'
    && Number(integration.stateRows) >= 501
    && integration.b09 === 'PASS'
    && integration.queryBookHealth === 'PASS'
    && integration.h04 === 'BLOCKED_REQUIRED_RISK'
  add(failures, checks.integration, '501 formal integration evidence is incomplete')

  const b09 = readJson(root, 'B13_R3_B09_SEMANTICS_UNCHANGED.json')
  checks.b09Semantics = sourceBound(b09, expectedSourceCommit, expectedBaselineCommit)
    && b09.status === 'PASS'
    && b09.sharedReader === 'task_states_by_ids_complete'
    && b09.semanticsUnchanged === true
  add(failures, checks.b09Semantics, 'B09 shared reader or semantic boundary evidence is incomplete')

  const product = readJson(root, 'B13_R3_PRODUCT_UX_BOOKPLAN_VOLUMEPLAN.json')
  const m11 = readJson(root, 'B13_R3/MANUAL_BROWSER/M11.json')
  const m11Text = m11?.actualVisibleText ?? ''
  checks.productUx = sourceBound(product, expectedSourceCommit, expectedBaselineCommit)
    && product.status === 'PASS'
    && product.bookPlanVisibleCount === 0
    && product.volumePlanVisibleCount === 0
    && product.productUxGate === 'PASS'
    && (options.requireMutations === false
      || (product.mutationIntroducedExitCode !== 0 && product.mutationRevertedExitCode === 0))
    && m11?.result === 'PASS'
    && m11?.actualVisibleTextSource === 'playwright-dom-innerText'
    && m11?.screenshotPath
    && fs.existsSync(path.join(root, m11.screenshotPath))
    && !/\bBookPlan\b|\bVolumePlan\b/.test(m11Text)
  add(failures, checks.productUx, 'BookPlan/VolumePlan product evidence is not fail-closed')

  const queryPlans = readJson(root, 'B13_R3_CURRENT_QUERY_PLANS.json')
  const plans = Array.isArray(queryPlans?.plans) ? queryPlans.plans : []
  const planByName = new Map(plans.map(plan => [plan.queryName, plan]))
  checks.queryPlans = sourceBound(queryPlans, expectedSourceCommit, expectedBaselineCommit)
    && queryPlans.status === 'PASS'
    && REQUIRED_PLANS.every(name => {
      const plan = planByName.get(name)
      return plan
        && plan.fixture === 'p-b13-r3'
        && Array.isArray(plan.rawExplainRows)
        && plan.rawExplainRows.length > 0
        && typeof plan.sql === 'string'
        && /^[a-f0-9]{64}$/.test(plan.sqlDigest ?? '')
        && plan.sqlDigest === sha256Json({ sql: plan.sql })
        && !R3_INTERNAL_PLAN_PATTERN.test(plan.sql)
    })
  add(failures, checks.queryPlans, 'current formal Query Plan evidence is incomplete or points at compatibility SQL')

  const formalRoute = planByName.get('formal_route_source')
  const projection = planByName.get('planning_state_projection_lookup')
  const payload = planByName.get('volume_plan_payload_lookup')
  const taskChunk = planByName.get('task_state_chunk_le_100')
  checks.currentH03 = Boolean(formalRoute)
    && formalRoute.sql.includes('volume_plan_index')
    && formalRoute.sql.includes('canonical_heads')
    && formalRoute.rawExplainRows.some(row => /volume_plan_index/i.test(row))
    && formalRoute.rawExplainRows.some(row => /volume_planning_state_projection/i.test(row))
  checks.currentH04 = Boolean(projection)
    && Boolean(taskChunk)
    && taskChunk.sql.includes('state_deltas')
    && /LIMIT\s+\?3/.test(taskChunk.sql)
    && taskChunk.rawExplainRows.some(row => /state_deltas/i.test(row))
  checks.taskChunk = checks.currentH04
    && taskChunk.sql.includes('json_each')
  add(failures, checks.currentH03, 'formal H03 Query Plan does not prove the current route/index/projection query')
  add(failures, checks.currentH04, 'formal H04 Query Plan does not prove the current task reality query')
  add(failures, checks.taskChunk, 'bounded task chunk Query Plan is missing')

  const performanceFiles = ['formal-route.explain.txt', 'planning-state.explain.txt', 'volume-plan-payload.explain.txt', 'task-state-chunk.explain.txt']
  checks.performanceLogs = performanceFiles.every(file => {
    const value = fs.existsSync(path.join(root, 'B13_R3/PERFORMANCE', file))
      ? fs.readFileSync(path.join(root, 'B13_R3/PERFORMANCE', file), 'utf8')
      : ''
    return value.includes('sourceCommit=' + expectedSourceCommit)
      && value.includes('rawExplainRows=')
      && value.includes('sqlDigest=')
  })
  add(failures, checks.performanceLogs, 'R3 Query Plan raw logs are missing source binding')

  const dual = readJson(root, 'B13_R3_DUAL_READER_UTF8.json')
  checks.dualReader = sourceBound(dual, expectedSourceCommit, expectedBaselineCommit)
    && dual.status === 'PASS'
    && dual.readerA === 'python-zipfile'
    && typeof dual.readerB === 'string'
    && dual.readerB !== 'python-zipfile'
    && dual.entryCountA === dual.entryCountB
    && dual.entrySetsEqual === true
    && dual.extractFileCountA === dual.extractFileCountB
    && dual.contentHashesEqual === true
    && dual.utf8FlagFailures === 0
    && dual.mojibakeFindings === 0
    && dual.readerAOk === true
    && dual.readerBOk === true
    && REQUIRED_READER_FILES.every(file => fs.existsSync(path.join(root, 'B13_R3/PACKAGE_READERS', file)))
  add(failures, checks.dualReader, 'dual Reader evidence is missing, mismatched, or not actually executed')

  const noAuthority = readJson(root, 'B13_R3_NO_AUTHORITY_MUTATION.json')
  const domains = Array.isArray(noAuthority?.domains) ? noAuthority.domains : []
  checks.noAuthority = sourceBound(noAuthority, expectedSourceCommit, expectedBaselineCommit)
    && noAuthority.status === 'PASS'
    && domains.length >= 8
    && domains.every(domain => domain.beforeCount > 0 && domain.beforeCount === domain.afterCount && domain.beforeDigest === domain.afterDigest && domain.unchanged === true)
    && noAuthority.projectionChanges === 0
  add(failures, checks.noAuthority, 'R2 authority read-only regression is not preserved')

  const noRemote = readJson(root, 'B13_R3_NO_REMOTE_CALL.json')
  checks.noRemote = sourceBound(noRemote, expectedSourceCommit, expectedBaselineCommit)
    && noRemote.status === 'PASS'
    && noRemote.modelRunsBefore === noRemote.modelRunsAfter
  add(failures, checks.noRemote, 'no-remote regression is not preserved')

  if (options.requireMutations !== false) {
    const mutations = readJson(root, 'B13_R3_MUTATION_GATES.json')
    const required = ['MUT-R3-01', 'MUT-R3-02', 'MUT-R3-03A', 'MUT-R3-03B']
    checks.mutations = sourceBound(mutations, expectedSourceCommit, expectedBaselineCommit)
      && mutations.status === 'PASS'
      && required.every(id => {
        const item = mutations.mutations?.find(candidate => candidate.id === id)
        return item?.status === 'PASS'
          && item.introducedStatus === 'FAIL'
          && item.revertedStatus === 'PASS'
          && item.reverted === true
      })
    add(failures, checks.mutations, 'R3 mutation evidence does not prove fail then revert for all blockers')
  }

  const packageConsistency = readJson(root, 'B13_R3_PACKAGE_CONSISTENCY.json')
  checks.packageConsistency = sourceBound(packageConsistency, expectedSourceCommit, expectedBaselineCommit)
    && packageConsistency.status === 'PASS'
  add(failures, checks.packageConsistency, 'R3 package consistency evidence is incomplete')

  const architecture = readJson(root, 'B13_R3_ARCHITECTURE.json')
  checks.architecture = sourceBound(architecture, expectedSourceCommit, expectedBaselineCommit)
    && architecture.status === 'PASS'
  add(failures, checks.architecture, 'R3 architecture evidence is incomplete')

  const regression = readJson(root, 'B13_R3_REGRESSION.json')
  checks.regression = sourceBound(regression, expectedSourceCommit, expectedBaselineCommit)
    && regression.status === 'PASS'
  add(failures, checks.regression, 'R3 regression evidence is incomplete')

  const changed = readJson(root, 'B13_R3_CHANGED_FILES.json')
  checks.changedFiles = sourceBound(changed, expectedSourceCommit, expectedBaselineCommit)
    && changed.status === 'PASS'
    && Array.isArray(changed.outOfScope)
    && changed.outOfScope.length === 0
    && Array.isArray(changed.paths)
    && changed.paths.every(item => ['B13-R3-01', 'B13-R3-02', 'B13-R3-03'].includes(item.task))
  add(failures, checks.changedFiles, 'R3 changed-file scope is incomplete')

  const forbidden = readJson(root, 'B13_R3_FORBIDDEN_SCOPE.json')
  checks.forbiddenScope = sourceBound(forbidden, expectedSourceCommit, expectedBaselineCommit)
    && forbidden.status === 'PASS'
    && Array.isArray(forbidden.findings)
    && forbidden.findings.length === 0
  add(failures, checks.forbiddenScope, 'R3 forbidden scope evidence is not clean')

  const result = {
    schema: 'b13-r3-evidence-truthfulness',
    sourceCommit: expectedSourceCommit,
    repairBaselineCommit: expectedBaselineCommit,
    checks,
    failures,
    status: failures.length === 0 ? 'PASS' : 'FAIL',
  }
  return result
}

if (process.argv[1]?.endsWith('verify-b13-r3-evidence-truthfulness.mjs')) {
  const evidenceDir = process.env.B13_R3_EVIDENCE_DIR
  const sourceCommit = process.env.B13_R3_SOURCE_COMMIT
  const baselineCommit = process.env.B13_R3_REPAIR_BASELINE_COMMIT
  if (!evidenceDir || !sourceCommit || !baselineCommit) {
    console.error('B13_R3_EVIDENCE_INPUT_REQUIRED')
    process.exitCode = 1
  } else {
    const result = evaluateR3EvidenceTruthfulness(evidenceDir, sourceCommit, baselineCommit, {
      requireMutations: process.env.B13_R3_REQUIRE_MUTATIONS !== 'false',
    })
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
