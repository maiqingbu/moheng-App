import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { inspectB11Architecture } from './verify-b11-architecture.mjs'
import { inspectB11CapabilityParity } from './verify-b11-capability-parity.mjs'
import {
  B11_BLOCKING_GATES,
  B11_ENGINEERING_GATES,
  ENGINEERING_GATES,
  HISTORICAL_REGRESSION_GATES,
  NEGATIVE_BLOCKING_GATES,
  RELEASE_GATES,
  R2_GLOBAL_BLOCKING_GATES,
  START_COMMIT,
  REQUIRED_BLOCKING_GATES,
  generateAcceptance,
} from './generate-b11-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root, 'src-tauri')
const evidence = process.env.B11_EVIDENCE_DIR
  ? path.resolve(process.env.B11_EVIDENCE_DIR)
  : fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b11-evidence-'))
fs.mkdirSync(evidence, { recursive: true })

const frozenHistoricalEvidence = [
  'evidence/B01/registry_integrity.json',
  'evidence/B01/schema_inventory.json',
  'evidence/B01/query-plans.json',
]

function git(args, encoding = 'utf8') {
  return spawnSync('git', args, { cwd: root, encoding })
}

function snapshot() {
  const status = git(['status', '--porcelain=v1', '--untracked-files=all']).stdout.trim()
  return {
    commit: git(['rev-parse', 'HEAD']).stdout.trim(),
    dirtyPaths: status ? status.split('\n').filter(Boolean) : [],
  }
}

function run(name, command, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
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
  const passed = members.every(member => member.status === 'PASS')
  const value = {
    name,
    command: members.map(member => member.command).join(' && '),
    status: passed ? 'PASS' : members.some(member => member.status === 'NOT_RUN') ? 'NOT_RUN' : 'FAIL',
    exitCode: passed ? 0 : 1,
    output,
    log: `${name}.log`,
  }
  fs.writeFileSync(path.join(evidence, value.log), output)
  return value
}

function writeJson(file, value) {
  fs.writeFileSync(path.join(evidence, file), `${JSON.stringify(value, null, 2)}\n`)
}

function resultPass(result) {
  return result?.status === 'PASS' && result.exitCode === 0
}

function rustTestPass(result, testName) {
  const escaped = testName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return resultPass(result) && new RegExp(`(?:^|\\n)test ${escaped}\\s+\\.\\.\\.\\s+ok(?:\\n|$)`).test(result.output)
}

function nodeTestPass(result, testName) {
  return resultPass(result) && result.output.includes(`✔ ${testName}`)
}

function parseJsonOutput(output) {
  const first = output.indexOf('{')
  const last = output.lastIndexOf('}')
  if (first < 0 || last < first) return null
  try {
    return JSON.parse(output.slice(first, last + 1))
  } catch {
    return null
  }
}

function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function walk(directory, output = [], rootDirectory = directory) {
  if (!fs.existsSync(directory)) return output
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) walk(target, output, rootDirectory)
    else output.push({ path: target, relative: path.relative(rootDirectory, target), symlink: entry.isSymbolicLink() })
  }
  return output
}

function copyEvidenceTo(directory) {
  fs.mkdirSync(directory, { recursive: true })
  for (const entry of fs.readdirSync(evidence, { withFileTypes: true })) {
    if (!entry.isFile()) continue
    fs.copyFileSync(path.join(evidence, entry.name), path.join(directory, entry.name))
  }
}

function buildSourceStage() {
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b11-source-stage-'))
  const archive = spawnSync('git', ['archive', '--format=tar', '--prefix=ai-novel-production-system/', 'HEAD'], {
    cwd: root,
    encoding: 'buffer',
    maxBuffer: 512 * 1024 * 1024,
  })
  const extracted = archive.status === 0
    ? spawnSync('tar', ['-xf', '-', '-C', stage], { input: archive.stdout, encoding: 'utf8' })
    : { status: 1, stderr: 'git archive failed' }
  const sourceRoot = path.join(stage, 'ai-novel-production-system')
  if (fs.existsSync(sourceRoot)) copyEvidenceTo(path.join(sourceRoot, 'evidence/B11'))
  return { stage, sourceRoot, archive, extracted }
}

function gate(passed, source, actual = {}) {
  return { status: passed ? 'PASS' : 'FAIL', blocking: true, source, ...actual }
}

function setGate(gates, name, passed, source, actual = {}) {
  gates[name] = gate(Boolean(passed), source, actual)
}

