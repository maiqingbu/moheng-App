import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R5_EVIDENCE_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r5-mutations-')))
const mutationDir = path.join(evidenceDir, 'B14_R5', 'MUTATIONS')
fs.mkdirSync(mutationDir, { recursive: true })
const run = (command, args, env = {}) => {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 128 * 1024 * 1024 })
  return { exitCode: result.status ?? 1, output: `${result.stdout ?? ''}${result.stderr ?? ''}` }
}
const specs = [
  ['MUT-R5-01', 'src/application/endgameMapping.ts', "return kind === 'reservation'\n    ? workflow.reservationCandidates[ref] ?? []\n    : workflow.hardPayoffCandidates[ref] ?? []", 'return workflow.fulfillmentCandidates as EndgameMappingCandidate[]', 'npx', ['vitest', 'run', 'tests/store/b14-r5-mapping-ui.test.ts']],
  ['MUT-R5-02', 'src-tauri/src/commands.rs', 'EndgameRepository::current_hard_task_ids(\n                        connection, project_id,\n                    )', 'Vec::new()', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_r5_empty_runtime_checks_recheck_current_task_and_thread_scope']],
  ['MUT-R5-03', 'src-tauri/tests/b14_endgame_integration.rs', 'INSERT INTO reader_desires(desire_id', 'INSERT INTO reader_desires_missing(desire_id', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_r5_no_authority_mutation_uses_non_empty_domain_fixtures']],
  ['MUT-R5-04', 'src-tauri/src/repositories/narrative.rs', 'let page_len = page.len() as i64;', 'let page_len = page.len() as i64; break;', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_r4_continuity_complete_reads_501_rows_and_exposes_hard_thread']],
  ['MUT-R5-05', 'src-tauri/tests/b14_r2_scale.rs', 'assert!(!rows.is_empty());', 'rows.truncate(1); assert!(!rows.is_empty());', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_r2_scale', 'b14_r2_runs_formal_completion_queries_at_ten_million_scale_without_page_drift']],
]
const results = []
for (const [id, file, needle, replacement, command, args] of specs) {
  const absolute = path.join(root, file)
  const original = fs.readFileSync(absolute, 'utf8')
  if (!original.includes(needle)) throw new Error(`B14_R5_MUTATION_TARGET_MISSING:${id}`)
  const folder = path.join(mutationDir, id)
  fs.mkdirSync(folder, { recursive: true })
  const beforeStatus = run('git', ['status', '--porcelain', '--untracked-files=all']).output
  let introduced
  try {
    fs.writeFileSync(absolute, original.replace(needle, replacement))
    const patchSha = crypto.createHash('sha256').update(`${file}\n${needle}\n${replacement}\n`).digest('hex')
    fs.writeFileSync(path.join(folder, 'patch.diff'), `${file}: ${needle} -> ${replacement}\npatchSha=${patchSha}\n`)
    introduced = run(command, args)
    fs.writeFileSync(path.join(folder, 'introduced.log'), introduced.output)
  } finally {
    fs.writeFileSync(absolute, original)
  }
  const reverted = run(command, args)
  fs.writeFileSync(path.join(folder, 'reverted.log'), reverted.output)
  const afterStatus = run('git', ['status', '--porcelain', '--untracked-files=all']).output
  const result = { id, file, introducedCommand: [command, ...args].join(' '), introducedExit: introduced.exitCode, introducedExpectedFailure: introduced.exitCode !== 0, revertedCommand: [command, ...args].join(' '), revertedExit: reverted.exitCode, gitCleanAfter: beforeStatus === afterStatus, status: introduced.exitCode !== 0 && reverted.exitCode === 0 && beforeStatus === afterStatus ? 'PASS' : 'FAIL' }
  fs.writeFileSync(path.join(folder, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
  results.push(result)
}

const frozenFolder = path.join(mutationDir, 'MUT-R5-06')
fs.mkdirSync(frozenFolder, { recursive: true })
const tempInput = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b14-r5-frozen-mutation-'))
const frozenNames = [
  '墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md',
  '墨衡_V2.1_B14_R1_定点返工与重新打包执行指令_2026-09-06.md',
  '墨衡_V2.1_B14_R2_最终集成返工与重新打包执行指令_2026-09-06.md',
  '墨衡_V2.1_B14_R3_最终封口返工与正式重新打包执行指令_2026-09-06.md',
  '墨衡_V2.1_B14_R4_最终微补丁与正式重新打包执行指令_2026-09-07.md',
  '墨衡_V2.1_B14_R5_两个集成漏接与Evidence真实性最终封口执行指令_2026-09-07.md',
  '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B13_2026-09-05.md',
]
for (const name of frozenNames) fs.copyFileSync(path.join(process.env.B14_R5_INPUT_DIR ?? '/Users/mac/Downloads', name), path.join(tempInput, name))
fs.appendFileSync(path.join(tempInput, frozenNames[2]), '\nR5 mutation\n')
const introducedFrozen = run('node', ['scripts/verify-b14-r5-frozen-inputs.mjs'], { B14_R5_INPUT_DIR: tempInput })
fs.writeFileSync(path.join(frozenFolder, 'introduced.log'), introducedFrozen.output)
for (const name of frozenNames) fs.copyFileSync(path.join(process.env.B14_R5_INPUT_DIR ?? '/Users/mac/Downloads', name), path.join(tempInput, name))
const revertedFrozen = run('node', ['scripts/verify-b14-r5-frozen-inputs.mjs'], { B14_R5_INPUT_DIR: tempInput })
fs.writeFileSync(path.join(frozenFolder, 'reverted.log'), revertedFrozen.output)
const frozenResult = { id: 'MUT-R5-06', file: frozenNames[2], introducedCommand: 'node scripts/verify-b14-r5-frozen-inputs.mjs [mutated copy]', introducedExit: introducedFrozen.exitCode, introducedExpectedFailure: introducedFrozen.exitCode !== 0, revertedCommand: 'node scripts/verify-b14-r5-frozen-inputs.mjs [restored copy]', revertedExit: revertedFrozen.exitCode, gitCleanAfter: true, status: introducedFrozen.exitCode !== 0 && revertedFrozen.exitCode === 0 ? 'PASS' : 'FAIL' }
fs.writeFileSync(path.join(frozenFolder, 'result.json'), `${JSON.stringify(frozenResult, null, 2)}\n`)
results.push(frozenResult)
const report = { schema: 'b14-r5-mutation-gates', sourceCommit: run('git', ['rev-parse', 'HEAD']).output.trim(), mutations: results, status: results.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL' }
fs.writeFileSync(path.join(evidenceDir, 'B14_R5_MUTATION_GATES.json'), `${JSON.stringify(report, null, 2)}\n`)
console.log(JSON.stringify(report, null, 2))
if (report.status !== 'PASS') process.exitCode = 1
