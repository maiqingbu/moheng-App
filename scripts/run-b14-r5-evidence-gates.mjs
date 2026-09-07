import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14R5Acceptance, R5_REPAIR_BASELINE, B13_FROZEN_COMMIT } from './generate-b14-r5-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R5_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r5-evidence-')))
const base = path.join(evidenceDir, 'B14_R5')
const dirs = ['AUTOMATIC', 'INTEGRATION', 'MUTATIONS', 'PERFORMANCE', 'PACKAGE_READERS']
for (const dir of dirs) fs.mkdirSync(path.join(base, dir), { recursive: true })
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim()
const sourceCommit = git(['rev-parse', 'HEAD'])
const dirtyBefore = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const commands = {}
const evidence = {}
const run = (name, command, args, env = {}) => {
  const started = Date.now()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 512 * 1024 * 1024 })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const rawLogPath = `B14_R5/AUTOMATIC/${name}.log`
  fs.writeFileSync(path.join(evidenceDir, rawLogPath), output)
  const record = { name, command: [command, ...args].join(' '), startedAt: new Date(started).toISOString(), finishedAt: new Date().toISOString(), durationMs: Date.now() - started, exitCode: result.status ?? 1, status: result.status === 0 ? 'PASS' : 'FAIL', rawLogPath, sourceCommit, output }
  commands[name] = record
  return record
}
const write = (name, value) => { fs.writeFileSync(path.join(evidenceDir, name), `${JSON.stringify(value, null, 2)}\n`); evidence[name] = value }
const status = value => value ? 'PASS' : 'FAIL'