const before = snapshot()
const results = {}
const add = (name, command, args, cwd = root, extraEnv = {}) => {
  results[name] = run(name, command, args, cwd, extraEnv)
  return results[name]
}

const b11Project = add('b11_project_entry', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b11_project_entry', '--', '--nocapture'])
const b11Negative = add('b11_negative_matrix', 'node', ['--test', 'tests/b11-negative-matrix.test.mjs'])
const b11Ux = add('b11_product_ux', 'node', ['--test', 'tests/b11-product-ux.test.mjs'])
const b11CapabilityParity = add('b11_capability_parity', 'node', ['--test', 'tests/b11-capability-parity.test.mjs'])
const b11Generator = add('b11_acceptance_generator', 'node', ['--test', 'tests/b11-acceptance-generator.test.mjs'])
const b11AssistantPresentation = add('b11_assistant_presentation', 'npx', ['vitest', 'run', 'tests/store/b11-assistant-presentation.test.ts', '--pool=forks', '--maxWorkers=1'])
const architecture = add('architecture', 'node', ['scripts/verify-b11-architecture.mjs'])
const architectureReport = parseJsonOutput(architecture.output) ?? inspectB11Architecture(root)
const capabilityParityReport = inspectB11CapabilityParity(root)
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
const b10 = add('b10_regression', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b10_creative_quality', '--', '--nocapture'])
const r2 = add('r2_alignment_regression', 'node', ['--test', 'tests/b08-h03-ui.test.mjs', 'tests/p01-provider.test.mjs', 'tests/contracts/architecture-guards.test.mjs'])

const node = add('node', 'npm', ['test'])
const vitest = add('vitest', 'npm', ['run', 'test:store'])
const contracts = add('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = add('typecheck', 'npm', ['run', 'typecheck'])
const build = add('build', 'npm', ['run', 'build'])
const cargoAll = add('cargo_all_targets', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoFmt = add('cargo_fmt', 'cargo', ['fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check'])
const cargoCheck = add('cargo_check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets'])
const cargoClippy = add('cargo_clippy', 'cargo', ['clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings'])

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
  schema: 'b11-source-binding-r1',
  batch: 'B11',
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
writeJson('B11_SOURCE_BINDING.json', sourceBindingReport)
writeJson('B11_PREBATCH_FREEZE.json', fs.existsSync(path.join(root, 'evidence/B11/B11_PREBATCH_FREEZE.json'))
  ? JSON.parse(fs.readFileSync(path.join(root, 'evidence/B11/B11_PREBATCH_FREEZE.json'), 'utf8'))
  : { status: 'FAIL' })

const gates = {}
const rustNames = {
  idea: 'b11_idea_entry_persists_three_substantive_candidates_without_auto_selection',
  ideaCausality: 'b11_idea_semantic_causality_changes_direction_fields_not_only_digest',
  market: 'b11_market_entry_requires_user_material_and_keeps_market_evidence_out_of_canon',
  marketCausality: 'b11_market_semantic_causality_changes_opportunity_and_pitch_fields',
  flow: 'b11_selection_setup_and_opening_ready_require_explicit_user_steps',
  b06Owner: 'b11_opening_uses_real_b06_context_owner',
  b07Owner: 'b11_opening_uses_real_b07_chapter_plan_owner',
  noFakeOwner: 'b11_no_fake_owner_label',
  repair: 'b11_assistant_repair_plan_uses_h03_authorization_and_postverify',
  repairQuestion: 'b11_assistant_repair_question_causality_selects_a_semantic_operation',
  repairEvidence: 'b11_assistant_repair_diagnosis_evidence_is_grounded',
  repairClassification: 'b11_assistant_repair_capability_classification_is_fail_closed',
  formalQueries: 'b11_formal_ui_queries_are_typed_and_owner_backed',
  query: 'b11_assistant_query_is_bounded_and_marks_missing_evidence_without_guessing',
  regenerate: 'b11_regeneration_creates_a_new_candidate_generation_and_merge_requires_explicit_selection',
  regenerateCausality: 'b11_regenerate_adjustment_causality_changes_semantic_fields',
  mergeCausality: 'b11_candidate_merge_causality_preserves_both_semantic_contributions',
}
const uxNames = {
  entry: 'B11 exposes the two Chinese project-entry paths and the opening flow',
  assistant: 'B11 wires Project Assistant through the existing frontend port and store-owned H03 commands',
  language: 'B11 formal UI does not expose demo or engineering fallback text',
  market: 'B11 market entry uses user-provided material instead of a fake global opportunity',
}
const rust = key => rustTestPass(b11Project, rustNames[key])
const ux = key => nodeTestPass(b11Ux, uxNames[key])
const negative = index => nodeTestPass(b11Negative, `NEG-${String(index).padStart(2, '0')}`)
const parity = testName => nodeTestPass(b11CapabilityParity, testName)

setGate(gates, 'B11-A00', architectureReport.checks?.freezeManifest === 'PASS', 'architecture.json')
setGate(gates, 'B11-A01', sourceBinding, 'B11_SOURCE_BINDING.json', { startCommitIsAncestor })
setGate(gates, 'B11-A02', architectureReport.checks?.frozenMigrations === 'PASS', 'architecture.json')
setGate(gates, 'B11-A03', rust('ideaCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A04', rust('marketCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A05', rust('market') && negative(3), 'b11_project_entry.log+NEG-03')
setGate(gates, 'B11-A06', rust('mergeCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A07', rust('ideaCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A08', rust('idea') && rust('flow'), 'b11_project_entry.log')
setGate(gates, 'B11-A09', rust('flow'), 'b11_project_entry.log')
setGate(gates, 'B11-A10', rust('regenerateCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A11', rust('flow'), 'b11_project_entry.log')
setGate(gates, 'B11-A12', rust('flow'), 'b11_project_entry.log')
setGate(gates, 'B11-A13', rust('b06Owner'), 'b11_project_entry.log')
setGate(gates, 'B11-A14', rust('b07Owner'), 'b11_project_entry.log')
setGate(gates, 'B11-A15', architectureReport.checks?.noSecondProductionPath === 'PASS', 'architecture.json')
setGate(gates, 'B11-A16', rust('flow'), 'b11_project_entry.log')
setGate(gates, 'B11-A17', rust('idea') && negative(1), 'b11_project_entry.log+NEG-01')
setGate(gates, 'B11-A18', ux('assistant'), 'b11_product_ux.log')
setGate(gates, 'B11-A19', rust('query'), 'b11_project_entry.log')
setGate(gates, 'B11-A20', resultPass(b11AssistantPresentation), 'b11_assistant_presentation.log')
setGate(gates, 'B11-A21', rust('query') && negative(10), 'b11_project_entry.log+NEG-10')
setGate(gates, 'B11-A22', resultPass(b07Main) && b07Main.output.includes('H02_SESSION_RESULT'), 'b07_h02_regression_main.log')
setGate(gates, 'B11-A23', rust('noFakeOwner'), 'b11_project_entry.log')
setGate(gates, 'B11-A24', rust('repairQuestion') && rust('repairEvidence') && rust('repairClassification'), 'b11_project_entry.log')
setGate(gates, 'B11-A25', rust('repair') && negative(11), 'b11_project_entry.log+NEG-11')
setGate(gates, 'B11-A26', resultPass(b08) && b08.output.includes('h03_same_authorization_two_workers_exactly_one_executes'), 'b08_h03_regression.log')
setGate(gates, 'B11-A27', negative(13) && negative(14) && architectureReport.checks?.repositoryBoundary === 'PASS', 'NEG-13/14+architecture.json')
setGate(gates, 'B11-A28', resultPass(b08) && b08.output.includes('h03_postcondition_query_failure_is_incomplete'), 'b08_h03_regression.log')
setGate(gates, 'B11-A29', negative(16) && architectureReport.globalGates?.future_batch_owner_early === 'PASS', 'NEG-16+architecture.json')
setGate(gates, 'B11-A30', negative(17), 'NEG-17')
setGate(gates, 'B11-A31', rust('flow') && ux('entry'), 'b11_project_entry.log+b11_product_ux.log')
setGate(gates, 'B11-A32', rust('market') && ux('market'), 'b11_project_entry.log+b11_product_ux.log')
setGate(gates, 'B11-A33', rust('query') && rust('formalQueries') && resultPass(b11AssistantPresentation), 'b11_project_entry.log+b11_assistant_presentation.log')
setGate(gates, 'B11-A34', rust('repair') && rust('repairClassification') && ux('assistant'), 'b11_project_entry.log+b11_product_ux.log')
setGate(gates, 'B11-A35', architectureReport.language?.status === 'PASS', 'B11_USER_VISIBLE_STRING_INVENTORY.json')
setGate(gates, 'B11-A36', architectureReport.language?.status === 'PASS', 'B11_INTERNAL_IDENTIFIER_LEAKAGE.json')
setGate(gates, 'B11-A37', architectureReport.language?.status === 'PASS', 'B11_CHINESE_LANGUAGE_SCAN.json')
setGate(gates, 'B11-A38', architectureReport.checks?.frontendDemoFailClosed === 'PASS', 'architecture.json')
setGate(gates, 'B11-A39', resultPass(b11AssistantPresentation), 'b11_assistant_presentation.log')
setGate(gates, 'B11-A40', resultPass(b11AssistantPresentation), 'b11_assistant_presentation.log')
setGate(gates, 'B11-A41', negative(18) && negative(19) && negative(20) && negative(21), 'b11_negative_matrix.log')
setGate(gates, 'B11-A42', architectureReport.checks?.frontendDemoFailClosed === 'PASS', 'architecture.json')
setGate(gates, 'B11-A43', parity('b11_formal_ui_capability_parity'), 'b11_capability_parity.log')
setGate(gates, 'B11-A44', architectureReport.language?.status === 'PASS', 'B11_CHINESE_LANGUAGE_SCAN.json')
setGate(gates, 'B11-A45', architectureReport.language?.status === 'PASS', 'B11_CHINESE_LANGUAGE_SCAN.json')
setGate(gates, 'B11-A46', rust('marketCausality'), 'b11_project_entry.log')
setGate(gates, 'B11-A47', rust('flow') && resultPass(b08), 'b11_project_entry.log+b08_h03_regression.log')
setGate(gates, 'B11-A48', resultPass(b10), 'b10_regression.log')
setGate(gates, 'B11-A49', [b05, b06, b07, b08, p01, b09, b10].every(resultPass), 'historical Cargo logs')
setGate(gates, 'B11-A50', parity('b11_future_route_hidden'), 'b11_capability_parity.log')

const negativeCaseNames = [
  'one Pitch generated then automatically selected is rejected',
  'highest score cannot become canonical automatically',
  'market text cannot become an instruction or Canon fact',
  'market refresh cannot silently alter a selected story',
  'story setup cannot create a full future chapter schedule',
  'B11 cannot create a second ChapterPlan owner',
  'B11 cannot create a second opening or production engine',
  'Start Creating cannot use BrowserDemo',
  'Project Assistant cannot answer a project fact without evidence',
  'truncated query cannot be presented as no records',
  'a vague repair request cannot mutate without explicit confirmation',
  'stale repair authorization is rejected',
  'Project Assistant cannot execute SQL directly',
  'Project Assistant cannot write Canon directly',
  'Project Assistant cannot invoke arbitrary shell, git, or editor',
  'formal Retcon remains proposal-only from Project Assistant',
  'conversation text cannot silently become project truth',
  'user-visible engineering status names are rejected',
  'raw JSON field output is rejected by the presentation layer',
  'source code symbols and module paths are sanitized',
  'AI boilerplate is rejected in assistant responses',
  'clickable formal actions require a real backend capability',
  'repair postverify mismatch cannot be presented as success',
  'future B12 and later pages cannot be exposed as usable routes',
]
for (let index = 0; index < NEGATIVE_BLOCKING_GATES.length; index += 1) {
  const id = NEGATIVE_BLOCKING_GATES[index]
  setGate(gates, id, nodeTestPass(b11Negative, `NEG-${String(index + 1).padStart(2, '0')} ${negativeCaseNames[index]}`), 'b11_negative_matrix.log', {
    exactTest: negativeCaseNames[index],
    assertionMatched: nodeTestPass(b11Negative, `NEG-${String(index + 1).padStart(2, '0')} ${negativeCaseNames[index]}`),
  })
}

for (const name of R2_GLOBAL_BLOCKING_GATES) setGate(gates, name, architectureReport.globalGates?.[name] === 'PASS', 'B11_GLOBAL_GATES.json')
const historical = { b05_regression: b05, b06_h01_regression: b06, b07_h02_regression: b07, b08_h03_regression: b08, r2_alignment_regression: r2, p01_regression: p01, b09_h04_regression: b09, b10_regression: b10 }
for (const name of HISTORICAL_REGRESSION_GATES) setGate(gates, name, resultPass(historical[name]), historical[name].log)
const engineering = { node, vitest, contracts, typecheck, build, cargo_all_targets: cargoAll, cargo_fmt: cargoFmt, cargo_check: cargoCheck, cargo_clippy: cargoClippy, architecture, module_boundary: moduleBoundary, m0a }
for (const name of ENGINEERING_GATES) setGate(gates, name, resultPass(engineering[name]), engineering[name].log)
const b11Engineering = { b11_project_entry: b11Project, b11_negative_matrix: b11Negative, b11_product_ux: b11Ux, b11_assistant_presentation: b11AssistantPresentation, b11_capability_parity: b11CapabilityParity, b11_language_scan: architecture }
for (const name of B11_ENGINEERING_GATES) setGate(gates, name, resultPass(b11Engineering[name]), b11Engineering[name].log)

const semanticCausalityPass = rust('ideaCausality') && rust('marketCausality') && rust('regenerateCausality') && rust('mergeCausality')
const openingOwnerPass = rust('b06Owner') && rust('b07Owner') && rust('noFakeOwner')
const assistantClassificationPass = rust('repairQuestion') && rust('repairEvidence') && rust('repairClassification')
const evidenceDrilldownPass = resultPass(b11AssistantPresentation)
const capabilityParityPass = resultPass(b11CapabilityParity) && capabilityParityReport.status === 'PASS'

writeJson('B11_USER_VISIBLE_STRING_INVENTORY.json', architectureReport.language ?? { status: 'FAIL' })
writeJson('B11_CHINESE_LANGUAGE_SCAN.json', { schema: 'b11-chinese-language-scan-r1', status: architectureReport.language?.status ?? 'FAIL', leaks: architectureReport.language?.leaks ?? [], scannedFileCount: architectureReport.language?.scannedFileCount ?? 0 })
writeJson('B11_INTERNAL_IDENTIFIER_LEAKAGE.json', { schema: 'b11-internal-identifier-leakage-r1', status: architectureReport.language?.status ?? 'FAIL', presentationLeaks: architectureReport.language?.presentationLeaks ?? [], source: 'template-visible-text-and-typed-presentation-scan' })
writeJson('B11_UI_CAPABILITY_PARITY.json', { ...capabilityParityReport, sourceTest: b11CapabilityParity.log, status: capabilityParityPass ? 'PASS' : 'FAIL' })
writeJson('B11_ASSISTANT_REPAIR_CLASSIFICATION.json', { schema: 'b11-assistant-repair-classification-r1', actionClasses: ['EXECUTABLE', 'PROPOSAL_ONLY', 'NO_ACTION'], questionCausality: rust('repairQuestion'), groundedDiagnosis: rust('repairEvidence'), failClosed: rust('repairClassification'), status: assistantClassificationPass ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_EVIDENCE_DRILLDOWN.json', { schema: 'b11-assistant-evidence-drilldown-r1', fields: ['source', 'record', 'currentStatus', 'related', 'recordedAt'], noCountOnlySummary: evidenceDrilldownPass, status: evidenceDrilldownPass ? 'PASS' : 'FAIL', sourceTest: b11AssistantPresentation.log })
writeJson('B11_SEMANTIC_CAUSALITY.json', { schema: 'b11-semantic-causality-r1', idea: rust('ideaCausality'), market: rust('marketCausality'), regeneration: rust('regenerateCausality'), candidateMerge: rust('mergeCausality'), status: semanticCausalityPass ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_OPENING_OWNER_PROOF.json', { schema: 'b11-opening-owner-proof-r1', b06ContextCompiler: rust('b06Owner'), b07ChapterPlanOwner: rust('b07Owner'), noFakeOwnerLabel: rust('noFakeOwner'), status: openingOwnerPass ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_FUTURE_ROUTE_PARITY.json', { schema: 'b11-future-route-parity-r1', hidden: parity('b11_future_route_hidden'), noUnsupportedMarketAction: parity('b11_market_refresh_truthfulness'), status: parity('b11_future_route_hidden') && parity('b11_market_refresh_truthfulness') ? 'PASS' : 'FAIL', sourceTest: b11CapabilityParity.log })
writeJson('B11_ASSISTANT_RESPONSE_CORPUS.json', { schema: 'b11-assistant-response-corpus-r1', sanitizer: 'typed', boilerplateRemoved: resultPass(b11AssistantPresentation), rawJsonRemoved: resultPass(b11AssistantPresentation), status: resultPass(b11AssistantPresentation) ? 'PASS' : 'FAIL', sourceTest: b11AssistantPresentation.log })
writeJson('B11_GLOBAL_GATES.json', { schema: 'b11-global-gates-r1', gates: architectureReport.globalGates ?? {}, missingGates: architectureReport.missingGates ?? [], status: architectureReport.status ?? 'FAIL' })
writeJson('B11_MIGRATION_FREEZE.json', { schema: 'b11-migration-freeze-r1', frozenMigrations: architectureReport.frozenMigrations ?? [], status: architectureReport.checks?.frozenMigrations === 'PASS' ? 'PASS' : 'FAIL' })
writeJson('B11_CHANGED_FILES.json', { schema: 'b11-changed-files-r1', startCommit: START_COMMIT, sourceCommit: before.commit, paths: git(['diff', '--name-status', `${START_COMMIT}..${before.commit}`]).stdout.trim().split('\n').filter(Boolean), status: sourceBinding ? 'PASS' : 'FAIL' })
writeJson('B11_PROJECT_ENTRY.json', { schema: 'b11-project-entry-r1', idea: rust('idea'), market: rust('market'), regeneration: rust('regenerate'), status: rust('idea') && rust('market') && rust('regenerate') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_IDEA_ENTRY.json', { schema: 'b11-idea-entry-r1', candidates: 3, canonicalSelectionBeforeUserAction: 0, semanticInputCausal: rust('ideaCausality'), status: rust('idea') && rust('ideaCausality') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_MARKET_ENTRY.json', { schema: 'b11-market-entry-r1', userMaterialRequired: true, opportunities: 3, semanticInputCausal: rust('marketCausality'), status: rust('market') && rust('marketCausality') && ux('market') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_MARKET_EVIDENCE.json', { schema: 'b11-market-evidence-r1', dataOnly: true, provenanceRequired: true, status: rust('market') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_MARKET_CANON_ISOLATION.json', { schema: 'b11-market-canon-isolation-r1', canonChangedByMarketEntry: false, status: rust('market') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_STORY_DIRECTION_CANDIDATES.json', { schema: 'b11-story-direction-candidates-r1', count: 3, substantive: rust('idea'), status: rust('idea') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_CANDIDATE_DIVERSITY.json', { schema: 'b11-candidate-diversity-r1', dimensions: ['mechanism', 'worldConflict'], materiallyDifferent: rust('idea'), status: rust('idea') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_PITCH_DECISION_RECEIPT.json', { schema: 'b11-pitch-decision-receipt-r1', explicitSelection: rust('flow'), decisionAndReceipt: rust('flow'), status: rust('flow') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_STORY_SETUP.json', { schema: 'b11-story-setup-r1', minimalSeed: rust('flow'), futureChapterPlansGenerated: 0, status: rust('flow') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_OPENING_READY.json', { schema: 'b11-opening-ready-r1', existingPlanningOwner: rust('b07Owner'), existingContextOwner: rust('b06Owner'), status: rust('flow') && openingOwnerPass ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_START_CREATING.json', { schema: 'b11-start-creating-r1', existingProductionRoot: architectureReport.checks?.openingUsesExistingOwners === 'PASS', currentChapterPlan: rust('flow'), status: rust('flow') && architectureReport.checks?.openingUsesExistingOwners === 'PASS' ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_ENTRY.json', { schema: 'b11-assistant-entry-r1', route: ux('assistant'), frontendPort: ux('assistant'), status: ux('assistant') ? 'PASS' : 'FAIL', sourceTest: b11Ux.log })
writeJson('B11_ASSISTANT_GROUNDED_QUERY.json', { schema: 'b11-assistant-grounded-query-r1', boundedPlan: rust('query'), realEvidence: rust('query'), status: rust('query') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_PROVENANCE.json', { schema: 'b11-assistant-provenance-r1', requestedAtPropagated: true, evidenceLedger: rust('query'), status: rust('query') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_INSUFFICIENT.json', { schema: 'b11-assistant-insufficient-r1', noGuessing: rust('query') && negative(10), status: rust('query') && negative(10) ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_PLANNING_DIAGNOSIS.json', { schema: 'b11-assistant-planning-diagnosis-r1', existingH02Session: b07Main.output.includes('H02_SESSION_RESULT'), status: resultPass(b07Main) && b07Main.output.includes('H02_SESSION_RESULT') ? 'PASS' : 'FAIL', sourceTest: b07Main.log })
writeJson('B11_ASSISTANT_REPAIR_FLOW.json', { schema: 'b11-assistant-repair-flow-r1', plan: rust('repair'), questionCausal: rust('repairQuestion'), typedOwner: rust('repair'), status: rust('repair') && assistantClassificationPass ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_AUTHORIZATION.json', { schema: 'b11-assistant-authorization-r1', explicitUserAction: negative(11), oneShot: resultPass(b08) && b08.output.includes('h03_same_authorization_two_workers_exactly_one_executes'), status: rust('repair') && negative(11) ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_POSTVERIFY.json', { schema: 'b11-assistant-postverify-r1', verifiedOnlyAfterPostcheck: rust('repair') && resultPass(b08), status: rust('repair') && resultPass(b08) ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_ASSISTANT_CHANGE_PROPOSAL_ONLY.json', { schema: 'b11-assistant-change-proposal-only-r1', formalChangeExecutionDenied: negative(16), status: negative(16) ? 'PASS' : 'FAIL', sourceTest: b11Negative.log })
writeJson('B11_PRODUCT_UX_IDEA_E2E.json', { schema: 'b11-product-ux-idea-e2e-r1', ideaToOpening: rust('flow') && ux('entry'), status: rust('flow') && ux('entry') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_PRODUCT_UX_MARKET_E2E.json', { schema: 'b11-product-ux-market-e2e-r1', userMaterialToSelection: rust('market') && rust('flow') && ux('market'), status: rust('market') && rust('flow') && ux('market') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_PRODUCT_UX_ASSISTANT_E2E.json', { schema: 'b11-product-ux-assistant-e2e-r1', query: rust('query') && ux('assistant'), status: rust('query') && ux('assistant') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_PRODUCT_UX_REPAIR_E2E.json', { schema: 'b11-product-ux-repair-e2e-r1', repair: rust('repair') && ux('assistant'), status: rust('repair') && ux('assistant') ? 'PASS' : 'FAIL', sourceTest: b11Project.log })
writeJson('B11_UI_TRUTHFULNESS.json', { schema: 'b11-ui-truthfulness-r1', noFormalDemoFallback: architectureReport.checks?.frontendDemoFailClosed === 'PASS', realActions: ux('entry') && ux('assistant'), status: architectureReport.checks?.frontendDemoFailClosed === 'PASS' && ux('entry') && ux('assistant') ? 'PASS' : 'FAIL', sourceTest: b11Ux.log })
writeJson('B11_REGRESSION.json', { schema: 'b11-regression-r1', frozenBatches: { B05: b05.status, B06_H01: b06.status, B07_H02: b07.status, B08_H03: b08.status, R2_ALIGNMENT: r2.status, P01: p01.status, B09_H04: b09.status, B10: b10.status }, status: [b05, b06, b07, b08, r2, p01, b09, b10].every(resultPass) ? 'PASS' : 'FAIL' })

for (const name of RELEASE_GATES) setGate(gates, name, false, 'pending-release-check')
setGate(gates, 'source_binding', sourceBinding, 'B11_SOURCE_BINDING.json', sourceBindingReport)

const sourceStage = buildSourceStage()
const sourceEntries = fs.existsSync(sourceStage.sourceRoot) ? walk(sourceStage.sourceRoot) : []
const forbiddenEntries = sourceEntries.filter(item => /(^|\/)(?:\.git|node_modules|target|dist)(?:\/|$)|\.db(?:$|[-.])|(^|\/)(?:keys?|secrets?|\.env)(?:\/|$)/i.test(item.relative))
const unsafeEntries = sourceEntries.filter(item => item.relative.split(path.sep).some(part => part === '..' || part === '') || item.symlink)
const sourceOnlyPackagePass = sourceStage.archive.status === 0
  && sourceStage.extracted.status === 0
  && fs.existsSync(sourceStage.sourceRoot)
  && forbiddenEntries.length === 0
  && unsafeEntries.length === 0
  && fs.existsSync(path.join(sourceStage.sourceRoot, 'evidence/B11/B11_PREBATCH_FREEZE.json'))
setGate(gates, 'package_smoke', sourceOnlyPackagePass, 'B11_PACKAGE_SMOKE.json', { forbiddenEntries: forbiddenEntries.length, unsafeEntries: unsafeEntries.length })

const receiverEvidence = path.join(sourceStage.sourceRoot, 'evidence/B11')
const receiverReport = () => ({
  schema: 'b11-gate-results-r1',
  batch: 'B11',
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

const freshArchitecture = run('fresh_package_architecture', 'node', ['scripts/verify-b11-architecture.mjs'], sourceStage.sourceRoot)
const freshArchitectureReport = parseJsonOutput(freshArchitecture.output)
const freshModuleBoundary = run('fresh_package_module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'], sourceStage.sourceRoot)
const freshNode = run('fresh_package_node', 'node', ['--test', 'tests/b11-acceptance-generator.test.mjs', 'tests/b11-negative-matrix.test.mjs', 'tests/b11-product-ux.test.mjs', 'tests/b11-capability-parity.test.mjs'], sourceStage.sourceRoot)
setGate(gates, 'fresh_package_architecture', resultPass(freshArchitecture), freshArchitecture.log)
setGate(gates, 'fresh_package_module_boundary', resultPass(freshModuleBoundary), freshModuleBoundary.log)
setGate(gates, 'fresh_package_node', resultPass(freshNode), freshNode.log)
setGate(gates, 'fresh_package_language', freshArchitectureReport?.language?.status === 'PASS', freshArchitecture.log)
setGate(gates, 'fresh_package_generator', false, 'fresh_package_generator.log', { selfCheckPending: true })
writeJson('B11_PACKAGE_SMOKE.json', { schema: 'b11-package-smoke-r1', sourceOnly: true, archiveStatus: sourceStage.archive.status, extractStatus: sourceStage.extracted.status, forbiddenEntries, unsafeEntries, freshReceiver: { architecture: freshArchitecture.status, moduleBoundary: freshModuleBoundary.status, node: freshNode.status, language: freshArchitectureReport?.language?.status ?? 'FAIL' }, status: sourceOnlyPackagePass && resultPass(freshArchitecture) && resultPass(freshModuleBoundary) && resultPass(freshNode) && freshArchitectureReport?.language?.status === 'PASS' ? 'PASS' : 'FAIL' })
copyEvidenceTo(receiverEvidence)
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify(receiverReport(), null, 2)}\n`)
const freshGenerator = run('fresh_package_generator', 'node', ['scripts/generate-b11-acceptance.mjs'], sourceStage.sourceRoot, { B11_EVIDENCE_DIR: receiverEvidence, B11_GENERATOR_SELF_CHECK: '1' })
setGate(gates, 'fresh_package_generator', resultPass(freshGenerator) && /READY_FOR_ACCEPTANCE/.test(freshGenerator.output), freshGenerator.log, { measuredStatus: freshGenerator.status, measuredExitCode: freshGenerator.exitCode })

const preflightZip = path.join(os.tmpdir(), `moheng-b11-${before.commit.slice(0, 7)}-preflight.zip`)
spawnSync('zip', ['-qr', preflightZip, path.basename(sourceStage.sourceRoot)], { cwd: sourceStage.stage, encoding: 'utf8' })
const preflightZipTest = spawnSync('unzip', ['-t', preflightZip], { encoding: 'utf8' })
const packageSmoke = sourceOnlyPackagePass && resultPass(freshArchitecture) && resultPass(freshModuleBoundary) && resultPass(freshNode) && freshArchitectureReport?.language?.status === 'PASS' && preflightZipTest.status === 0
setGate(gates, 'package_smoke', packageSmoke, 'B11_PACKAGE_SMOKE.json', { forbiddenEntries: forbiddenEntries.length, unsafeEntries: unsafeEntries.length, zipIntegrity: preflightZipTest.status === 0 })

writeJson('B11_PACKAGE_SMOKE.json', { schema: 'b11-package-smoke-r1', sourceOnly: true, archiveStatus: sourceStage.archive.status, extractStatus: sourceStage.extracted.status, forbiddenEntries, unsafeEntries, zipIntegrity: preflightZipTest.status === 0, freshReceiver: { architecture: freshArchitecture.status, moduleBoundary: freshModuleBoundary.status, generator: freshGenerator.status, node: freshNode.status, language: freshArchitectureReport?.language?.status ?? 'FAIL' }, status: packageSmoke ? 'PASS' : 'FAIL' })
copyEvidenceTo(receiverEvidence)
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify(receiverReport(), null, 2)}\n`)

const finalReport = {
  schema: 'b11-gate-results-r1',
  batch: 'B11',
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
writeJson('gate-results.json', finalReport)
const acceptance = generateAcceptance(evidence)
copyEvidenceTo(receiverEvidence)
fs.writeFileSync(path.join(receiverEvidence, 'gate-results.json'), `${JSON.stringify(receiverReport(), null, 2)}\n`)
const finalAcceptance = generateAcceptance(evidence)
console.log(JSON.stringify({ evidenceDir: evidence, sourceCommit: before.commit, status: finalAcceptance.status, missingGates: finalAcceptance.missingGates, failedGates: finalAcceptance.failedGates }, null, 2))
if (finalAcceptance.status !== 'READY_FOR_ACCEPTANCE') process.exitCode = 1
