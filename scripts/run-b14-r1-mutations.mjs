import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R1_EVIDENCE_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r1-mutations-')))
const mutationDir = path.join(evidenceDir, 'B14_R1', 'MUTATIONS')
fs.mkdirSync(mutationDir, { recursive: true })

function run(command, args) {
  return spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
}
function mutation(id, file, needle, replacement, command, args) {
  const target = path.join(root, file)
  const original = fs.readFileSync(target)
  const source = original.toString('utf8')
  const index = source.indexOf(needle)
  if (index < 0) return { id, status: 'FAIL', reason: 'needle-not-found' }
  fs.writeFileSync(target, source.slice(0, index) + replacement + source.slice(index + needle.length))
  let result
  try { result = run(command, args) } finally { fs.writeFileSync(target, original) }
  const expectedFailure = result.status !== 0
  const report = { id, file, command: [command, ...args].join(' '), mutationIntroducedExitCode: result.status ?? 1, status: expectedFailure ? 'PASS' : 'FAIL' }
  fs.writeFileSync(path.join(mutationDir, `${id}.json`), `${JSON.stringify(report, null, 2)}\n`)
  fs.writeFileSync(path.join(mutationDir, `${id}.log`), `${result.stdout || ''}${result.stderr || ''}`)
  return report
}

const results = []
results.push(mutation('MUT-02', 'src-tauri/src/endgame.rs', 'if unresolved_obligations > 0 {', 'if unresolved_obligations > 999 {', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame', 'hard_obligation_blocks_completion']))
results.push(mutation('MUT-03', 'src-tauri/src/repositories/endgame.rs', '== Some(current_digest.as_str());', '== Some("never-matches");', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_unrelated_revision_does_not_make_endgame_contract_stale']))
results.push(mutation('MUT-04', 'src-tauri/src/endgame.rs', '"softAdvisories": input.soft_advisories,', '"softAdvisories": input.soft_advisories, "detailLimit": input.detail_limit,', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame', 'detail_page_limit_does_not_change_global_truth_or_digest']))
results.push(mutation('MUT-05', 'src-tauri/src/endgame.rs', 'let stale_count = i64::from(input.pending_relevant_change) + i64::from(input.plan_stale);', 'let stale_count = 0_i64;', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame', 'relevant_change_and_stale_plan_block_completion']))
results.push(mutation('MUT-06', 'src/ui/pages/Completion.vue', 'title="终局与完结"', 'title="BookExitState"', 'node', ['scripts/verify-product-ux.mjs']))
results.push(mutation('MUT-07', 'src-tauri/src/endgame.rs', 'use serde_json::json;', 'use serde_json::json;\npub struct ExportRuntime;', process.execPath, ['scripts/verify-b14-architecture.mjs']))
results.push(mutation('MUT-08', 'src-tauri/src/commands.rs', 'if history.currently_completed {', 'if history.has_history {', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_relevant_change_requires_recheck_and_allows_new_digest_completion']))

const status = results.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
fs.writeFileSync(path.join(evidenceDir, 'B14_R1_MUTATION_GATES.json'), `${JSON.stringify({ schema: 'b14-r1-mutation-gates', results, status }, null, 2)}\n`)
console.log(JSON.stringify({ status, results }, null, 2))
if (status !== 'PASS') process.exitCode = 1
