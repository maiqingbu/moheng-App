import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateAcceptance, H04_BLOCKING_GATES, REQUIRED_BLOCKING_GATES, START_COMMIT } from './generate-b09-h04-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root, 'src-tauri')
const evidence = process.env.B09_H04_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b09-h04-evidence-'))
fs.mkdirSync(evidence, { recursive: true })

function git(args, encoding = 'utf8') {
  return spawnSync('git', args, { cwd: root, encoding })
}

function snapshot() {
  return {
    commit: git(['rev-parse', 'HEAD']).stdout.trim(),
    dirtyPaths: git(['status', '--porcelain=v1', '--untracked-files=all']).stdout.trim().split('\n').filter(Boolean),
  }
}

function run(name, command, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', env: { ...process.env, ...extraEnv } })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const log = `${name}.log`
  fs.writeFileSync(path.join(evidence, log), output)
  return { name, command: [command, ...args].join(' '), status: result.status === 0 ? 'PASS' : 'FAIL', exitCode: result.status ?? 1, output, log }
}

const before = snapshot()
const results = {}
const add = (name, command, args, cwd = root, extraEnv = {}) => { results[name] = run(name, command, args, cwd, extraEnv); return results[name] }

const b09 = add('b09_targeted', 'cargo', ['test', '--test', 'b09_h04', '--', '--nocapture'], rustRoot)
const scale = process.env.B09_H04_RUN_SCALE === '1'
  ? add('b09_h04_scale', 'cargo', ['test', '--test', 'b09_h04_scale', '--', '--ignored', '--nocapture'], rustRoot, { B09_H04_SCALE_EVIDENCE_DIR: evidence })
  : { name: 'b09_h04_scale', command: 'cargo test --test b09_h04_scale -- --ignored --nocapture', status: 'FAIL', exitCode: 77, output: 'B09_H04_SCALE_SKIPPED: set B09_H04_RUN_SCALE=1\n', log: 'b09_h04_scale.log' }
