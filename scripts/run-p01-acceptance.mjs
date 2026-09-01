import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const rustRoot = path.join(root, 'src-tauri')
const evidence = process.env.P01_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-p01-evidence.'))
fs.mkdirSync(evidence, { recursive: true })
const startCommit = '59206aa7c736025ff5ade55991437f5ed33cf8e3'
const beforeCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const dirtyBefore = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0
function run(name, command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', env: process.env })
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
  fs.writeFileSync(path.join(evidence, `${name}.log`), output)
  return { status: result.status === 0 ? 'PASS' : 'FAIL', output }
}
const p01 = run('p01_targeted', 'cargo', ['test', '--test', 'p01_provider', '--', '--nocapture'], rustRoot)
const b05 = run('b05_regression', 'cargo', ['test', '--test', 'b05_narrative'], rustRoot)
const b06 = run('b06_h01_regression', 'cargo', ['test', '--test', 'b06_h01'], rustRoot)
const b07 = run('b07_h02_regression', 'cargo', ['test', '--test', 'b07_h02', '--test', 'b07_h02_semantic_negative'], rustRoot)
const b08 = run('b08_h03_regression', 'cargo', ['test', '--test', 'b08_h03'], rustRoot)
const b03 = run('b03_regression', 'cargo', ['test', '--test', 'b03_runtime'], rustRoot)
const node = run('node', 'npm', ['test'])
const vitest = run('vitest', 'npm', ['run', 'test:store'])
const contracts = run('contracts', 'npm', ['run', 'verify:contracts'])
const typecheck = run('typecheck', 'npm', ['run', 'typecheck'])
const build = run('build', 'npm', ['run', 'build'])
const architecture = run('architecture', 'node', ['scripts/verify-b08-h03-architecture.mjs'])
const boundary = run('module_boundary', 'node', ['scripts/verify-module-boundaries.mjs'])
const m0a = run('m0a', 'node', ['scripts/verify-m0a-evidence.mjs'])
const fmt = run('cargo_fmt', 'cargo', ['fmt', '--all', '--', '--check'], rustRoot)
const check = run('cargo_check', 'cargo', ['check', '--all-targets'], rustRoot)
const clippy = run('cargo_clippy', 'cargo', ['clippy', '--all-targets', '--all-features', '--', '-D', 'warnings'], rustRoot)
const allTargets = run('cargo_all_targets', 'cargo', ['test', '--all-targets'], rustRoot)
const current = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
if (!dirtyBefore) {
  for (const file of ['evidence/B01/registry_integrity.json', 'evidence/B01/schema_inventory.json']) spawnSync('git', ['restore', '--source=HEAD', '--', file], { cwd: root })
}
const dirtyAfter = spawnSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).stdout.trim().length > 0
const sourceBinding = beforeCommit === current && !dirtyBefore && !dirtyAfter && beforeCommit !== startCommit
const has = (result, marker) => result.status === 'PASS' && result.output.includes(marker)
const gates = {
  'P01-A01': { status: sourceBinding ? 'PASS' : 'FAIL', source: 'git' },
  'P01-A02': { status: has(architecture, 'singleProductionRoot') && has(p01, 'test p01_configured_gateway_requires_secret_and_never_returns_it'), source: 'architecture.log+p01_targeted.log' },
  'P01-A03': { status: has(p01, 'test p01_configured_gateway_requires_secret_and_never_returns_it') && !/UnavailableProductionGateway/.test(fs.readFileSync(path.join(root, 'src-tauri/src/commands.rs'), 'utf8')), source: 'p01_targeted.log+commands.rs' },
  'P01-A04': { status: has(p01, 'test p01_settings_persist_without_api_key_and_provider_adapter_maps_transport_error'), source: 'p01_targeted.log' },
  'P01-A05': { status: has(p01, 'test p01_openai_compatible_adapter_maps_real_http_success_auth_and_timeout'), source: 'p01_targeted.log' },
  'P01-A06': { status: has(p01, 'test p01_formal_budget_binding_blocks_provider_before_send_and_settles_success_cost'), source: 'p01_targeted.log' },
  'P01-A07': { status: has(p01, 'test p01_formal_provider_raw_response_persists_before_model_payload_parse') && has(p01, 'test p01_formal_raw_persist_failure_never_invokes_parse_or_authority_mutation'), source: 'p01_targeted.log' },
  'P01-A08': { status: has(p01, 'test p01_formal_configured_gateway_timeout_preserves_unknown_remote_outcome') && has(p01, 'test p01_formal_configured_gateway_unknown_outcome_preserves_decision_boundary'), source: 'p01_targeted.log' },
  'P01-A09': { status: has(p01, 'test p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist'), source: 'p01_targeted.log' },
  'P01-A10': { status: has(p01, 'test p01_configured_gateway_requires_secret_and_never_returns_it') && has(node, 'P01 provider errors and capability truthfulness are typed'), source: 'p01_targeted.log+node.log' },
  'P01-A11': { status: architecture.status === 'PASS' && node.status === 'PASS', source: 'architecture.log+node.log' },
  'P01-A12': { status: has(b08, 'b08_plan_expected_satisfied_but_actual_active_keeps_reality_active'), source: 'b08_h03_regression.log' },
  'P01-A13': { status: has(p01, 'test p01_openai_compatible_adapter_maps_real_http_success_auth_and_timeout') && b08.status === 'PASS', source: 'p01_targeted.log+b08_h03_regression.log' },
  'P01-A14': { status: architecture.status === 'PASS', source: 'architecture.log' },
  r2_global: { status: architecture.status === 'PASS' && boundary.status === 'PASS' && m0a.status === 'PASS', source: 'architecture.log+module_boundary.log+m0a.log' },
  b05_regression: { status: b05.status === 'PASS', source: 'b05_regression.log' },
  b06_h01_regression: { status: b06.status === 'PASS', source: 'b06_h01_regression.log' },
  b07_h02_regression: { status: b07.status === 'PASS', source: 'b07_h02_regression.log' },
  b08_h03_regression: { status: b08.status === 'PASS', source: 'b08_h03_regression.log' },
  node: { status: node.status === 'PASS', source: 'node.log' }, vitest: { status: vitest.status === 'PASS', source: 'vitest.log' }, contracts: { status: contracts.status === 'PASS', source: 'contracts.log' }, typecheck: { status: typecheck.status === 'PASS', source: 'typecheck.log' }, build: { status: build.status === 'PASS', source: 'build.log' },
  cargo_all_targets: { status: allTargets.status === 'PASS', source: 'cargo_all_targets.log' }, cargo_fmt: { status: fmt.status === 'PASS', source: 'cargo_fmt.log' }, cargo_check: { status: check.status === 'PASS', source: 'cargo_check.log' }, cargo_clippy: { status: clippy.status === 'PASS', source: 'cargo_clippy.log' }, architecture: { status: architecture.status === 'PASS', source: 'architecture.log' }, module_boundary: { status: boundary.status === 'PASS', source: 'module_boundary.log' }, m0a: { status: m0a.status === 'PASS', source: 'm0a.log' }, source_binding: { status: sourceBinding ? 'PASS' : 'FAIL', source: 'git' },
}
for (const gate of Object.values(gates)) gate.status = gate.status === true || gate.status === 'PASS' ? 'PASS' : 'FAIL'
const allGatesPass = Object.values(gates).every(gate => gate.status === 'PASS')
const evidenceBinding = {
  startCommit,
  sourceCommit: beforeCommit,
  evidenceSourceCommit: beforeCommit,
  sourceCommitAfter: current,
  workingTreeDirtyBefore: dirtyBefore,
  workingTreeDirtyAfter: dirtyAfter,
  excludedUncommittedPaths: [],
  sourceBinding: sourceBinding ? 'PASS' : 'FAIL',
  status: sourceBinding ? 'PASS' : 'FAIL',
}
fs.writeFileSync(path.join(evidence, 'P01_SOURCE_BINDING.json'), `${JSON.stringify(evidenceBinding, null, 2)}\n`)
const formalEvidence = (test, pass, extra = {}) => ({
  schema: 'moheng-v2.1-p01-formal-evidence-r1',
  status: sourceBinding && pass ? 'PASS' : 'FAIL',
  sourceCommit: beforeCommit,
  evidenceSourceCommit: beforeCommit,
  sourceCommitAfter: current,
  workingTreeDirtyBefore: dirtyBefore,
  workingTreeDirtyAfter: dirtyAfter,
  excludedUncommittedPaths: [],
  test,
  log: 'p01_targeted.log',
  ...extra,
})
fs.writeFileSync(path.join(evidence, 'P01_FORMAL_RAW_RESPONSE.json'), `${JSON.stringify(formalEvidence('p01_formal_provider_raw_response_persists_before_model_payload_parse', gates['P01-A07'].status === 'PASS', { rawPersistFaultTest: 'p01_formal_raw_persist_failure_never_invokes_parse_or_authority_mutation' }), null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'P01_FORMAL_UNKNOWN_REMOTE_OUTCOME.json'), `${JSON.stringify(formalEvidence('p01_formal_configured_gateway_timeout_preserves_unknown_remote_outcome', gates['P01-A08'].status === 'PASS', { uncertainOutcomeTest: 'p01_formal_configured_gateway_unknown_outcome_preserves_decision_boundary' }), null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'P01_FORMAL_BUDGET_BINDING.json'), `${JSON.stringify(formalEvidence('p01_formal_budget_binding_blocks_provider_before_send_and_settles_success_cost', gates['P01-A06'].status === 'PASS', { budgetAuthority: 'chapter_production_budgets', realProviderUsageTest: 'p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist' }), null, 2)}\n`)
fs.writeFileSync(path.join(evidence, 'P01_FORMAL_PROVIDER_USAGE.json'), `${JSON.stringify(formalEvidence('p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist', gates['P01-A09'].status === 'PASS', { provider: 'OpenAiCompatibleProvider', mapping: { prompt_tokens: 'input_tokens', completion_tokens: 'output_tokens', total_tokens: 'total_tokens', cached_tokens: 'cachedTokens', id: 'providerRequestId' } }), null, 2)}\n`)
const status = allGatesPass ? 'READY_FOR_REACCEPTANCE' : 'FAIL'
fs.writeFileSync(path.join(evidence, 'P01_ACCEPTANCE.json'), `${JSON.stringify({ schema: 'moheng-v2.1-p01-acceptance-r1', status, sourceCommit: beforeCommit, evidenceSourceCommit: beforeCommit, sourceCommitAfter: current, workingTreeDirtyBefore: dirtyBefore, workingTreeDirtyAfter: dirtyAfter, excludedUncommittedPaths: [], gates }, null, 2)}\n`)
console.log(`P01 gates: ${Object.values(gates).filter(gate => gate.status === 'PASS').length}/${Object.keys(gates).length} PASS; status=${status}; sourceBinding=${sourceBinding ? 'PASS' : 'FAIL'}; evidence=${evidence}`)
if (!allGatesPass) process.exitCode = 1
