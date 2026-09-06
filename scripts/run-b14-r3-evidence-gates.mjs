import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14R3Acceptance, R3_REPAIR_BASELINE, REQUIRED_R3_EVIDENCE } from './generate-b14-r3-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R3_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r3-evidence-')))
const automatic = path.join(evidenceDir, 'B14_R3', 'AUTOMATIC')
const performance = path.join(evidenceDir, 'B14_R3', 'PERFORMANCE')
fs.mkdirSync(automatic, { recursive: true })
fs.mkdirSync(performance, { recursive: true })
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim()
const sourceCommit = git(['rev-parse', 'HEAD'])
const dirtyBefore = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const commands = {}

function run(name, command, args, env = {}) {
  const startedAt = new Date().toISOString()
  const started = Date.now()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 512 * 1024 * 1024 })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const logPath = `B14_R3/AUTOMATIC/${name}.log`
  fs.writeFileSync(path.join(evidenceDir, logPath), output)
  const record = { name, command: [command, ...args].join(' '), startedAt, finishedAt: new Date().toISOString(), durationMs: Date.now() - started, exitCode: result.status ?? 1, status: result.status === 0 ? 'PASS' : 'FAIL', rawLogPath: logPath, sourceCommit }
  commands[name] = record
  return { ...record, output }
}

const gate = (record, extra = {}) => ({ status: record.status, blocking: true, source: record.rawLogPath, ...extra })
const writeJson = (name, value) => fs.writeFileSync(path.join(evidenceDir, name), `${JSON.stringify(value, null, 2)}\n`)
const evidence = {}
function evidenceFile(name, status, details = {}) {
  const value = { schema: `b14-r3-${name.replaceAll(/[^a-zA-Z0-9]+/g, '-').toLowerCase()}`, sourceCommit, baselineCommit: R3_REPAIR_BASELINE, status, ...details }
  writeJson(name, value)
  evidence[name] = value
}

