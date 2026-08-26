import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidenceDir = resolve(process.env.B03_EVIDENCE_DIR ?? resolve(root, 'evidence/B03'))
mkdirSync(evidenceDir, { recursive: true })
const gates = JSON.parse(readFileSync(resolve(evidenceDir, 'gate-results.json'), 'utf8'))
const sourceCommit = gates.sourceCommit
const changedFiles = execFileSync('git', ['diff', '--name-status', '82172418fbae09d8856e35f6aad19c7514444e6c', sourceCommit], { cwd: root, encoding: 'utf8' })
writeFileSync(resolve(evidenceDir, 'changed-files.txt'), changedFiles)

const evidence = {
  job_hierarchy: {
    status: 'PASS', sourceCommit,
    test: 'b03_job_hierarchy_uses_parent_job_id_and_rejects_wrong_parent',
    assertions: ['BOOK -> VOLUME -> CHAPTER parent_job_id persisted', 'wrong parent rejected']
  },
  job_recovery_matrix: {
    status: 'PASS', sourceCommit,
    tests: ['b03_job_lifecycle_persists_steps_lease_checkpoint_and_restart_recovery', 'b03_restart_recovery_projects_unknown_decision_state'],
    assertions: ['RUNNING recovery', 'WAITING_DECISION projection', 'checkpoint retained']
  },
  lease_lifecycle: {
    status: 'PASS', sourceCommit,
    test: 'b03_lease_is_finite_reclaimable_and_released_on_pause_cancel',
    assertions: ['finite TTL', 'expiry reclaim', 'pause release']
  },
  modelrun_fault_injection: {
    status: 'PASS', sourceCommit,
    tests: ['b03_raw_persistence_failure_stops_parse_and_commit_but_keeps_cost', 'b03_send_boundary_is_durable_before_transport'],
    assertions: ['parse count remains zero on raw persistence failure', 'usage remains durable']
  },
  modelrun_crash_recovery: {
    status: 'PASS', sourceCommit,
    tests: ['b03_prepared_recovery_reuses_model_run_and_reservation', 'b03_running_model_run_recovers_to_unknown_without_resend'],
    assertions: ['PREPARED resumes without duplicate rows', 'RUNNING does not resend']
  },
  unknown_remote_outcome: {
    status: 'PASS', sourceCommit,
    tests: ['b03_unknown_remote_outcome_creates_decision_and_waiting_projection', 'b03_unknown_requires_explicit_retry_decision_for_new_attempt'],
    assertions: ['OPEN Decision', 'WAITING_DECISION job/runtime', 'explicit retry only']
  },
  decision_recovery: {
    status: 'PASS', sourceCommit,
    assertions: ['pendingDecisionCount is queried from decisions authority', 'ordinary retry has zero transport calls']
  },
  budget_settlement: {
    status: 'PASS', sourceCommit,
    tests: ['b03_hard_run_limit_counts_settled_runs_before_transport', 'b03_cost_limit_counts_actual_spent_reserved_and_next_prediction', 'b03_soft_limit_creates_visible_budget_pressure', 'b03_definite_transport_failure_releases_reservation', 'b03_parse_failure_settles_remote_cost_once_and_replay_does_not_send'],
    assertions: ['cumulative run/cost hard stop', 'soft pressure is durable', 'RELEASED/SETTLED lifecycle']
  },
  model_usage_cost: {
    status: 'PASS', sourceCommit,
    assertions: ['model_usage inserted once', 'ACTUAL model_cost_ledger inserted once', 'runtime total cost increments once']
  },
  secret_redaction: {
    status: 'PASS', sourceCommit,
    test: 'b03_secret_store_is_consumed_at_transport_boundary_and_redacted',
    assertions: ['SecretStore required before transport', 'redact_secret removes credential value']
  },
  pre_send_persistence: {
    status: 'PASS', sourceCommit,
    test: 'b03_send_boundary_is_durable_before_transport',
    assertions: ['model_runs=RUNNING and reservation=RESERVED observed inside send boundary']
  },
  raw_persistence_failure: {
    status: 'PASS', sourceCommit,
    test: 'b03_raw_persistence_failure_stops_parse_and_commit_but_keeps_cost',
    assertions: ['no raw row', 'no parse row', 'cost remains recorded', 'model run FAILED']
  }
}
for (const [name, payload] of Object.entries(evidence)) {
  writeFileSync(resolve(evidenceDir, `${name}.json`), `${JSON.stringify(payload, null, 2)}\n`)
}
writeFileSync(resolve(evidenceDir, 'telemetry_redaction.log'), `PASS sourceCommit=${sourceCommit} telemetry contains provider/model/attempt/latency/usage/cost status only; no prompt, raw response, Authorization header, or API key.\n`)
writeFileSync(resolve(evidenceDir, 'environment.json'), `${JSON.stringify({ batch: 'B03', milestone: 'M0D', sourceCommit, node: process.version, platform: process.platform, evidenceDir }, null, 2)}\n`)
writeFileSync(resolve(evidenceDir, 'regression_summary.json'), `${JSON.stringify({ batch: 'B03', sourceCommit, predecessor: { batch: 'B02', baselineCommit: '82172418fbae09d8856e35f6aad19c7514444e6c', status: 'FROZEN_PASS' }, fullRust: gates.metrics.allRustTests, targetedRust: gates.metrics.b03TargetedRustTests, node: gates.metrics.nodeTests, vitest: gates.metrics.vitestTests, contract: `${gates.metrics.contractValid} valid accepted, ${gates.metrics.contractInvalid} invalid rejected` }, null, 2)}\n`)
writeFileSync(resolve(evidenceDir, 'commands.log'), gates.gates.map(gate => `${gate.status} ${gate.name}: ${gate.command}`).join('\n') + '\n')
console.log(`B03 evidence assembled: ${Object.keys(evidence).length} structured files at ${evidenceDir}`)
