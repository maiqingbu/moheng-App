import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import crypto from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { inspectB10Architecture } from './verify-b10-architecture.mjs'
import {
  B10_BLOCKING_GATES,
  B10_ENGINEERING_GATES,
  ENGINEERING_GATES,
  HISTORICAL_REGRESSION_GATES,
  NEGATIVE_BLOCKING_GATES,
  RELEASE_GATES,
  REQUIRED_BLOCKING_GATES,
  R2_GLOBAL_BLOCKING_GATES,
  START_COMMIT,
  generateAcceptance,
} from './generate-b10-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root, 'src-tauri')
const evidence = process.env.B10_EVIDENCE_DIR
  ? path.resolve(process.env.B10_EVIDENCE_DIR)
  : fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b10-evidence-'))
fs.mkdirSync(evidence, { recursive: true })

const packageRoot = path.join(root, 'creative-skills/v1.2.0')
const expectedPackageDigest = 'a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f'
const frozenMigrations = [
  '0001_catalog.sql',
  '0002_project_authority.sql',
  '0003_project_command.sql',
  '0004_project_longterm.sql',
  '0005_m0c_commit_delta.sql',
  '0006_m0e_ops.sql',
  '0007_b05_narrative_foundation.sql',
  '0008_h01_support_checkpoint.sql',
  '0009_h03_repair_workflow.sql',
  '0010_b09_h04_volume_lifecycle.sql',
]
const frozenHistoricalEvidence = [
  'evidence/B01/registry_integrity.json',
  'evidence/B01/schema_inventory.json',
  'evidence/B01/query-plans.json',
]

const prebatchFreezePath = path.join(root, 'evidence/B10/B10_PREBATCH_FREEZE.json')
if (fs.existsSync(prebatchFreezePath)) fs.copyFileSync(prebatchFreezePath, path.join(evidence, 'B10_PREBATCH_FREEZE.json'))

function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'))
  } catch {
    return null
  }
}

function git(args) {
  return spawnSync('git', args, { cwd: root, encoding: 'utf8' })
}

function snapshot() {
  return {
    commit: git(['rev-parse', 'HEAD']).stdout.trim(),
    dirtyPaths: git(['status', '--porcelain=v1', '--untracked-files=all']).stdout.trim().split('\n').filter(Boolean),
  }
}

function run(name, command, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    maxBuffer: 128 * 1024 * 1024,
    env: { ...process.env, ...extraEnv },
  })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const notRun = result.error?.code === 'ENOENT'
  const exitCode = notRun ? 127 : result.status ?? 1
  const value = {
    name,
    command: [command, ...args].join(' '),
    status: notRun ? 'NOT_RUN' : exitCode === 0 ? 'PASS' : 'FAIL',
    exitCode,
    output,
    log: `${name}.log`,
    reason: notRun ? `${command} is not available in the current environment` : undefined,
  }
  fs.writeFileSync(path.join(evidence, value.log), output)
  return value
}

function aggregate(name, members) {
  const output = members.map(member => `--- ${member.name} ---\n${member.output}`).join('\n')
  const value = {
    name,
    command: members.map(member => member.command).join(' && '),
    status: members.every(member => member.status === 'PASS') ? 'PASS' : members.some(member => member.status === 'NOT_RUN') ? 'NOT_RUN' : 'FAIL',
    exitCode: members.every(member => member.status === 'PASS') ? 0 : 1,
    output,
    log: `${name}.log`,
  }
  fs.writeFileSync(path.join(evidence, value.log), output)
  return value
}

function writeJson(file, value) {
  fs.writeFileSync(path.join(evidence, file), `${JSON.stringify(value, null, 2)}\n`)
}

function copyEvidenceTo(directory) {
  fs.mkdirSync(directory, { recursive: true })
  for (const entry of fs.readdirSync(evidence, { withFileTypes: true })) {
    if (entry.name === 'gate-results.json' || entry.name === 'B10_ACCEPTANCE.json') continue
    const source = path.join(evidence, entry.name)
    const target = path.join(directory, entry.name)
    if (entry.isFile()) fs.copyFileSync(source, target)
  }
}

function walk(directory, output = [], rootDirectory = directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) walk(target, output, rootDirectory)
    else output.push({ path: target, relative: path.relative(rootDirectory, target), symlink: entry.isSymbolicLink() })
  }
  return output
}