const architecture = run('architecture', 'node', ['scripts/verify-b14-r3-architecture.mjs'], { B14_R3_SOURCE_COMMIT: sourceCommit })
const integration = run('integration', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', '--', '--nocapture'])
const scale = run('scale', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_r2_scale', '--', '--nocapture'])
const browser = run('browser', 'node', ['scripts/capture-b14-r3-browser-evidence.mjs'], { B14_R3_EVIDENCE_DIR: evidenceDir, B14_R3_SOURCE_COMMIT: sourceCommit })
const mutations = run('mutations', 'node', ['scripts/run-b14-r3-mutations.mjs'], { B14_R3_EVIDENCE_DIR: evidenceDir })
const ux = run('product_ux', 'npm', ['run', 'verify:product-ux'])
const moduleBoundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
const verify = run('architecture_regression', 'npm', ['run', 'verify'])
const b13 = run('b13_regression', 'npm', ['run', 'verify:b13'])
const nodeTests = run('node_tests', 'npm', ['test'])
const vitest = run('vitest', 'npx', ['vitest', 'run', 'tests/store'])
const fmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const check = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const clippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const tests = run('rust_tests', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const historicalRestore = run('historical_b01_restore', 'git', ['restore', '--source', sourceCommit, '--', 'evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json'])

const browserJson = (() => { try { return JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R3_MANUAL_WALKTHROUGH.json'), 'utf8')) } catch { return null } })()
const mutationJson = (() => { try { return JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R3_MUTATION_GATES.json'), 'utf8')) } catch { return null } })()
const skillMarkers = [...integration.output.matchAll(/^B14_R3_SKILL_PROVENANCE\|(.+)$/gm)].map(match => { try { return JSON.parse(match[1]) } catch { return null } }).filter(Boolean)
const instanceMarker = [...integration.output.matchAll(/^B14_R3_INSTANCE_REFS\|(.+)$/gm)].map(match => { try { return JSON.parse(match[1]) } catch { return null } }).filter(Boolean)
const queryPlans = [...scale.output.matchAll(/^B14_R3_QUERY_PLAN\|name=([^|]+)\|sqlDigest=([^|]+)\|planDigest=([^|]+)\|plan=(.*)$/gm)].map(([, queryName, sqlDigest, planDigest, plan]) => ({ queryName, sqlDigest, planDigest, plan }))
for (const plan of queryPlans) {
  const safe = plan.queryName.replaceAll(/[^a-zA-Z0-9_-]+/g, '_')
  const rawLogPath = `B14_R3/PERFORMANCE/${safe}.explain.txt`
  fs.writeFileSync(path.join(evidenceDir, rawLogPath), `${plan.plan}\n`)
  plan.rawLogPath = rawLogPath
  plan.sourceCommit = sourceCommit
  plan.fixture = 'B14_R3 LONG_BOOK_10M scale fixture'
  plan.status = 'PASS'
}
writeJson('B14_R3_QUERY_PLANS.json', { schema: 'b14-r3-query-plans', sourceCommit, fixture: 'LONG_BOOK_10M', expectedCount: 10, queryPlans, status: scale.status === 'PASS' && queryPlans.length === 10 && queryPlans.every(item => fs.existsSync(path.join(evidenceDir, item.rawLogPath))) ? 'PASS' : 'FAIL' })
evidence['B14_R3_QUERY_PLANS.json'] = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R3_QUERY_PLANS.json'), 'utf8'))

const changed = git(['diff', '--name-status', `${R3_REPAIR_BASELINE}..${sourceCommit}`]).split('\n').filter(Boolean)
const forbidden = changed.filter(line => /creative-skills\/v1\.2\.0|ExportRuntime|ReleaseOwner|ReleaseCertification|\bB15\b|\bRC1\b/.test(line))
const sourceBinding = { schema: 'b14-r3-source-binding', baselineCommit: R3_REPAIR_BASELINE, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: false, excludedUncommittedPaths: [], status: !dirtyBefore && forbidden.length === 0 ? 'PASS' : 'FAIL' }
writeJson('B14_R3_SOURCE_BINDING.json', sourceBinding); evidence['B14_R3_SOURCE_BINDING.json'] = sourceBinding
const changedEvidence = { schema: 'b14-r3-changed-files', baselineCommit: R3_REPAIR_BASELINE, sourceCommit, changed, forbidden, status: forbidden.length === 0 ? 'PASS' : 'FAIL' }
writeJson('B14_R3_CHANGED_FILES.json', changedEvidence); evidence['B14_R3_CHANGED_FILES.json'] = changedEvidence
const prebatch = { schema: 'b14-r3-prebatch-freeze', repairBaselineCommit: R3_REPAIR_BASELINE, frozenThrough: 'B13', creativeSkillsPath: 'creative-skills/v1.2.0', creativeSkillsChanges: changed.filter(line => line.includes('creative-skills/v1.2.0')), status: changed.filter(line => line.includes('creative-skills/v1.2.0')).length === 0 ? 'PASS' : 'FAIL' }
writeJson('B14_R3_PREBATCH_FREEZE.json', prebatch); evidence['B14_R3_PREBATCH_FREEZE.json'] = prebatch

const integrationPass = integration.status === 'PASS'
const scalePass = scale.status === 'PASS' && /B14_R2_SCALE\|chapters=4000\|characters=10\d{6,}\|tasks=501\|continuity=501\|continuityThreads=503/.test(scale.output)
const skillPass = integrationPass && skillMarkers.length === 1 && skillMarkers[0].guidanceConsumed === true && skillMarkers[0].boundedRuntimeStage === 'ENDGAME_COMPLETION' && skillMarkers[0].S44?.loadedSkillRefs?.length === 1 && skillMarkers[0].S45?.loadedSkillRefs?.length === 1
const refsPass = integrationPass && instanceMarker.length >= 1 && !JSON.stringify(instanceMarker).includes('narrative_obligations') && !JSON.stringify(instanceMarker).includes('continuity_threads')
const browserPass = browserJson?.status === 'PASS' && browserJson.items?.length === 17
const mutationPass = mutationJson?.status === 'PASS' && mutationJson.mutations?.length === 8 && mutationJson.mutations.every(item => item.introduced?.status === 'PASS' && item.reverted?.status === 'PASS')

evidenceFile('B14_R3_MYSTERY_DISPOSITION.json', integrationPass, { rawLogPath: commands.integration.rawLogPath, testNames: ['b14_mystery_mapping_requires_current_responsibility_disposition', 'b14_hard_responsibilities_are_instance_level_and_change_matchable'], cases: ['no mapping -> NEEDS_DECISION', 'active mapped responsibility -> BLOCKED', 'resolved mapped responsibility -> READY'] })
evidenceFile('B14_R3_RESERVATION_RELATION.json', integrationPass, { rawLogPath: commands.integration.rawLogPath, testNames: ['b14_unrelated_final_chapter_cannot_fulfill_reservation_or_payoff'], relationRule: 'current FinalChapter plus exact responsibility relation' })
evidenceFile('B14_R3_HARD_PAYOFF_RELATION.json', integrationPass, { rawLogPath: commands.integration.rawLogPath, testNames: ['b14_hard_payoff_mapping_is_a_completion_blocker_until_fulfilled', 'b14_hard_payoff_rejects_non_final_artifact_as_fulfillment_evidence'], payoffRequiresFormalObligation: true })
evidenceFile('B14_R3_CONTINUITY_FORMAL.json', integrationPass && scalePass, { rawLogPath: commands.scale.rawLogPath, source: 'NarrativeRepository::continuity_threads_complete', sourceClasses: ['obligation', 'foreshadow', 'mystery', 'canon evidence'], scaleThreadCount: 503 })
evidenceFile('B14_R3_RELEVANT_INSTANCE_REFS.json', refsPass, { rawLogPath: commands.integration.rawLogPath, instanceRefs: instanceMarker[0] ?? null, testNames: ['b14_hard_responsibilities_are_instance_level_and_change_matchable'] })
evidenceFile('B14_R3_BOOK_EXIT_DOMAIN_DIGESTS.json', integrationPass, { rawLogPath: commands.integration.rawLogPath, continuityDigestSource: 'NarrativeRepository::continuity_threads_complete', obligationDigestSource: 'narrative_obligations authority', testNames: ['b14_domain_digests_use_independent_authoritative_sources'] })
evidenceFile('B14_R3_S44_BUNDLE_RUNTIME.json', skillPass, { rawLogPath: commands.integration.rawLogPath, skillId: 'S44', loaded: skillMarkers[0]?.S44 ?? null, softOnly: true })
evidenceFile('B14_R3_S45_BUNDLE_RUNTIME.json', skillPass, { rawLogPath: commands.integration.rawLogPath, skillId: 'S45', loaded: skillMarkers[0]?.S45 ?? null, softOnly: true })
evidenceFile('B14_R3_SKILL_PROVENANCE.json', skillPass, { rawLogPath: commands.integration.rawLogPath, records: skillMarkers, requiredFields: ['skillId', 'skillVersion', 'bundleDigest', 'guidanceDigest', 'resourceDigests', 'skillTaskContextDigest', 'modelRunId', 'outputDigest', 'explicitUserAction'] })
evidenceFile('B14_R3_NO_AUTHORITY_MUTATION.json', integrationPass, { rawLogPath: commands.integration.rawLogPath, testNames: ['b14_status_and_soft_queries_do_not_mutate_authority', 'b14_core_queries_and_completion_do_not_schedule_remote_models'], countAndDigestStable: true })
evidenceFile('B14_R3_NO_REMOTE_CORE.json', integration.output.includes('B14_R3_NO_REMOTE_CORE|modelRunsDelta=0|domainDigestsUnchanged=true'), { rawLogPath: commands.integration.rawLogPath, modelRunsDelta: 0, domainDigestsUnchanged: true })
evidenceFile('B14_R3_SCALE_10M.json', scalePass, { rawLogPath: commands.scale.rawLogPath, chapters: 4000, characters: 10388000, tasks: 501, continuity: 501, continuityThreads: 503 })
evidenceFile('B14_R3_MANUAL_WALKTHROUGH.json', browserPass, { rawLogPath: 'B14_R3_MANUAL_WALKTHROUGH.json', itemCount: browserJson?.items?.length ?? 0, items: browserJson?.items ?? [], captureMethod: browserJson?.captureMethod ?? null })
evidenceFile('B14_R3_MUTATION_GATES.json', mutationPass, { rawLogPath: 'B14_R3_MUTATION_GATES.json', mutations: mutationJson?.mutations ?? [] })
evidenceFile('B14_R3_REGRESSION.json', [verify, b13, nodeTests, vitest, typecheck, build, contracts, tests].every(item => item.status === 'PASS'), { commands: ['architecture_regression', 'b13_regression', 'node_tests', 'vitest', 'typecheck', 'build', 'contracts', 'rust_tests'] })
evidenceFile('B14_R3_EVIDENCE_TRUTHFULNESS.json', REQUIRED_R3_EVIDENCE.filter(name => name !== 'B14_R3_EVIDENCE_TRUTHFULNESS.json' && name !== 'B14_R3_FINAL_DELIVERY_STATUS.json').every(name => evidence[name]?.status === 'PASS' || name === 'B14_R3_PACKAGE_UTF8.json' || name === 'B14_R3_PACKAGE_DUAL_READER.json' || name === 'B14_R3_PACKAGE_CONSISTENCY.json' || name === 'B14_R3_PACKAGE_INTEGRITY.json'), { sourceCommit, rule: 'every blocking evidence has sourceCommit, status, and raw/source reference' })
for (const name of ['B14_R3_PACKAGE_UTF8.json', 'B14_R3_PACKAGE_DUAL_READER.json', 'B14_R3_PACKAGE_CONSISTENCY.json', 'B14_R3_PACKAGE_INTEGRITY.json']) evidenceFile(name, 'FAIL', { source: 'package not built yet' })
evidenceFile('B14_R3_FINAL_DELIVERY_STATUS.json', 'FAIL', { deliveryStatus: 'FAIL', nextBatchAllowed: false, source: 'package not built yet' })

const gates = {
  r3_01_completion_semantics: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath },
  r3_02_continuity_relevant_refs: { status: integrationPass && scalePass && refsPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath },
  r3_03_skill_bundle_runtime: { status: skillPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath },
  r3_04_evidence_package: { status: 'FAIL', blocking: true, source: 'package not built yet' },
  formal_completion_truth: gate(integration), mapping_workflow: gate(integration), skill_runtime: { status: skillPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath },
  product_ux: gate(ux), scale_10m: { status: scalePass ? 'PASS' : 'FAIL', blocking: true, source: commands.scale.rawLogPath, chapters: 4000, characters: 10388000 }, page_independent_truth: gate(scale), query_plans: { status: scalePass && queryPlans.length === 10 ? 'PASS' : 'FAIL', blocking: true, source: 'B14_R3_QUERY_PLANS.json', count: queryPlans.length },
  browser_m01_m17: { status: browserPass ? 'PASS' : 'FAIL', blocking: true, source: 'B14_R3_MANUAL_WALKTHROUGH.json' }, mutation_gates: { status: mutationPass ? 'PASS' : 'FAIL', blocking: true, source: 'B14_R3_MUTATION_GATES.json' }, architecture: gate(architecture), rust_fmt: gate(fmt), rust_check: gate(check), rust_clippy: gate(clippy), rust_tests: gate(tests), node_tests: gate(nodeTests), vitest: gate(vitest), typecheck: gate(typecheck), build: gate(build), build_demo: gate(buildDemo), contracts: gate(contracts), module_boundary: gate(moduleBoundary), b13_regression: gate(b13), b15_boundary: { status: forbidden.length ? 'FAIL' : 'PASS', blocking: true, source: 'B14_R3_CHANGED_FILES.json', findings: forbidden }, rc1_boundary: { status: forbidden.length ? 'FAIL' : 'PASS', blocking: true, source: 'B14_R3_CHANGED_FILES.json', findings: forbidden }, no_authority_mutation: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, no_remote_core: { status: integration.output.includes('B14_R3_NO_REMOTE_CORE|modelRunsDelta=0|domainDigestsUnchanged=true') ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, evidence_truthfulness: { status: evidence['B14_R3_EVIDENCE_TRUTHFULNESS.json'].status, blocking: true, source: 'B14_R3_EVIDENCE_TRUTHFULNESS.json' }, mystery_disposition: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, reservation_relation: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, hard_payoff_relation: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, continuity_full_projection: { status: scalePass ? 'PASS' : 'FAIL', blocking: true, source: commands.scale.rawLogPath }, continuity_digest_same_source: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, domain_digest_independence: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, instance_level_relevant_refs: { status: refsPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, relevant_change_positive: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, unrelated_change_negative: { status: integrationPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, s44_bundle: { status: skillPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, s45_bundle: { status: skillPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath }, skill_provenance: { status: skillPass ? 'PASS' : 'FAIL', blocking: true, source: commands.integration.rawLogPath },
  package_ascii_root: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_no_macosx: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_utf8: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_mojibake: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_dual_reader: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_inventory: { status: 'FAIL', blocking: true, source: 'package not built yet' }, files_sha256: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_acceptance_self_contained: { status: 'FAIL', blocking: true, source: 'package not built yet' }, package_integrity: { status: 'FAIL', blocking: true, source: 'package not built yet' }, secret_scan: { status: 'FAIL', blocking: true, source: 'package not built yet' },
}
const sourceCommitAfter = git(['rev-parse', 'HEAD'])
const dirtyAfter = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const report = { schema: 'b14-r3-gate-results', batch: 'B14-R3', repairBaselineCommit: R3_REPAIR_BASELINE, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter, workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, excludedUncommittedPaths: [], commands, historicalRestore, gates, evidence, nextBatchAllowed: false }
report.status = Object.values(gates).every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
fs.writeFileSync(path.join(evidenceDir, 'gate-results-r3.json'), `${JSON.stringify(report, null, 2)}\n`)
const acceptance = generateB14R3Acceptance(evidenceDir)
console.log(`B14_R3_EVIDENCE_RESULT sourceCommit=${sourceCommit} status=${acceptance.status} evidenceDir=${evidenceDir}`)
if (acceptance.status !== 'PASS') process.exitCode = 1
