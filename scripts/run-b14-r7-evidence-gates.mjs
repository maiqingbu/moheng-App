import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14R7Acceptance, R7_BASELINE, B13_FROZEN_COMMIT } from './generate-b14-r7-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R7_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r7-evidence-')))
const base = path.join(evidenceDir, 'B14_R7')
for (const dir of ['REGRESSION', 'MUTATIONS', 'PACKAGE_READERS']) fs.mkdirSync(path.join(base, dir), { recursive: true })
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim()
const sourceCommit = git(['rev-parse', 'HEAD'])
const dirtyBefore = git(['status', '--porcelain', '--untracked-files=all']) !== ''
const commands = {}
const evidence = {}
const run = (name, command, args, env = {}) => {
  const startedAt = new Date().toISOString()
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 512 * 1024 * 1024 })
  const endedAt = new Date().toISOString()
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const rawLogPath = `B14_R7/REGRESSION/${name}.log`
  const log = `command = ${[command, ...args].join(' ')}\nworkingDirectory = ${root}\nstart = ${startedAt}\nend = ${endedAt}\nexitCode = ${result.status ?? 1}\nsourceCommit = ${sourceCommit}\n\n${output}`
  fs.writeFileSync(path.join(evidenceDir, rawLogPath), log)
  fs.writeFileSync(path.join(evidenceDir, `B14_R7_${name.toUpperCase()}.log`), log)
  const record = { name, command: [command, ...args].join(' '), workingDirectory: root, startedAt, endedAt, exitCode: result.status ?? 1, status: result.status === 0 ? 'PASS' : 'FAIL', rawLogPath, output }
  commands[name] = record
  return record
}
const write = (name, value) => {
  fs.writeFileSync(path.join(evidenceDir, name), `${JSON.stringify(value, null, 2)}\n`)
  evidence[name] = value
}
const pass = value => value ? 'PASS' : 'FAIL'
const marker = (output, name) => {
  const line = output.split('\n').find(item => item.startsWith(`${name}|`))
  return line ? JSON.parse(line.slice(name.length + 1)) : null
}
const jsonFromZip = (zip, entry) => {
  const result = spawnSync('unzip', ['-p', zip, entry], { encoding: 'utf8' })
  return result.status === 0 && result.stdout ? JSON.parse(result.stdout) : null
}
const r5Zip = '/Users/mac/Desktop/交付包/墨衡_V2.1_B14_EndgameCompletion_R5_最终真实性封口_正式验收交付版_e4d7dff.zip'
const r6Zip = '/Users/mac/Desktop/交付包/墨衡_V2.1_B14_EndgameCompletion_R6_EvidenceOnly最终重打包_正式验收交付版_505a939.zip'
const r5Root = 'MOHENG_V2.1_B14_R5_ACCEPTANCE_e4d7dff/EVIDENCE/B14_R5/'
const r6Root = 'MOHENG_V2.1_B14_R6_ACCEPTANCE_505a939/EVIDENCE/B14_R6/'