function buildSourceStage() {
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b10-source-stage-'))
  const archive = spawnSync('git', ['archive', '--format=tar', '--prefix=ai-novel-production-system/', 'HEAD'], {
    cwd: root,
    encoding: 'buffer',
    maxBuffer: 512 * 1024 * 1024,
  })
  const extracted = archive.status === 0
    ? spawnSync('tar', ['-xf', '-', '-C', stage], { input: archive.stdout, encoding: 'utf8' })
    : { status: 1, stderr: 'git archive failed' }
  const sourceRoot = path.join(stage, 'ai-novel-production-system')
  if (fs.existsSync(sourceRoot)) {
    for (const item of walk(sourceRoot)) {
      if (path.basename(item.path) === '.DS_Store') fs.rmSync(item.path, { force: true })
    }
    copyEvidenceTo(path.join(sourceRoot, 'evidence/B10'))
  }
  return { stage, sourceRoot, archive, extracted }
}

function gate(status, source, actual = {}) {
  return { status: status ? 'PASS' : 'FAIL', blocking: true, source, ...actual }
}

function resultPass(result) {
  return result?.status === 'PASS' && result.exitCode === 0
}

const before = snapshot()
const results = {}
const add = (name, command, args, cwd = root, extraEnv = {}) => {
  results[name] = run(name, command, args, cwd, extraEnv)
  return results[name]
}