if (scale.status !== 'PASS' && !fs.existsSync(path.join(evidence, scale.log))) {
  fs.writeFileSync(path.join(evidence, scale.log), scale.output)
}
const b08 = add('b08_h03_regression', 'cargo', ['test', '--test', 'b08_h03', '--', '--nocapture'], rustRoot)
const b07 = add('b07_h02_regression', 'cargo', ['test', '--test', 'b07_h02', '--test', 'b07_h02_semantic_negative'], rustRoot)
const b06 = add('b06_h01_regression', 'cargo', ['test', '--test', 'b06_h01', '--', '--nocapture'], rustRoot)
const b05 = add('b05_regression', 'cargo', ['test', '--test', 'b05_narrative'], rustRoot)
const b03 = add('b03_regression', 'cargo', ['test', '--test', 'b03_runtime'], rustRoot)
const b02 = add('b02_regression', 'cargo', ['test', '--test', 'b02_commit_bundle'], rustRoot)
const p01 = add('p01_regression', 'cargo', ['test', '--test', 'p01_provider'], rustRoot)
const cargoAll = add('cargo_all_targets', 'cargo', ['test', '--all-targets'], rustRoot)
const fmt = add('cargo_fmt', 'cargo', ['fmt', '--all', '--', '--check'], rustRoot)
const check = add('cargo_check', 'cargo', ['check', '--all-targets'], rustRoot)
const clippy = add('cargo_clippy', 'cargo', ['clippy', '--all-targets', '--all-features', '--', '-D', 'warnings'], rustRoot)
const node = add('node', 'npm', ['test'])
const vitest = add('vitest', 'npm', ['run', 'test:store'])
const typecheck = add('typecheck', 'npm', ['run', 'typecheck'])
const build = add('build', 'npm', ['run', 'build'])
const contracts = add('contracts', 'npm', ['run', 'verify:contracts'])
const architecture = add('architecture', 'node', ['scripts/verify-b09-h04-architecture.mjs'])
const baseArchitecture = add('base_architecture', 'node', ['scripts/verify-architecture.mjs'])
const moduleBoundary = add('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const m0a = add('m0a', 'node', ['scripts/verify-m0a-evidence.mjs'])

// B01's live-schema tests intentionally refresh two historical evidence files.
// Restore only those exact frozen outputs after the regression sweep. A dirty
// worktree before the run never gets rewritten.
if (before.dirtyPaths.length === 0) {
  for (const file of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json', 'evidence/B01/query-plans.json']) {
    if (git(['diff', '--quiet', '--', file]).status !== 0) {
      const restored = git(['restore', '--source', before.commit, '--', file])
      if (restored.status !== 0) results.frozen_b01_evidence = { name: 'frozen_b01_evidence', command: `git restore --source ${before.commit} -- ${file}`, status: 'FAIL', exitCode: restored.status ?? 1, log: 'gate-results.json', output: restored.stderr ?? '' }
    }
  }
}

const after = snapshot()
const startCommitIsAncestor = git(['merge-base', '--is-ancestor', START_COMMIT, before.commit]).status === 0
const sourceBinding = before.commit === after.commit && before.dirtyPaths.length === 0 && after.dirtyPaths.length === 0
const sourceBindingReport = {
  batch: 'B09_H04',
  startCommit: START_COMMIT,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: after.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  status: sourceBinding ? 'PASS' : 'FAIL',
}
fs.writeFileSync(path.join(evidence, 'B09_H04_SOURCE_BINDING.json'), `${JSON.stringify(sourceBindingReport, null, 2)}\n`)

const sourceStage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b09-h04-source-stage-'))
const archive = spawnSync('git', ['archive', '--format=tar', '--prefix=ai-novel-production-system/', 'HEAD'], { cwd: root, encoding: 'buffer', maxBuffer: 200 * 1024 * 1024 })
const extract = archive.status === 0 ? spawnSync('tar', ['-xf', '-', '-C', sourceStage], { input: archive.stdout, encoding: 'utf8' }) : { status: 1, stdout: '', stderr: 'git archive failed' }
const sourceRoot = path.join(sourceStage, 'ai-novel-production-system')
// Historical B01-B08 evidence is not part of the source-only payload. B09
// evidence is overlaid by the final package step after this smoke check.
try { fs.rmSync(path.join(sourceRoot, 'evidence'), { recursive: true, force: true }) } catch {}
const sourceEntries = []
const walkStage = directory => {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) walkStage(target)
    else sourceEntries.push(path.relative(sourceStage, target))
  }
}
if (fs.existsSync(sourceRoot)) walkStage(sourceRoot)
const forbiddenEntries = sourceEntries.filter(entry => /(^|\/)(?:\.git|node_modules|target|dist)(?:\/|$)|\.db(?:$|[-.])|(?:^|\/)(?:evidence|reports?)\//i.test(entry))
const packageSmoke = archive.status === 0 && extract.status === 0 && fs.existsSync(sourceRoot) && forbiddenEntries.length === 0
fs.writeFileSync(path.join(evidence, 'package_smoke.log'), `${archive.stderr ?? ''}${extract.stdout ?? ''}${extract.stderr ?? ''}\nforbiddenEntries=${forbiddenEntries.length}\nsourceRoot=${sourceRoot}\n`)

// Re-run the architecture and acceptance generator from the same source-only
// shape that a receiver gets.  There is no .git in this stage; the binding file
// and frozen migration hashes are the only baseline authority available there.
const receiverEvidence = path.join(sourceRoot, 'evidence/B09_H04')
fs.mkdirSync(receiverEvidence, { recursive: true })
fs.copyFileSync(path.join(evidence, 'B09_H04_SOURCE_BINDING.json'), path.join(receiverEvidence, 'B09_H04_SOURCE_BINDING.json'))
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify({
  schema: 'b09-h04-gate-results-r1',
  batch: 'B09_H04',
  ...sourceBindingReport,
  sourceBinding: 'PASS',
  gates: Object.fromEntries(REQUIRED_BLOCKING_GATES.map(name => [name, { status: 'PASS', source: 'receiver-schema-smoke' }])),
}, null, 2)}\n`)
const freshArchitecture = run('fresh_package_architecture', 'node', ['scripts/verify-b09-h04-architecture.mjs'], sourceRoot)
const freshBaseArchitecture = run('fresh_package_base_architecture', 'node', ['scripts/verify-architecture.mjs'], sourceRoot)
const freshModuleBoundary = run('fresh_package_module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'], sourceRoot)
const freshGenerator = run('fresh_package_generator', 'node', ['scripts/generate-b09-h04-acceptance.mjs'], sourceRoot, { B09_H04_EVIDENCE_DIR: receiverEvidence })
const freshNode = run('fresh_package_node', 'node', ['--test', 'tests/b09-h04-acceptance-generator.test.mjs', 'tests/b09-h04-ui.test.mjs'], sourceRoot)
Object.assign(results, {
  fresh_package_architecture: freshArchitecture,
  fresh_package_base_architecture: freshBaseArchitecture,
  fresh_package_module_boundary: freshModuleBoundary,
  fresh_package_generator: freshGenerator,
  fresh_package_node: freshNode,
})
const freshPackageArchitecture = archive.status === 0
  && extract.status === 0
  && freshArchitecture.status === 'PASS'
  && freshBaseArchitecture.status === 'PASS'
  && freshModuleBoundary.status === 'PASS'
  && freshGenerator.status === 'PASS'
  && freshNode.status === 'PASS'

const marker = (result, pattern) => result.status === 'PASS' && pattern.test(result.output)
const gate = (status, source, actual = {}) => ({ status: status ? 'PASS' : 'FAIL', source, blocking: true, ...actual })
const allPass = (...items) => items.every(item => item.status === 'PASS')
const readJson = file => {
  try { return JSON.parse(fs.readFileSync(path.join(evidence, file), 'utf8')) } catch { return null }
}
const scaleReport = readJson('b09_h04_scale.json')
const scaleCountsPass = scaleReport?.fixture === 'LONG_BOOK_10M'
  && Number(scaleReport.materializedFinalBodyChars) >= 10_000_000
  && Number(scaleReport.counts?.books) >= 1
  && Number(scaleReport.counts?.volumes) >= 40
  && Number(scaleReport.counts?.chapters) >= 4_000
  && Number(scaleReport.counts?.artifacts) >= 500_000
  && Number(scaleReport.counts?.canonFacts) >= 1_000_000
  && Number(scaleReport.counts?.dependencies) >= 3_000_000
  && Number(scaleReport.counts?.modelRuns) >= 100_000
  && scaleReport.fullProjectScan === false
  && scaleReport.webviewFullDump === false
  && scaleReport.status === 'PASS'

const gates = {
  'B09-A01': gate(architecture.status === 'PASS' && before.commit !== START_COMMIT && startCommitIsAncestor, 'architecture.log+git'),
  'B09-A02': gate(architecture.status === 'PASS' && !/CommitBundleV3|NarrativeRuntimeDelta/.test(architecture.output), 'architecture.log'),
  'B09-A03': gate(marker(b09, /B09_PLANNING_STATE_RESULT .*status=PASS/), b09.log),
  'B09-A04': gate(marker(b09, /H04_PROJECTION_REPAIR_RESULT .*projection_rebuilt=PASS/), b09.log),
  'B09-A05': gate(marker(b09, /B09_ROUTE_REPLAN_RESULT .*owner=B09::VolumeLifecycleOwner .*reuses=B07::PlanningOwner/), b09.log),
  'B09-A06': gate(marker(b09, /B09_ROUTE_REPLAN_RESULT .*reason=PASS .*evidence=PASS/), b09.log),
  'B09-A07': gate(marker(b09, /B09_ROUTE_REPLAN_RESULT .*evidence=PASS .*receipt=PASS/), b09.log),
  'B09-A08': gate(marker(b09, /B09_ROUTE_REPLAN_STALE_RESULT .*stale=BLOCK .*current_head_unchanged=PASS/), b09.log),
  'B09-A09': gate(marker(b09, /B09_ROUTE_REPLAN_RACE_RESULT .*dual_connection=PASS .*successCount=1/), b09.log),
  'B09-A10': gate(architecture.status === 'PASS' && productionRootConnected(), 'architecture.log'),
  'B09-A11': gate(marker(b09, /B09_VOLUME_CLOSE_NEGATIVE_RESULT .*required_exit=BLOCK .*partial_close=0/), b09.log),
  'B09-A12': gate(marker(b09, /B09_VOLUME_CLOSE_EVIDENCE_NEGATIVE_RESULT .*missing_evidence=BLOCK .*partial_close=0/), b09.log),
  'B09-A13': gate(marker(b09, /B09_CLOSE_RESULT .*exit_snapshot=PASS/), b09.log),
  'B09-A14': gate(marker(b09, /B09_CLOSE_RESULT .*eligibility=PASS/), b09.log),
  'B09-A15': gate(marker(b09, /B09_CLOSE_RESULT .*handoff=PASS/), b09.log),
  'B09-A16': gate(architecture.status === 'PASS' && productionRootConnected(), 'architecture.log'),
  'B09-A17': gate(marker(b09, /B09_MULTI_VOLUME_RESULT .*open_thread_preserved=PASS/), b09.log),
  'B09-A18': gate(marker(b09, /B09_MULTI_VOLUME_RESULT .*reservation_boundary=PASS/), b09.log),
  'B09-A19': gate(marker(b09, /B09_MULTI_VOLUME_RESULT .*exact_evidence_chain=PASS/), b09.log),
  'B09-A20': gate(architecture.status === 'PASS' && /noSecondCommitProtocol/.test(architecture.output), 'architecture.log'),
  'B09-A21': gate(architecture.status === 'PASS' && /uiTruthful/.test(architecture.output), 'architecture.log'),
  'B09-A22': gate(scale.status === 'PASS' && marker(scale, /B09_H04_SCALE_RESULT .*boundedRead=PASS .*contextBounded=PASS .*pagination=PASS .*status=PASS/) && scaleCountsPass, 'b09_h04_scale.log', { scale: scaleReport }),
  'H04-A01': gate(architecture.status === 'PASS' && /"noFourthHarness": "PASS"/.test(architecture.output), 'architecture.log'),
  'H04-A02': gate(scale.status === 'PASS' && marker(scale, /B09_H04_SCALE_RESULT .*boundedRead=PASS .*contextBounded=PASS .*status=PASS/) && scaleCountsPass, 'b09_h04_scale.log', { scale: scaleReport }),
  'H04-A03': gate(marker(b09, /H04_PROVENANCE_RESULT .*claim=PASS .*sourceEvidence=PASS .*capabilitySpec=PASS .*resultDigest=PASS .*cross_volume=PASS .*status=PASS/), b09.log),
  'H04-A04': gate(marker(b09, /H04_H02_NO_MUTATION_RESULT .*plan=PASS .*route=PASS .*taskReality=PASS .*thread=PASS .*status=PASS/), b09.log),
  'H04-A05': gate(marker(b09, /H04_REPLAN_AUTH_RESULT .*typed_owner=PASS .*explicit_authorization=PASS .*status=PASS/), b09.log),
  'H04-A06': gate(marker(b09, /H04_PROJECTION_REPAIR_RESULT .*authority_unchanged=PASS .*reality_unchanged=PASS .*projection_rebuilt=PASS .*status=PASS/), b09.log),
  'H04-A07': gate(b08.status === 'PASS' && /proposal_only|change_request/.test(b08.output), b08.log),
  'H04-A08': gate(marker(b09, /H04_CHECKPOINT_RESULT .*revision_same=PASS .*revalidate=PASS .*status=PASS/), b09.log),
  'H04-A09': gate(marker(b09, /H04_CHECKPOINT_RESULT .*revision_same=PASS .*capability_digest_change=STALE .*revalidate=PASS .*status=PASS/), b09.log),
  'H04-A10': gate(marker(b08, /H03_AUTH_RACE_RESULT .*workers=2 .*successful=1 .*rejected=1 .*activeOwners=1 .*status=PASS/), b08.log),
  'H04-A11': gate(scale.status === 'PASS' && marker(scale, /B09_H04_SCALE_RESULT .*fixture=LONG_BOOK_10M .*boundedRead=PASS .*status=PASS/) && scaleCountsPass, 'b09_h04_scale.log', { scale: scaleReport }),
  'H04-A12': gate(marker(b06, /H01_RESULT .*bounded_replan=PASS .*max_steps=7/) && moduleBoundary.status === 'PASS', 'b06_h01_regression.log+module_boundary.log'),
  baseline: gate(architecture.status === 'PASS', 'architecture.log'),
  no_second_engine: gate(architecture.status === 'PASS', 'architecture.log'),
  migration_freeze: gate(architecture.status === 'PASS', 'architecture.log'),
  architecture: gate(architecture.status === 'PASS' && baseArchitecture.status === 'PASS', 'architecture.log+base_architecture.log'),
  module_boundary: gate(moduleBoundary.status === 'PASS', moduleBoundary.log),
  contracts: gate(contracts.status === 'PASS', contracts.log),
  node: gate(node.status === 'PASS', node.log),
  vitest: gate(vitest.status === 'PASS', vitest.log),
  typecheck: gate(typecheck.status === 'PASS', typecheck.log),
  build: gate(build.status === 'PASS', build.log),
  cargo_all_targets: gate(cargoAll.status === 'PASS', cargoAll.log),
  cargo_fmt: gate(fmt.status === 'PASS', fmt.log),
  cargo_check: gate(check.status === 'PASS', check.log),
  cargo_clippy: gate(clippy.status === 'PASS', clippy.log),
  b02_regression: gate(b02.status === 'PASS', b02.log),
  b03_regression: gate(b03.status === 'PASS', b03.log),
  b05_regression: gate(b05.status === 'PASS', b05.log),
  b06_h01_regression: gate(b06.status === 'PASS', b06.log),
  b07_h02_regression: gate(b07.status === 'PASS', b07.log),
  b08_h03_regression: gate(b08.status === 'PASS', b08.log),
  p01_regression: gate(p01.status === 'PASS', p01.log),
  package_smoke: gate(packageSmoke, 'package_smoke.log', { forbiddenEntries: forbiddenEntries.length }),
  fresh_package_architecture: gate(freshPackageArchitecture, 'fresh_package_architecture.log+fresh_package_generator.log', { receiver: { architecture: freshArchitecture.status, generator: freshGenerator.status, node: freshNode.status } }),
}

function productionRootConnected() {
  return architecture.status === 'PASS' && /productionRootConnected/.test(architecture.output)
}

gates.source_binding = gate(sourceBinding, 'git', {
  startCommit: START_COMMIT,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: after.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
})

const report = {
  schema: 'b09-h04-gate-results-r1',
  batch: 'B09_H04',
  startCommit: START_COMMIT,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: after.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  sourceBinding: sourceBinding ? 'PASS' : 'FAIL',
  gates,
  results: Object.fromEntries(Object.entries(results).map(([name, result]) => [name, { status: result.status, exitCode: result.exitCode, log: result.log }])),
  generatedAt: new Date().toISOString(),
}
fs.writeFileSync(path.join(evidence, 'gate-results.json'), `${JSON.stringify(report, null, 2)}\n`)

const structured = (name, gateNames, extra = {}) => {
  const selected = gateNames.map(gateName => ({ name: gateName, ...(gates[gateName] ?? { status: 'FAIL', source: 'missing' }) }))
  const status = selected.every(gate => gate.status === 'PASS') ? 'PASS' : 'FAIL'
  fs.writeFileSync(path.join(evidence, name), `${JSON.stringify({ batch: 'B09_H04', status, startCommit: START_COMMIT, startCommitIsAncestor, sourceCommit: before.commit, evidenceSourceCommit: before.commit, sourceCommitAfter: after.commit, workingTreeDirtyBefore: report.workingTreeDirtyBefore, workingTreeDirtyAfter: report.workingTreeDirtyAfter, excludedUncommittedPaths: [], ...extra, gates: selected }, null, 2)}\n`)
}

structured('B09_PLANNING_STATE.json', ['B09-A03', 'B09-A04'])
structured('B09_PROJECTION_REBUILD.json', ['B09-A04'])
structured('B09_ROUTE_REPLAN.json', ['B09-A05', 'B09-A06', 'B09-A07'])
structured('B09_ROUTE_REPLAN_STALE.json', ['B09-A08'])
structured('B09_ROUTE_REPLAN_RACE.json', ['B09-A09'])
structured('B09_VOLUME_CLOSE.json', ['B09-A11', 'B09-A13', 'B09-A14'])
structured('B09_VOLUME_CLOSE_ATOMIC.json', ['B09-A12', 'B09-A13'])
structured('B09_NEXT_VOLUME_ELIGIBILITY.json', ['B09-A14'])
structured('B09_NEXT_VOLUME_HANDOFF.json', ['B09-A15'])
structured('B09_MULTI_VOLUME_CERTIFICATION.json', ['B09-A17', 'B09-A18', 'B09-A19'], { profile: '3_VOLUME_DETERMINISTIC_FIXTURE' })
structured('B09_EXACT_EVIDENCE_CHAIN.json', ['B09-A19'])
structured('B09_RESERVATION_CROSS_VOLUME.json', ['B09-A18'])
structured('B09_UI_TRUTHFULNESS.json', ['B09-A21'])
structured('H04_CERTIFICATION.json', H04_BLOCKING_GATES)
structured('H04_BOUNDED_READ.json', ['H04-A02', 'H04-A11', 'H04-A12'])
structured('H04_PROVENANCE.json', ['H04-A03'])
structured('H04_H02_NO_MUTATION.json', ['H04-A04'])
structured('H04_REPLAN_CAPABILITY.json', ['H04-A05'])
structured('H04_PROJECTION_REPAIR.json', ['H04-A06'])
structured('H04_CHECKPOINT_REVALIDATION.json', ['H04-A08'])
structured('H04_CAPABILITY_REVALIDATION.json', ['H04-A09'])
structured('H04_CANON_CONFLICT_PROPOSAL_ONLY.json', ['H04-A07'])
structured('H04_REPAIR_AUTHORIZATION.json', ['H04-A10'])
structured('H04_NO_UNBOUNDED_LOOP.json', ['H04-A12'])
structured('B09_H04_10M_PERFORMANCE.json', ['B09-A22', 'H04-A02', 'H04-A11'], { profile: 'LONG_BOOK_10M', scale: scaleReport, fullProjectScan: scaleReport?.fullProjectScan ?? null, webviewFullDump: scaleReport?.webviewFullDump ?? null })
structured('B09_H04_GLOBAL_GATES.json', ['baseline', 'no_second_engine', 'migration_freeze', 'architecture', 'module_boundary', 'contracts', 'node', 'vitest', 'typecheck', 'build', 'cargo_all_targets', 'cargo_fmt', 'cargo_check', 'cargo_clippy', 'source_binding', 'package_smoke', 'fresh_package_architecture'])
structured('B09_H04_REGRESSION.json', ['b02_regression', 'b03_regression', 'b05_regression', 'b06_h01_regression', 'b07_h02_regression', 'b08_h03_regression', 'p01_regression'])
fs.writeFileSync(path.join(evidence, 'B09_H04_SOURCE_BINDING.json'), `${JSON.stringify({
  batch: 'B09_H04',
  startCommit: START_COMMIT,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: report.workingTreeDirtyBefore,
  workingTreeDirtyAfter: report.workingTreeDirtyAfter,
  excludedUncommittedPaths: [],
  status: report.sourceBinding,
}, null, 2)}\n`)
const baselineCommit = START_COMMIT
const changedFiles = git(['diff', '--name-status', `${baselineCommit}..${before.commit}`]).stdout.trim().split('\n').filter(Boolean).map(line => {
  const [status, ...file] = line.split(/\s+/)
  return { status, file: file.join(' ') }
})
fs.writeFileSync(path.join(evidence, 'B09_H04_CHANGED_FILES.json'), `${JSON.stringify({
  batch: 'B09_H04', baselineCommit, sourceCommit: before.commit, changedFiles, status: before.dirtyPaths.length === 0 ? 'PASS' : 'FAIL',
}, null, 2)}\n`)
const frozenMigrationPaths = Array.from({ length: 9 }, (_, index) => `src-tauri/migrations/${String(index + 1).padStart(4, '0')}_${['catalog','project_authority','project_command','project_longterm','m0c_commit_delta','m0e_ops','b05_narrative_foundation','h01_support_checkpoint','h03_repair_workflow'][index]}.sql`)
const frozenMigrationsUnchanged = frozenMigrationPaths.every(file => git(['diff', '--quiet', `${baselineCommit}..${before.commit}`, '--', file]).status === 0)
fs.writeFileSync(path.join(evidence, 'B09_H04_MIGRATION_FREEZE.json'), `${JSON.stringify({
  batch: 'B09_H04', baselineCommit, frozenMigrations: frozenMigrationPaths, unchanged: frozenMigrationsUnchanged, additiveMigration: 'src-tauri/migrations/0010_b09_h04_volume_lifecycle.sql', status: frozenMigrationsUnchanged ? 'PASS' : 'FAIL',
}, null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'B09_H04_PACKAGE_SMOKE.json'), `${JSON.stringify({
  batch: 'B09_H04', sourceCommit: before.commit, sourceOnlyStage: true, rootPrefix: 'ai-novel-production-system/', forbiddenEntries, archiveValidated: packageSmoke, status: packageSmoke ? 'PASS' : 'FAIL',
}, null, 2)}\n`)
const acceptance = generateAcceptance(evidence)

const failed = Object.entries(gates).filter(([, gate]) => gate.status !== 'PASS').map(([name]) => name)
console.log(`B09_H04 gates recorded: ${Object.values(gates).filter(gate => gate.status === 'PASS').length}/${Object.keys(gates).length} PASS; acceptance=${acceptance.status}; sourceBinding=${report.sourceBinding}; failed=${failed.join(',') || 'none'}; evidence=${evidence}`)
if (failed.length > 0) process.exitCode = 1