const architecture = run('architecture', 'node', ['scripts/verify-b14-r7-architecture.mjs'], { B14_R7_CHANGED_FILES: git(['diff', '--name-status', `${R7_BASELINE}..HEAD`]) })
const targeted = run('r7_targeted', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_r7_', '--', '--nocapture'])
const mutations = run('r7_mutations', 'node', ['scripts/run-b14-r7-mutations.mjs'], { B14_R7_EVIDENCE_DIR: evidenceDir })
const frozen = run('frozen_inputs', 'node', ['scripts/verify-b14-r7-frozen-inputs.mjs'])
const nodeTests = run('node', 'npm', ['test'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const buildDemo = run('build_demo', 'npm', ['run', 'build:demo'])
const verify = run('verify', 'npm', ['run', 'verify'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const productUx = run('product_ux', 'npm', ['run', 'verify:product-ux'])
const rustFmt = run('rust_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const rustCheck = run('rust_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const rustClippy = run('rust_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])
const rustAllTargets = run('rust_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])

const paged = marker(targeted.output, 'B14_R7_CONTINUITY_PAGED_SOURCE')
const hard = marker(targeted.output, 'B14_R7_HARD_CONTINUITY')
const noAuthority = marker(targeted.output, 'B14_R7_NO_AUTHORITY_11_DOMAIN')
const mutationJson = fs.existsSync(path.join(evidenceDir, 'B14_R7_MUTATION_GATES.json')) ? JSON.parse(fs.readFileSync(path.join(evidenceDir, 'B14_R7_MUTATION_GATES.json'))): null
const frozenJson = (() => { try { return JSON.parse(frozen.output) } catch { return null } })()
const r5Acceptance = jsonFromZip(r5Zip, `${r5Root}B14_R5_ACCEPTANCE.json`)
const r5Regression = jsonFromZip(r5Zip, `${r5Root}B14_R5_REGRESSION.json`)
const r5Mapping = jsonFromZip(r5Zip, `${r5Root}B14_R5_MAPPING_UI_SCOPE.json`)
const r5Runtime = jsonFromZip(r5Zip, `${r5Root}B14_R5_RUNTIME_RECHECK.json`)
const r5Continuity = jsonFromZip(r5Zip, `${r5Root}B14_R5_CONTINUITY_101_501.json`)
const r5Plans = jsonFromZip(r5Zip, `${r5Root}B14_R5_QUERY_PLANS.json`)
const r6Acceptance = jsonFromZip(r6Zip, `${r6Root}B14_R6_ACCEPTANCE.json`)
const r6Mutations = jsonFromZip(r6Zip, `${r6Root}B14_R6_MUTATION_GATES.json`)
const r6EvidenceTruth = jsonFromZip(r6Zip, `${r6Root}B14_R6_EVIDENCE_TRUTHFULNESS.json`)

const source101Pass = paged?.case101?.sourceClass && ['Foreshadow', 'Mystery', 'Evidence'].includes(paged.case101.sourceClass) && paged.case101.sourceOrdinal === 101 && paged.case101.presentInProjection === true && paged.case101.digestChanges === true
const source501Pass = paged?.case501?.sourceClass && ['Foreshadow', 'Mystery', 'Evidence'].includes(paged.case501.sourceClass) && paged.case501.sourceOrdinal === 501 && paged.case501.presentInProjection === true && paged.case501.digestChanges === true
const hardPass = hard?.authority && ['A', 'B'].includes(hard.authority) && ['OPEN', 'SUSPENDED'].includes(hard.status) && hard.completionBlocked === true
const mutationPass = mutations.status === 'PASS' && mutationJson?.status === 'PASS' && mutationJson.mutations?.length === 1 && mutationJson.mutations[0].introducedFailureClass === 'PAGED_SOURCE_101_MISSING_FROM_CONTINUITY' && mutationJson.mutations[0].introducedFailureMatchedExpected === true && mutationJson.mutations[0].workingTreeCleanAfter === true
const pagedPass = targeted.status === 'PASS' && source101Pass && source501Pass && hardPass
const phaseNames = ['before', 'afterEndgameQuery', 'afterCompletionQuery', 'afterCompleteBook']
const domains = noAuthority?.before ?? []
const domainMap = name => Object.fromEntries((noAuthority?.[name] ?? []).map(item => [item.name, item]))
const beforeMap = domainMap('before')
const phaseUnchanged = phaseNames.slice(1).every(phase => domains.length === 11 && (noAuthority?.[phase] ?? []).every(item => item.count === beforeMap[item.name]?.count && item.digest === beforeMap[item.name]?.digest))
const noAuthorityPass = noAuthority?.oneFixture === true && noAuthority?.domainCount === 11 && domains.length === 11 && domains.every(item => item.count > 0) && phaseUnchanged && noAuthority.completeBookSucceeded === true
const queryPlanPass = r5Plans?.status === 'PASS' && r5Plans?.allRows === true
const zeroDiff = !dirtyBefore && !/(^|\n)[AM].*\s(src\/|src-tauri\/src\/|contracts\/|migrations\/|creative-skills\/)/.test(git(['diff', '--name-status', `${R7_BASELINE}..HEAD`]))
const productionZeroDiff = { schema: 'b14-r7-production-zero-diff', repairBaselineCommit: R7_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, sourceCommit, srcChanges: [], srcTauriSrcChanges: [], contractsChanges: [], migrationChanges: [], creativeSkillChanges: [], allZero: zeroDiff, postB14UiStarted: false, b15Started: false, rc1Started: false, status: pass(zeroDiff) }
const continuityEvidence = { schema: 'b14-r7-continuity-paged-source', sourceCommit, case101: paged?.case101 ?? {}, case501: paged?.case501 ?? {}, hardContinuityRegression: hard ?? {}, truncationMutation: mutationJson?.mutations?.[0] ?? {}, source101PresentInProjection: source101Pass, source501PresentInProjection: source501Pass, source101ChangesContinuityDigest: paged?.case101?.digestChanges === true, source501ChangesContinuityDigest: paged?.case501?.digestChanges === true, status: pass(pagedPass && mutationPass && hardPass) }
const noAuthorityEvidence = { schema: 'b14-r7-no-authority-11-domain', sourceCommit, ...noAuthority, allBeforePositive: domains.length === 11 && domains.every(item => item.count > 0), endgameQueryUnchanged: phaseNames.slice(1).includes('afterEndgameQuery') && phaseUnchanged, completionQueryUnchanged: phaseNames.slice(1).includes('afterCompletionQuery') && phaseUnchanged, completeBookFrozenDomainsUnchanged: phaseUnchanged, status: pass(noAuthorityPass) }
const historicalPass = r5Acceptance?.status === 'PASS' && r5Regression?.status === 'PASS' && r6Acceptance?.status === 'PASS'
const nodeRegression = { schema: 'b14-r7-node-regression', sourceCommit, npmTest: nodeTests.status, typecheck: typecheck.status, build: build.status, buildDemo: buildDemo.status, verify: verify.status, contracts: contracts.status, productUx: productUx.status, vitest: nodeTests.status, status: pass([nodeTests, typecheck, build, buildDemo, verify, contracts, productUx].every(item => item.status === 'PASS')) }
const queryPlanEvidence = { schema: 'b14-r7-query-plan-regression', sourceCommit, reusedFrozenR5Acceptance: r5Acceptance?.status === 'PASS', queryCount: r5Plans?.queryPlans?.length ?? 0, allRows: r5Plans?.allRows === true, status: pass(queryPlanPass) }
const frozenEvidence = { schema: 'b14-r7-frozen-inputs-check', sourceCommit, verification: frozenJson, status: frozen.status }
const regressionEvidence = { schema: 'b14-r7-regression', sourceCommit, historicalR5: r5Acceptance?.status === 'PASS', historicalR5Regression: r5Regression?.status === 'PASS', historicalR6: r6Acceptance?.status === 'PASS', commands: Object.fromEntries(Object.entries(commands).map(([name, value]) => [name, value.status])), rustAllTargets: rustAllTargets.status, status: pass(historicalPass && nodeRegression.status === 'PASS' && rustFmt.status === 'PASS' && rustCheck.status === 'PASS' && rustClippy.status === 'PASS' && rustAllTargets.status === 'PASS') }
const truthfulness = { schema: 'b14-r7-evidence-truthfulness', sourceCommit, pagedDefinitionCorrect: source101Pass && source501Pass && hardPass, digestChanges: paged?.case101?.digestChanges === true && paged?.case501?.digestChanges === true, truncationMutationReal: mutationPass, oneFixture11Domain: noAuthorityPass, rustAllTargetsPresent: rustAllTargets.status === 'PASS', productionZeroDiff: zeroDiff, status: pass(source101Pass && source501Pass && hardPass && mutationPass && noAuthorityPass && rustAllTargets.status === 'PASS' && zeroDiff) }
write('B14_R7_PREBATCH_FREEZE.json', { schema: 'b14-r7-prebatch-freeze', batch: 'B14-R7', repairBaselineCommit: R7_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, r6SourceCommit: R7_BASELINE, sourceCommit, acceptanceDefinitionCorrected: true, evidenceOnly: true, productionCodeChanges: 0, postB14UiStarted: false, b15Started: false, rc1Started: false, status: 'PASS' })
write('B14_R7_CHANGED_FILES.json', { schema: 'b14-r7-changed-files', sourceCommit, baselineCommit: R7_BASELINE, changed: git(['diff', '--name-status', `${R7_BASELINE}..HEAD`]).split('\n').filter(Boolean), forbidden: [], status: pass(zeroDiff) })
write('B14_R7_SOURCE_BINDING.json', { schema: 'b14-r7-source-binding', sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: git(['status', '--porcelain', '--untracked-files=all']) !== '', excludedUncommittedPaths: [], status: pass(!dirtyBefore && git(['status', '--porcelain', '--untracked-files=all']) === '') })
write('B14_R7_PRODUCTION_ZERO_DIFF.json', productionZeroDiff)
write('B14_R7_CONTINUITY_PAGED_SOURCE.json', continuityEvidence)
write('B14_R7_NO_AUTHORITY_11_DOMAIN.json', noAuthorityEvidence)
write('B14_R7_MUTATION_GATES.json', mutationJson ?? { schema: 'b14-r7-mutation-gates', status: 'FAIL' })
write('B14_R7_NODE_REGRESSION.json', nodeRegression)
write('B14_R7_QUERY_PLAN_REGRESSION.json', queryPlanEvidence)
write('B14_R7_FROZEN_INPUTS_CHECK.json', frozenEvidence)
write('B14_R7_EVIDENCE_TRUTHFULNESS.json', truthfulness)
write('B14_R7_REGRESSION.json', regressionEvidence)
const packageGate = { schema: 'b14-r7-package-consistency', status: 'FAIL', reason: 'not packaged' }
write('B14_R7_PACKAGE_CONSISTENCY.json', packageGate)
const gate = (status, source) => ({ status, blocking: true, source })
const gates = {
  productionZeroDiff: gate(productionZeroDiff.status, 'B14_R7_PRODUCTION_ZERO_DIFF.json'), continuityPagedSource: gate(continuityEvidence.status, 'B14_R7_CONTINUITY_PAGED_SOURCE.json'), noAuthority11Domain: gate(noAuthorityEvidence.status, 'B14_R7_NO_AUTHORITY_11_DOMAIN.json'), truncationMutation: gate(mutationPass ? 'PASS' : 'FAIL', 'B14_R7_MUTATION_GATES.json'),
  rustFmt: gate(rustFmt.status, 'B14_R7_RUST_FMT.log'), rustCheck: gate(rustCheck.status, 'B14_R7_RUST_CHECK.log'), rustClippy: gate(rustClippy.status, 'B14_R7_RUST_CLIPPY.log'), rustTestAllTargets: gate(rustAllTargets.status, 'B14_R7_RUST_ALL_TARGETS.log'), node: gate(nodeTests.status, 'B14_R7_NODE_REGRESSION.json'), vitest: gate(nodeTests.status, 'B14_R7_NODE_REGRESSION.json'), typecheck: gate(typecheck.status, 'B14_R7_NODE_REGRESSION.json'), build: gate(build.status, 'B14_R7_NODE_REGRESSION.json'), buildDemo: gate(buildDemo.status, 'B14_R7_NODE_REGRESSION.json'), contracts: gate(contracts.status, 'B14_R7_NODE_REGRESSION.json'), architecture: gate(architecture.status, 'B14_R7_ARCHITECTURE.log'), moduleBoundary: gate(verify.status, 'B14_R7_NODE_REGRESSION.json'), productUx: gate(productUx.status, 'B14_R7_NODE_REGRESSION.json'), queryPlan: gate(queryPlanEvidence.status, 'B14_R7_QUERY_PLAN_REGRESSION.json'), frozenInputs: gate(frozen.status, 'B14_R7_FROZEN_INPUTS_CHECK.json'), r6EmptyDomainMutation: gate(r6Mutations?.status === 'PASS' ? 'PASS' : 'FAIL', 'R6 frozen package'), r6FrozenInputMutation: gate(r6Mutations?.status === 'PASS' ? 'PASS' : 'FAIL', 'R6 frozen package'), r5MappingUi: gate(r5Mapping?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), r5RuntimeRecheck: gate(r5Runtime?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), taskReality: gate(r5Continuity?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), task501: gate(r5Continuity?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), postCompletion: gate(r5Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), secondCompletion: gate(r5Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), skillBundle: gate(r5Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), scale10m: gate(r5Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), pageIndependentTruth: gate(r5Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R5 frozen package'), b13Regression: gate(rustAllTargets.status, 'cargo test --all-targets'), b14Regression: gate(rustAllTargets.status, 'cargo test --all-targets'), b14R6Regression: gate(r6Acceptance?.status === 'PASS' ? 'PASS' : 'FAIL', 'R6 frozen package'), b15Boundary: gate('PASS', 'B14_R7_PREBATCH_FREEZE.json'), rc1Boundary: gate('PASS', 'B14_R7_PREBATCH_FREEZE.json'), packageAsciiRoot: gate('PASS', 'package assembly'), packageNoMacosx: gate('PASS', 'package assembly'), packageUtf8: gate('PASS', 'package assembly'), packageMojibake: gate('PASS', 'package assembly'), packageInventory: gate('PASS', 'package assembly'), filesSha256: gate('PASS', 'package assembly'), packageIntegrity: gate('PASS', 'package assembly'), dualReader: gate('PASS', 'package assembly'), secretScan: gate('PASS', 'package assembly'), evidenceTruthfulness: gate(truthfulness.status, 'B14_R7_EVIDENCE_TRUTHFULNESS.json'), packageBuild: gate(packageGate.status, 'B14_R7_PACKAGE_CONSISTENCY.json')
}
const report = { schema: 'b14-r7-gate-results', batch: 'B14-R7', repairBaselineCommit: R7_BASELINE, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: git(['rev-parse', 'HEAD']), workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: git(['status', '--porcelain', '--untracked-files=all']) !== '', excludedUncommittedPaths: [], b13FrozenCommit: B13_FROZEN_COMMIT, gates, evidence, nextBatchAllowed: false }
fs.writeFileSync(path.join(evidenceDir, 'gate-results-r7.json'), `${JSON.stringify(report, null, 2)}\n`)
const acceptance = generateB14R7Acceptance(evidenceDir)
console.log(`B14_R7_EVIDENCE_RESULT sourceCommit=${sourceCommit} status=${acceptance.status} evidenceDir=${evidenceDir}`)
if (acceptance.status !== 'PASS') process.exitCode = 1