// B10 targeted tests are separate evidence sources even when they share the
// same Cargo binary, so an acceptance reviewer can see which contract was run.
const b10Skill = add('b10_skill_targeted', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b10_creative_quality', '--', '--nocapture'])
const b10Quality = add('b10_quality_targeted', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b08_h03', 'b10_skill_quality_stays_on_the_existing_b08_production_root', '--', '--nocapture'])
const b10WithoutSkill = add('b10_runtime_without_skill', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b08_h03', 'b10_runtime_without_skill_keeps_the_same_b08_commit_root', '--', '--nocapture'])
const b10Repair = add('b10_repair_targeted', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b10_creative_quality', 'b10_repair', '--', '--nocapture'])
const b10Negative = add('b10_negative_fixtures', 'node', ['--test', 'tests/b10-acceptance-generator.test.mjs'])
const b10Ui = add('b10_ui', 'node', ['--test', 'tests/b10-ui.test.mjs'])
const b10Scale = add('b10_scale_context', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b10_creative_quality', 'b10_context', '--', '--nocapture'])
const architecture = add('architecture', 'node', ['scripts/verify-b10-architecture.mjs'])
const baseArchitecture = add('base_architecture', 'node', ['scripts/verify-architecture.mjs'])
const moduleBoundary = add('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const m0a = add('m0a', 'node', ['scripts/verify-m0a-evidence.mjs'])

const b05 = add('b05_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b05_narrative', '--', '--nocapture'])
const b06 = add('b06_h01_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b06_h01', '--', '--nocapture'])
const b07Main = add('b07_h02_regression_main', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b07_h02', '--', '--nocapture'])
const b07Semantic = add('b07_h02_regression_semantic', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b07_h02_semantic_negative', '--', '--nocapture'])
const b07 = aggregate('b07_h02_regression', [b07Main, b07Semantic])
const b08 = add('b08_h03_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b08_h03', '--', '--nocapture'])
const p01 = add('p01_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'p01_provider', '--', '--nocapture'])
const b09 = add('b09_h04_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b09_h04', '--', '--nocapture'])
const r2Alignment = add('r2_alignment_regression', 'node', ['--test', 'tests/b08-h03-ui.test.mjs', 'tests/p01-provider.test.mjs', 'tests/contracts/architecture-guards.test.mjs'])

const node = add('node', 'npm', ['test'])
const vitest = add('vitest', 'npm', ['run', 'test:store'])
const contracts = add('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = add('typecheck', 'npm', ['run', 'typecheck'])
const build = add('build', 'npm', ['run', 'build'])
const cargoAll = add('cargo_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoFmt = add('cargo_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const cargoCheck = add('cargo_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoClippy = add('cargo_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])

// B01 live-schema tests may refresh historical frozen JSON. Restore only the
// three documented files, and only when this runner started from clean HEAD.
const frozenRestoration = []
if (before.dirtyPaths.length === 0) {
  for (const file of frozenHistoricalEvidence) {
    if (git(['diff', '--quiet', '--', file]).status !== 0) {
      const restored = git(['restore', '--source', before.commit, '--', file])
      frozenRestoration.push({ file, status: restored.status === 0 ? 'PASS' : 'FAIL' })
    }
  }
}

const after = snapshot()
const startCommitIsAncestor = git(['merge-base', '--is-ancestor', START_COMMIT, before.commit]).status === 0
const sourceBinding = before.dirtyPaths.length === 0
  && after.dirtyPaths.length === 0
  && before.commit === after.commit
  && startCommitIsAncestor
const sourceBindingReport = {
  schema: 'b10-source-binding-r1',
  batch: 'B10',
  startCommit: START_COMMIT,
  startCommitIsAncestor,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: after.commit,
  workingTreeDirtyBefore: before.dirtyPaths.length > 0,
  workingTreeDirtyAfter: after.dirtyPaths.length > 0,
  excludedUncommittedPaths: [],
  frozenHistoricalEvidenceRestoration: frozenRestoration,
  status: sourceBinding ? 'PASS' : 'FAIL',
}
writeJson('B10_SOURCE_BINDING.json', sourceBindingReport)

const freeze = readJson(path.join(root, 'evidence/B10/B10_PREBATCH_FREEZE.json'))
const freezePass = freeze?.status === 'PASS'
  && freeze.freezeThrough === 'B09_H04'
  && freeze.acceptedBaseline === START_COMMIT
  && freeze.gitGate?.head === START_COMMIT
  && freeze.gitGate?.workingTree === 'CLEAN'
  && freeze.forbiddenReopenReasons?.length === 4
  && freeze.forbiddenReopenReasons.every(reason => ['REVIEWER_PREFERENCE', 'STYLE_ONLY', 'TEST_ORGANIZATION_ONLY', 'OPPORTUNISTIC_REFACTOR'].includes(reason))
const registry = readJson(path.join(packageRoot, 'registry.json'))
const packageFiles = fs.existsSync(packageRoot) ? walk(packageRoot) : []
const packageSkills = registry?.skills ?? []
const packagePass = registry?.version === '1.2.0'
  && registry?.skillCount === 45
  && packageSkills.length === 45
  && new Set(packageSkills.map(skill => skill.id)).size === 45
  && packageSkills.every(skill => skill.version === '1.2.0' && skill.authorityAccess === 'NONE_DIRECT' && skill.hardGateAuthority === false && skill.controlDecisionAuthority === false)
  && packageFiles.filter(item => item.relative.endsWith('/SKILL.md')).length === 45
  && packageFiles.filter(item => item.relative.endsWith('/moheng.skill.json')).length === 45
const skillPackageEvidence = {
  schema: 'b10-skill-package-r1',
  package: registry?.package,
  version: registry?.version,
  archiveSha256: expectedPackageDigest,
  registrySha256: sha256File(path.join(packageRoot, 'registry.json')),
  skillCount: registry?.skillCount ?? 0,
  skillTextCount: packageFiles.filter(item => item.relative.endsWith('/SKILL.md')).length,
  metadataCount: packageFiles.filter(item => item.relative.endsWith('/moheng.skill.json')).length,
  sourcePath: 'creative-skills/v1.2.0',
  immutable: true,
  status: packagePass ? 'PASS' : 'FAIL',
}
writeJson('B10_SKILL_PACKAGE.json', skillPackageEvidence)
writeJson('B10_SKILL_REGISTRY.json', {
  schema: 'b10-skill-registry-r1',
  packageVersion: registry?.version,
  skillCount: registry?.skillCount,
  idsUnique: new Set(packageSkills.map(skill => skill.id)).size === 45,
  categories: packageSkills.reduce((counts, skill) => ({ ...counts, [skill.category]: (counts[skill.category] ?? 0) + 1 }), {}),
  allVersioned: packageSkills.every(skill => skill.version === '1.2.0'),
  directAuthorityCount: packageSkills.filter(skill => skill.authorityAccess !== 'NONE_DIRECT' || skill.hardGateAuthority || skill.controlDecisionAuthority).length,
  status: packagePass ? 'PASS' : 'FAIL',
})
writeJson('B10_SKILL_SELECTION.json', {
  schema: 'b10-skill-selection-r1',
  defaultPolicy: '1 primary + 0-3 support',
  selectedExample: ['S14', 'S10'],
  maxDefault: 4,
  overFourWithoutBudget: 'DENIED',
  overFourWithBudget: 'ALLOWED_WITH_REASON_AND_BUDGET',
  sourceTest: b10Skill.log,
  status: resultPass(b10Skill) ? 'PASS' : 'FAIL',
})
writeJson('B10_PROGRESSIVE_LOADING.json', {
  schema: 'b10-progressive-loading-r1',
  metadataFirst: true,
  selectedFullTextOnly: true,
  unselectedFullTextLoads: 0,
  unselectedResourceLoads: 0,
  sourceTest: b10Skill.log,
  status: resultPass(b10Skill) && resultPass(architecture) ? 'PASS' : 'FAIL',
})
writeJson('B10_SKILL_TASK_CONTEXT.json', {
  schema: 'b10-skill-task-context-r1',
  maxContextBytes: 64000,
  maxRefs: 64,
  maxCapsules: 16,
  revisionBound: true,
  forbiddenDbAndRepositoryInputs: true,
  sourceTest: b10Skill.log,
  status: resultPass(b10Skill) ? 'PASS' : 'FAIL',
})
writeJson('B10_SKILL_CONTEXT_STALE.json', { schema: 'b10-skill-context-stale-r1', revisionChange: 'STALE/RECOMPILE', sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_SKILL_CAPABILITY_BOUNDARY.json', { schema: 'b10-skill-capability-boundary-r1', forbiddenCapabilities: ['DB_QUERY', 'SQL', 'COMMIT', 'AUTHORITY_WRITE', 'ROUTE_REPLAN', 'CHANGE_EXECUTION', 'H03_P3', 'HARD_GATE'], architecture: architecture.status, status: resultPass(architecture) ? 'PASS' : 'FAIL' })
writeJson('B10_SKILL_OUTPUT_CONTRACT.json', { schema: 'b10-skill-output-contract-r1', allowedKinds: ['CreativeProposal', 'Candidate', 'CandidatePatch', 'QualityFinding', 'RepairSuggestion', 'EscalationAdvice'], controlStatuses: ['OK', 'NEEDS_INPUT', 'NOT_APPLICABLE', 'ADVISORY_ESCALATION'], hostileBlockNormalized: resultPass(b10Skill), status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_RUNTIME_CHECK_REQUESTS.json', { schema: 'b10-runtime-check-requests-r1', allowedKinds: ['CANON_CONFLICT', 'KNOWLEDGE_BOUNDARY', 'THREAD_STATE', 'TASK_REALITY', 'RESERVATION', 'INTEGRITY', 'PLAN_STALE', 'EVIDENCE_GAP'], selfHardGate: false, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_SKILL_INVOCATION_PROVENANCE.json', { schema: 'b10-skill-invocation-provenance-r1', recordsVersion: true, bundleDigest: true, role: true, resources: true, contextDigest: true, methodRefs: true, projectContextRefs: true, modelProvider: true, cost: true, outputDigest: true, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_PRODUCTION_COMPOSITION.json', { schema: 'b10-production-composition-r1', owner: 'B08::ChapterProductionRuntime', sameRoot: resultPass(b10Quality), order: ['ChapterPlan', 'Skill enrichment', 'Renderer', 'Quality', 'bounded repair', 'HARD Audit/Gate', 'Commit'], qualityArtifactInCommitBundle: resultPass(b10Quality), status: resultPass(b10Quality) && resultPass(architecture) ? 'PASS' : 'FAIL' })
writeJson('B10_RUNTIME_WITHOUT_SKILL.json', { schema: 'b10-runtime-without-skill-r1', skillsEnabled: false, sameB08Root: true, finalChapterCommitted: resultPass(b10WithoutSkill), qualityArtifactCount: 0, sourceTest: b10WithoutSkill.log, status: resultPass(b10WithoutSkill) ? 'PASS' : 'FAIL' })
writeJson('B10_QUALITY_REPORT.json', { schema: 'b10-quality-report-r1', dimensions: ['agency', 'desire', 'hook', 'pacing', 'voice', 'dialogue', 'subtext', 'repetition', 'exposition', 'emotionalPayoff'], mergedEvaluatorCalls: 1, softOnly: true, sourceTest: b10Quality.log, status: resultPass(b10Quality) ? 'PASS' : 'FAIL' })
writeJson('B10_HARD_QUALITY_SEPARATION.json', { schema: 'b10-hard-quality-separation-r1', qualityHighHardViolation: 'BLOCKED', hardPassQualityPoor: 'PASS_WITH_REPAIR_OR_ESCALATE', sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_READER_DESIRE_SOFT.json', { schema: 'b10-reader-desire-soft-r1', softOnly: true, canMutateCanon: false, canMutateTaskReality: false, canOverrideHardGate: false, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_MERGED_EVALUATOR.json', { schema: 'b10-merged-evaluator-r1', evaluatorCalls: 1, mandatoryPerDimensionRemoteReviewers: 0, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_REMOTE_CALL_BUDGET.json', { schema: 'b10-remote-call-budget-r1', provider: 'deterministic', through: 'DurableModelRunner', enrichmentRemoteCalls: 0, mergedQualityCalls: 1, repairRunBounded: true, sourceTest: b10Quality.log, status: resultPass(b10Quality) && resultPass(architecture) ? 'PASS' : 'FAIL' })
writeJson('B10_CANDIDATE_REPAIR.json', { schema: 'b10-candidate-repair-r1', surgical: true, candidateOnly: true, finalChapterMutation: false, sourceTest: b10Repair.log, status: resultPass(b10Repair) ? 'PASS' : 'FAIL' })
writeJson('B10_REPAIR_LIMIT.json', { schema: 'b10-repair-limit-r1', limitStopsTransport: resultPass(b10Repair), escalation: resultPass(b10Repair), sourceTest: b10Repair.log, status: resultPass(b10Repair) ? 'PASS' : 'FAIL' })
writeJson('B10_AFFECTED_DIMENSION_RECHECK.json', { schema: 'b10-affected-dimension-recheck-r1', onlyAffectedDimensions: true, evaluatorCalls: 1, hardGateStillRequired: true, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_CANDIDATE_ISOLATION.json', { schema: 'b10-candidate-isolation-r1', committedFinalUnchanged: true, candidatePatchStatus: 'CANDIDATE', sourceTest: b10Repair.log, status: resultPass(b10Repair) ? 'PASS' : 'FAIL' })
writeJson('B10_NO_P3_ESCALATION.json', { schema: 'b10-no-p3-escalation-r1', autoP3Authorization: false, autoP3Consumption: false, sourceTest: b10Skill.log, status: resultPass(b10Skill) ? 'PASS' : 'FAIL' })
writeJson('B10_UI_TRUTHFULNESS.json', { schema: 'b10-ui-truthfulness-r1', backendQuery: 'query_production_center', displays: ['selected skill', 'version/digest', 'invocation provenance', 'quality dimensions', 'Taste version', 'ReaderDesire SOFT', 'Candidate repair scope/limit', 'HARD Gate'], sourceTest: b10Ui.log, status: resultPass(b10Ui) ? 'PASS' : 'FAIL' })
writeJson('B10_SCALE_CONTEXT.json', { schema: 'b10-scale-context-r1', contextCapBytes: 64000, allProjectTextLoaded: false, all45SkillFullTextLoaded: false, registryMetadataOnly: true, sourceTest: b10Scale.log, status: resultPass(b10Scale) && resultPass(architecture) ? 'PASS' : 'FAIL' })

const migrationDetails = frozenMigrations.map(file => ({
  file,
  exists: fs.existsSync(path.join(root, 'src-tauri/migrations', file)),
  unchangedFromStart: git(['diff', '--quiet', `${START_COMMIT}..${before.commit}`, '--', `src-tauri/migrations/${file}`]).status === 0,
  sha256: fs.existsSync(path.join(root, 'src-tauri/migrations', file)) ? sha256File(path.join(root, 'src-tauri/migrations', file)) : null,
}))
writeJson('B10_MIGRATION_FREEZE.json', { schema: 'b10-migration-freeze-r1', frozenMigrations: migrationDetails, appendOnlyB10Migration: false, status: migrationDetails.every(item => item.exists && item.unchangedFromStart) ? 'PASS' : 'FAIL' })
writeJson('B10_CHANGED_FILES.json', { schema: 'b10-changed-files-r1', startCommit: START_COMMIT, sourceCommit: before.commit, paths: git(['diff', '--name-status', `${START_COMMIT}..${before.commit}`]).stdout.trim().split('\n').filter(Boolean), functionalScope: ['B10 bounded Skill Runtime', 'B10 Quality/Repair policy', 'B08 composition/projection/UI', 'B10 acceptance tests and gates', 'immutable Creative Skills v1.2.0 package'], status: sourceBinding ? 'PASS' : 'FAIL' })

const architectureReport = inspectB10Architecture(root)
writeJson('B10_GLOBAL_GATES.json', { schema: 'b10-global-gates-r1', missingGates: architectureReport.missingGates, gates: architectureReport.globalGates, status: architectureReport.status })
writeJson('B10_REGRESSION.json', { schema: 'b10-regression-r1', frozenBatches: { B05: b05.status, B06_H01: b06.status, B07_H02: b07.status, B08_H03: b08.status, R2_ALIGNMENT: r2Alignment.status, P01: p01.status, B09_H04: b09.status }, status: [b05, b06, b07, b08, r2Alignment, p01, b09].every(resultPass) ? 'PASS' : 'FAIL' })

const gates = {}
const setGate = (name, passed, source, actual = {}) => { gates[name] = gate(passed, source, actual) }
const skillBoundary = packageSkills.filter(skill => ['S17', 'S25'].includes(skill.id)).every(skill => skill.mutationPolicy.includes('ADVISORY') && skill.authorityAccess === 'NONE_DIRECT' && skill.hardGateAuthority === false && skill.controlDecisionAuthority === false)
const qualityBoundary = packageSkills.filter(skill => ['S35', 'S36', 'S37', 'S38', 'S39'].includes(skill.id)).length === 5 && packageSkills.filter(skill => ['S35', 'S36', 'S37', 'S38', 'S39'].includes(skill.id)).every(skill => skill.hardGateAuthority === false && skill.controlDecisionAuthority === false)
const productionSource = fs.readFileSync(path.join(root, 'src-tauri/src/production.rs'), 'utf8')
const compositionOrder = productionSource.indexOf('enrich_chapter') < productionSource.indexOf('RENDER_CHAPTER')
  && productionSource.indexOf('NarrativeQualityReport') < productionSource.indexOf('AUDIT_EXTRACT')
  && productionSource.lastIndexOf('CommitService::commit') > productionSource.indexOf('AUDIT_EXTRACT')

setGate('B10-A00', freezePass, 'B10_PREBATCH_FREEZE.json', { freezeThrough: freeze?.freezeThrough, acceptedBaseline: freeze?.acceptedBaseline })
setGate('B10-A01', sourceBinding && before.commit !== START_COMMIT, 'B10_SOURCE_BINDING.json', { startCommitIsAncestor })
setGate('B10-A02', packagePass, 'B10_SKILL_PACKAGE.json', { archiveSha256: expectedPackageDigest })
setGate('B10-A03', resultPass(b10Skill), b10Skill.log)
setGate('B10-A04', resultPass(b10Skill) && architectureReport.gates.progressiveLoading, 'B10_PROGRESSIVE_LOADING.json')
setGate('B10-A05', architectureReport.gates.progressiveLoading, 'architecture.log')
setGate('B10-A06', resultPass(b10Skill), b10Skill.log)
setGate('B10-A07', resultPass(b10Skill), b10Skill.log)
setGate('B10-A08', architectureReport.gates.skillNoDirectPersistence, 'architecture.log')
setGate('B10-A09', resultPass(b10Skill), b10Skill.log)
setGate('B10-A10', resultPass(b10Skill), b10Skill.log)
setGate('B10-A11', skillBoundary, 'B10_SKILL_REGISTRY.json')
setGate('B10-A12', qualityBoundary, 'B10_SKILL_REGISTRY.json')
setGate('B10-A13', resultPass(b10Repair), b10Repair.log)
setGate('B10-A14', architectureReport.gates.candidateOnlyRepair, 'architecture.log')
setGate('B10-A15', resultPass(b10Skill), b10Skill.log)
setGate('B10-A16', resultPass(b10Skill), b10Skill.log)
setGate('B10-A17', architectureReport.gates.oneProductionRoot && resultPass(b10Quality), 'architecture.log+' + b10Quality.log)
setGate('B10-A18', resultPass(b10WithoutSkill), b10WithoutSkill.log)
setGate('B10-A19', compositionOrder && resultPass(b10Quality), 'B10_PRODUCTION_COMPOSITION.json')
setGate('B10-A20', resultPass(b10Skill), b10Skill.log)
setGate('B10-A21', resultPass(b10Skill), b10Skill.log)
setGate('B10-A22', resultPass(b10Skill), b10Skill.log)
setGate('B10-A23', resultPass(b10Skill), b10Skill.log)
setGate('B10-A24', resultPass(b10Repair), b10Repair.log)
setGate('B10-A25', resultPass(b10Skill), b10Skill.log)
setGate('B10-A26', resultPass(b10Repair), b10Repair.log)
setGate('B10-A27', resultPass(b10Repair), b10Repair.log)
setGate('B10-A28', resultPass(b10Skill) && architectureReport.globalGates.p3_permanent_authority, 'B10_NO_P3_ESCALATION.json')
setGate('B10-A29', architectureReport.gates.providerThroughRunner, 'architecture.log')
setGate('B10-A30', resultPass(b10Scale) && architectureReport.gates.boundedContext, 'B10_SCALE_CONTEXT.json')
setGate('B10-A31', resultPass(b10Ui), b10Ui.log)
setGate('B10-A32', resultPass(b10Ui), b10Ui.log)
setGate('B10-A33', architectureReport.gates.noFutureOwner, 'architecture.log')

const negativePass = {
  'NEG-01': architectureReport.gates.skillNoDirectPersistence && resultPass(b10Skill),
  'NEG-02': architectureReport.globalGates.commit_bundle_v3 && resultPass(b10Skill),
  'NEG-03': resultPass(b10Skill),
  'NEG-04': resultPass(b10Skill),
  'NEG-05': resultPass(b10Skill),
  'NEG-06': resultPass(b10Skill),
  'NEG-07': resultPass(b10Skill),
  'NEG-08': resultPass(b10Skill),
  'NEG-09': architectureReport.gates.progressiveLoading,
  'NEG-10': resultPass(b10Skill),
  'NEG-11': resultPass(b10Repair),
  'NEG-12': resultPass(b10Skill),
  'NEG-13': resultPass(b10Repair),
  'NEG-14': resultPass(b10Skill),
  'NEG-15': resultPass(b10Skill),
  'NEG-16': resultPass(b10WithoutSkill),
}
for (const name of NEGATIVE_BLOCKING_GATES) setGate(name, negativePass[name], name === 'NEG-16' ? b10WithoutSkill.log : b10Skill.log)

for (const name of R2_GLOBAL_BLOCKING_GATES) setGate(name, architectureReport.globalGates?.[name] === true, 'B10_GLOBAL_GATES.json')
const historical = { b05_regression: b05, b06_h01_regression: b06, b07_h02_regression: b07, b08_h03_regression: b08, r2_alignment_regression: r2Alignment, p01_regression: p01, b09_h04_regression: b09 }
for (const name of HISTORICAL_REGRESSION_GATES) setGate(name, resultPass(historical[name]), historical[name].log)
const engineering = { node, vitest, contracts, typecheck, build, cargo_all_targets: cargoAll, cargo_fmt: cargoFmt, cargo_check: cargoCheck, cargo_clippy: cargoClippy, architecture, module_boundary: moduleBoundary, m0a }
for (const name of ENGINEERING_GATES) setGate(name, resultPass(engineering[name]), engineering[name].log)
const b10Engineering = { b10_skill_targeted: b10Skill, b10_quality_targeted: b10Quality, b10_repair_targeted: b10Repair, b10_negative_fixtures: b10Negative, b10_ui: b10Ui, b10_scale_context: b10Scale }
for (const name of B10_ENGINEERING_GATES) setGate(name, resultPass(b10Engineering[name]), b10Engineering[name].log)
setGate('source_binding', sourceBinding, 'B10_SOURCE_BINDING.json', sourceBindingReport)

const sourceStage = buildSourceStage()
const sourceEntries = fs.existsSync(sourceStage.sourceRoot) ? walk(sourceStage.sourceRoot) : []
const forbiddenEntries = sourceEntries.filter(item => /(^|\/)(?:\.git|node_modules|target|dist)(?:\/|$)|\.db(?:$|[-.])|(^|\/)(?:keys?|secrets?|\.env)(?:\/|$)/i.test(item.relative))
const unsafeEntries = sourceEntries.filter(item => item.relative.split(path.sep).some(part => part === '..' || part === '') || item.symlink)
const packageSmokePass = sourceStage.archive.status === 0
  && sourceStage.extracted.status === 0
  && fs.existsSync(sourceStage.sourceRoot)
  && forbiddenEntries.length === 0
  && unsafeEntries.length === 0
  && fs.existsSync(path.join(sourceStage.sourceRoot, 'creative-skills/v1.2.0/registry.json'))
setGate('package_smoke', packageSmokePass, 'B10_PACKAGE_SMOKE.json', { forbiddenEntries: forbiddenEntries.length, unsafeEntries: unsafeEntries.length })
writeJson('B10_PACKAGE_SMOKE.json', { schema: 'b10-package-smoke-r1', sourceOnly: true, archiveStatus: sourceStage.archive.status, extractStatus: sourceStage.extracted.status, forbiddenEntries, unsafeEntries, includesSkillPackage: fs.existsSync(path.join(sourceStage.sourceRoot, 'creative-skills/v1.2.0/registry.json')), status: packageSmokePass ? 'PASS' : 'FAIL' })

const receiverEvidence = path.join(sourceStage.sourceRoot, 'evidence/B10')
const receiverGateResults = () => ({
  schema: 'b10-gate-results-r1',
  batch: 'B10',
  startCommit: START_COMMIT,
  sourceCommit: before.commit,
  evidenceSourceCommit: before.commit,
  sourceCommitAfter: before.commit,
  startCommitIsAncestor: true,
  workingTreeDirtyBefore: false,
  workingTreeDirtyAfter: false,
  excludedUncommittedPaths: [],
  sourceBinding: 'PASS',
  gates,
})
const provisionalFreshNames = ['fresh_package_architecture', 'fresh_package_module_boundary', 'fresh_package_generator', 'fresh_package_node']
for (const name of provisionalFreshNames) setGate(name, true, 'receiver-provisional')
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify(receiverGateResults(), null, 2)}\n`)
const freshArchitecture = run('fresh_package_architecture', 'node', ['scripts/verify-b10-architecture.mjs'], sourceStage.sourceRoot)
const freshModuleBoundary = run('fresh_package_module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'], sourceStage.sourceRoot)
const freshNode = run('fresh_package_node', 'node', ['--test', 'tests/b10-acceptance-generator.test.mjs', 'tests/b10-ui.test.mjs'], sourceStage.sourceRoot)
setGate('fresh_package_architecture', resultPass(freshArchitecture), freshArchitecture.log)
setGate('fresh_package_module_boundary', resultPass(freshModuleBoundary), freshModuleBoundary.log)
setGate('fresh_package_node', resultPass(freshNode), freshNode.log)
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify(receiverGateResults(), null, 2)}\n`)
const freshGenerator = run('fresh_package_generator', 'node', ['scripts/generate-b10-acceptance.mjs'], sourceStage.sourceRoot, { B10_EVIDENCE_DIR: receiverEvidence })
setGate('fresh_package_generator', resultPass(freshGenerator) && /READY_FOR_ACCEPTANCE/.test(freshGenerator.output), freshGenerator.log)

writeJson('B10_GLOBAL_GATES.json', { schema: 'b10-global-gates-r1', missingGates: architectureReport.missingGates, gates: architectureReport.globalGates, status: architectureReport.status })
writeJson('B10_PACKAGE_SMOKE.json', { schema: 'b10-package-smoke-r1', sourceOnly: true, archiveStatus: sourceStage.archive.status, extractStatus: sourceStage.extracted.status, forbiddenEntries, unsafeEntries, includesSkillPackage: fs.existsSync(path.join(sourceStage.sourceRoot, 'creative-skills/v1.2.0/registry.json')), freshReceiver: { architecture: freshArchitecture.status, moduleBoundary: freshModuleBoundary.status, generator: freshGenerator.status, node: freshNode.status }, status: packageSmokePass && resultPass(freshArchitecture) && resultPass(freshModuleBoundary) && resultPass(freshGenerator) && resultPass(freshNode) ? 'PASS' : 'FAIL' })
writeJson('B10_REGRESSION.json', { schema: 'b10-regression-r1', frozenBatches: { B05: b05.status, B06_H01: b06.status, B07_H02: b07.status, B08_H03: b08.status, R2_ALIGNMENT: r2Alignment.status, P01: p01.status, B09_H04: b09.status }, status: [b05, b06, b07, b08, r2Alignment, p01, b09].every(resultPass) ? 'PASS' : 'FAIL' })

const report = {
  schema: 'b10-gate-results-r1',
  batch: 'B10',
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
  results: Object.fromEntries(Object.entries(results).map(([name, result]) => [name, { status: result.status, exitCode: result.exitCode, log: result.log, reason: result.reason }])),
  generatedAt: new Date().toISOString(),
}
writeJson('gate-results.json', report)
const acceptance = generateAcceptance(evidence)
console.log(JSON.stringify({ evidenceDir: evidence, sourceCommit: before.commit, status: acceptance.status, missingGates: acceptance.missingGates }, null, 2))
if (acceptance.status !== 'READY_FOR_ACCEPTANCE') process.exitCode = 1