const architecture = run('architecture', 'node', ['scripts/verify-b14-r5-architecture.mjs'], { B14_R5_SOURCE_COMMIT: sourceCommit })
const integration = run('integration', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', '--', '--nocapture'])
const scale = run('scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_r2_scale', '--', '--nocapture'])
const mappingUi = run('mapping_ui', 'npx', ['vitest', 'run', 'tests/store/b14-r5-mapping-ui.test.ts'])
const mutations = run('mutations', 'node', ['scripts/run-b14-r5-mutations.mjs'], { B14_R5_EVIDENCE_DIR: evidenceDir })
const frozen = run('frozen_inputs', 'node', ['scripts/verify-b14-r5-frozen-inputs.mjs'])
const ux = run('product_ux', 'npm', ['run', 'verify:product-ux'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
const architectureRegression = run('architecture_regression', 'npm', ['run', 'verify'])
const b13Regression = run('b13_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health', '--test', 'b13_r2_authority', '--test', 'b13_r2_integration', '--test', 'b13_r3', '--test', 'b13_scale'])
const nodeTests = run('node_tests', 'npm', ['test'])
const vitest = run('vitest', 'npx', ['vitest', 'run', 'tests/store'])
const fmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const check = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const clippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const rustTests = run('rust_tests', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const historicalRestore = run('historical_b01_restore', 'git', ['restore', '--source', sourceCommit, '--', 'evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json'])

const integrationPass = integration.status === 'PASS'
const mappingPass = mappingUi.status === 'PASS' && architecture.status === 'PASS' && !/mappingWorkflow\.fulfillmentCandidates/.test(fs.readFileSync(path.join(root, 'src/ui/pages/Completion.vue'), 'utf8'))
const runtimePass = integrationPass && integration.output.includes('B14_R5_RUNTIME_EMPTY_SCOPE|taskPositive=PASS|taskNegative=PASS|threadPositive=PASS|threadNegative=PASS') && integration.output.includes('B14_R5_RUNTIME_EXPLICIT|scopedTask=PASS|otherTaskExcluded=true|crossProjectUnsupported=PASS')
const noAuthorityMatch = integration.output.match(/B14_R5_NO_AUTHORITY_REAL\|domains=(\[.*\])/)
const noAuthorityDomains = noAuthorityMatch ? JSON.parse(noAuthorityMatch[1]) : []
const noAuthorityPass = integrationPass && noAuthorityDomains.length === 10 && noAuthorityDomains.every(domain => domain.beforeCount > 0 && domain.beforeCount === domain.afterCount && domain.beforeDigest === domain.afterDigest && domain.unchanged === true)
const continuityPass = integrationPass && integration.output.includes('B14_R5_CONTINUITY_REAL|sourceIndex101=thread:obligation:r5:101|sourceIndex501=thread:obligation:r5:501|hardBlocker101=true|hardBlocker501=true|completeRows=501')
const mutationJson = fs.existsSync(path.join(evidenceDir, 'B14_R5_MUTATION_GATES.json')) ? JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R5_MUTATION_GATES.json'))): null
const mutationPass = mutations.status === 'PASS' && mutationJson?.status === 'PASS' && mutationJson.mutations?.length === 6 && mutationJson.mutations.every(item => item.status === 'PASS' && item.introducedExpectedFailure && item.revertedExit === 0)
const planLines = [...scale.output.matchAll(/^B14_R3_QUERY_PLAN\|name=([^|]+)\|sqlDigest=([^|]+)\|planDigest=([^|]+)\|plan=(.*?)\|rowCount=(\d+)\|rows=(.*)$/gm)]
const queryPlans = planLines.map(([, queryName, sqlDigest, planDigest, plan, rowCount, rows]) => ({ queryName, sqlDigest, planDigest, plan, rowCount: Number(rowCount), rows: JSON.parse(rows), rawLogPath: `B14_R5/PERFORMANCE/${queryName.replaceAll(/[^a-zA-Z0-9_-]+/g, '_')}.explain.json`, sourceCommit, status: 'PASS' }))
for (const plan of queryPlans) fs.writeFileSync(path.join(evidenceDir, plan.rawLogPath), `${JSON.stringify(plan, null, 2)}\n`)
const queryPlansPass = scale.status === 'PASS' && queryPlans.length === 15 && queryPlans.every(plan => plan.rowCount > 0 && plan.rows.length === plan.rowCount)
const changed = git(['diff', '--name-status', `${R5_REPAIR_BASELINE}..${sourceCommit}`]).split('\n').filter(Boolean)
const forbidden = changed.filter(line => /creative-skills\/v1\.2\.0|ExportRuntime|ReleaseOwner|ReleaseCertification|\bB15\b|\bRC1\b/.test(line))
const sourceBinding = { schema: 'b14-r5-source-binding', batch: 'B14-R5', repairBaselineCommit: R5_REPAIR_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: git(['status', '--porcelain', '--untracked-files=all']) !== '', excludedUncommittedPaths: [], b15Started: false, rc1Started: false, status: !dirtyBefore && forbidden.length === 0 ? 'PASS' : 'FAIL' }
write('B14_R5_SOURCE_BINDING.json', sourceBinding)
write('B14_R5_CHANGED_FILES.json', { schema: 'b14-r5-changed-files', sourceCommit, changed, forbidden, status: forbidden.length ? 'FAIL' : 'PASS' })
write('B14_R5_PREBATCH_FREEZE.json', { schema: 'b14-r5-prebatch-freeze', repairBaselineCommit: R5_REPAIR_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, creativeSkillsVersion: '1.2.0', creativeSkillsChanges: changed.filter(line => line.includes('creative-skills/v1.2.0')), status: changed.some(line => line.includes('creative-skills/v1.2.0')) ? 'FAIL' : 'PASS' })
write('B14_R5_MAPPING_UI_SCOPE.json', { schema: 'b14-r5-mapping-ui-scope', sourceCommit, reservationScoped: mappingPass, payoffScoped: mappingPass, crossResponsibilityLeak: mappingPass ? 0 : 'FAIL', noRelatedFallback: mappingPass, bounded2000: mappingPass, rawLogPath: commands.mapping_ui.rawLogPath, status: status(mappingPass) })
write('B14_R5_RUNTIME_RECHECK.json', { schema: 'b14-r5-runtime-recheck', sourceCommit, emptyRefTaskPositive: runtimePass, emptyRefTaskNegative: runtimePass, emptyRefThreadPositive: runtimePass, emptyRefThreadNegative: runtimePass, explicitRefsScoped: runtimePass, invalidRefsUnsupported: runtimePass, noExtraModel: runtimePass, noAuthorityWrite: runtimePass, rawLogPath: commands.integration.rawLogPath, status: status(runtimePass) })
write('B14_R5_NO_AUTHORITY_MUTATION.json', { schema: 'b14-r5-no-authority-mutation', sourceCommit, domains: noAuthorityDomains, realCounts: noAuthorityPass, countDigestUnchanged: noAuthorityPass, rawLogPath: commands.integration.rawLogPath, status: status(noAuthorityPass) })
write('B14_R5_CONTINUITY_101_501.json', { schema: 'b14-r5-continuity-101-501', sourceCommit, sourceIndex101: 'thread:obligation:r5:101', sourceIndex501: 'thread:obligation:r5:501', hardBlocker101: continuityPass, hardBlocker501: continuityPass, completeRows: 501, rawLogPath: commands.integration.rawLogPath, status: status(continuityPass) })
write('B14_R5_QUERY_PLANS.json', { schema: 'b14-r5-query-plans', sourceCommit, expectedCount: 15, queryPlans, allRows: queryPlansPass, status: status(queryPlansPass) })
write('B14_R5_FROZEN_INPUTS_CHECK.json', { schema: 'b14-r5-frozen-inputs-check', sourceCommit, status: frozen.status, rawLogPath: commands.frozen_inputs.rawLogPath })
write('B14_R5_MUTATION_GATES.json', mutationJson ?? { schema: 'b14-r5-mutation-gates', sourceCommit, status: 'FAIL', mutations: [] })
const regressionPass = [architectureRegression, b13Regression, nodeTests, vitest, typecheck, build, buildDemo, contracts, rustTests, fmt, check, clippy].every(item => item.status === 'PASS')
write('B14_R5_REGRESSION.json', { schema: 'b14-r5-regression', sourceCommit, regressionPass, commands: Object.keys(commands), status: status(regressionPass) })
write('B14_R5_EVIDENCE_TRUTHFULNESS.json', { schema: 'b14-r5-evidence-truthfulness', sourceCommit, actualNoAuthorityCounts: noAuthorityPass, actualContinuity101501: continuityPass, queryPlanRowsCaptured: queryPlansPass, mutationExecutable: mutationPass, frozenInputShaVerified: frozen.status === 'PASS', status: status(noAuthorityPass && continuityPass && queryPlansPass && mutationPass && frozen.status === 'PASS') })
const gates = {
  mappingUiScopedCandidates: { status: status(mappingPass), blocking: true, source: 'B14_R5_MAPPING_UI_SCOPE.json' },
  runtimeRecheckMeaningful: { status: status(runtimePass), blocking: true, source: 'B14_R5_RUNTIME_RECHECK.json' },
  noAuthorityAllDomainsNonempty: { status: status(noAuthorityPass), blocking: true, source: 'B14_R5_NO_AUTHORITY_MUTATION.json' },
  continuity101Real: { status: status(continuityPass), blocking: true, source: 'B14_R5_CONTINUITY_101_501.json' },
  continuity501Real: { status: status(continuityPass), blocking: true, source: 'B14_R5_CONTINUITY_101_501.json' },
  queryPlansAllRows: { status: status(queryPlansPass), blocking: true, source: 'B14_R5_QUERY_PLANS.json' },
  frozenInputsExactSha: { status: frozen.status, blocking: true, source: 'B14_R5_FROZEN_INPUTS_CHECK.json' },
  dualReaderRawEvidence: { status: 'FAIL', blocking: true, source: 'package not built' },
  mutationBehavioral: { status: status(mutationPass), blocking: true, source: 'B14_R5_MUTATION_GATES.json' },
  evidenceTruthfulness: { status: status(noAuthorityPass && continuityPass && queryPlansPass && mutationPass && frozen.status === 'PASS'), blocking: true, source: 'B14_R5_EVIDENCE_TRUTHFULNESS.json' },
  architecture: { status: architecture.status, blocking: true, source: commands.architecture.rawLogPath }, moduleBoundary: { status: moduleBoundary.status, blocking: true, source: commands.module_boundary.rawLogPath },
  b13Regression: { status: b13Regression.status, blocking: true, source: commands.b13_regression.rawLogPath }, b15Boundary: { status: forbidden.length ? 'FAIL' : 'PASS', blocking: true, source: 'B14_R5_CHANGED_FILES.json' }, rc1Boundary: { status: forbidden.length ? 'FAIL' : 'PASS', blocking: true, source: 'B14_R5_CHANGED_FILES.json' },
  rustFmt: { status: fmt.status, blocking: true, source: commands.rust_fmt.rawLogPath }, rustCheck: { status: check.status, blocking: true, source: commands.rust_check.rawLogPath }, rustClippy: { status: clippy.status, blocking: true, source: commands.rust_clippy.rawLogPath }, rustTests: { status: rustTests.status, blocking: true, source: commands.rust_tests.rawLogPath }, nodeTests: { status: nodeTests.status, blocking: true, source: commands.node_tests.rawLogPath }, vitest: { status: vitest.status, blocking: true, source: commands.vitest.rawLogPath }, typecheck: { status: typecheck.status, blocking: true, source: commands.typecheck.rawLogPath }, build: { status: build.status, blocking: true, source: commands.build.rawLogPath }, buildDemo: { status: buildDemo.status, blocking: true, source: commands.build_demo.rawLogPath }, contracts: { status: contracts.status, blocking: true, source: commands.contracts.rawLogPath }, productUx: { status: ux.status, blocking: true, source: commands.product_ux.rawLogPath }, scale10m: { status: scale.status, blocking: true, source: commands.scale.rawLogPath }, pageIndependentTruth: { status: scale.status, blocking: true, source: commands.scale.rawLogPath },
  packageAsciiRoot: { status: 'FAIL', blocking: true, source: 'not packaged' }, packageNoMacosx: { status: 'FAIL', blocking: true, source: 'not packaged' }, packageUtf8: { status: 'FAIL', blocking: true, source: 'not packaged' }, packageMojibake: { status: 'FAIL', blocking: true, source: 'not packaged' }, packageInventory: { status: 'FAIL', blocking: true, source: 'not packaged' }, filesSha256: { status: 'FAIL', blocking: true, source: 'not packaged' }, packageIntegrity: { status: 'FAIL', blocking: true, source: 'not packaged' }, secretScan: { status: 'FAIL', blocking: true, source: 'not packaged' },
}
const report = { schema: 'b14-r5-gate-results', batch: 'B14-R5', repairBaselineCommit: R5_REPAIR_BASELINE, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: git(['status', '--porcelain', '--untracked-files=all']) !== '', excludedUncommittedPaths: [], commands, gates, evidence, historicalRestore, nextBatchAllowed: false }
report.status = Object.values(gates).every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
fs.writeFileSync(path.join(evidenceDir, 'gate-results-r5.json'), `${JSON.stringify(report, null, 2)}\n`)
const acceptance = generateB14R5Acceptance(evidenceDir)
console.log(`B14_R5_EVIDENCE_RESULT sourceCommit=${sourceCommit} status=${acceptance.status} evidenceDir=${evidenceDir}`)
if (acceptance.status !== 'PASS') process.exitCode = 1
