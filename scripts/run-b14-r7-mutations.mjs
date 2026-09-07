import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R7_EVIDENCE_DIR ?? '')
if (!evidenceDir) throw new Error('B14_R7_EVIDENCE_DIR_REQUIRED')

const mutationDir = path.join(evidenceDir, 'B14_R7', 'MUTATIONS', 'MUT-R7-01')
fs.mkdirSync(mutationDir, { recursive: true })
const target = path.join(root, 'src-tauri/src/repositories/narrative.rs')
const original = fs.readFileSync(target, 'utf8')
const originalFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'b14-r7-original-')), 'narrative.rs')
fs.writeFileSync(originalFile, original)
const targetBlock = `            offset += CONTINUITY_SOURCE_CHUNK_SIZE;\n        }\n        Ok(result)\n    }\n\n    pub fn has_more_foreshadows`
const truncatedBlock = `            break;\n        }\n        Ok(result)\n    }\n\n    pub fn has_more_foreshadows`
if (original.split(targetBlock).length !== 2) throw new Error('R7_MUTATION_TARGET_NOT_UNIQUE')
const mutated = original.replace(targetBlock, truncatedBlock)
const mutatedFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'b14-r7-mutated-')), 'narrative.rs')
fs.writeFileSync(mutatedFile, mutated)
const patch = spawnSync('diff', ['-u', originalFile, mutatedFile], { cwd: root, encoding: 'utf8' })
const patchText = patch.stdout.replaceAll(originalFile, 'a/src-tauri/src/repositories/narrative.rs').replaceAll(mutatedFile, 'b/src-tauri/src/repositories/narrative.rs')
fs.writeFileSync(path.join(mutationDir, 'patch.diff'), patchText)
const patchSha256 = crypto.createHash('sha256').update(patchText).digest('hex')
const command = ['cargo', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b14_endgame_integration', 'b14_r7_paged_sources_enter_continuity_and_change_digest', '--', '--nocapture']
const run = () => spawnSync(command[0], command.slice(1), { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })
let introduced
let reverted
try {
  fs.writeFileSync(target, mutated)
  introduced = run()
  fs.writeFileSync(path.join(mutationDir, 'introduced.log'), `${introduced.stdout ?? ''}${introduced.stderr ?? ''}`)
} finally {
  fs.writeFileSync(target, original)
}
reverted = run()
fs.writeFileSync(path.join(mutationDir, 'reverted.log'), `${reverted.stdout ?? ''}${reverted.stderr ?? ''}`)
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout.trim()
const failureClass = introduced.stdout?.includes('PAGED_SOURCE_101_MISSING_FROM_CONTINUITY') || introduced.stderr?.includes('PAGED_SOURCE_101_MISSING_FROM_CONTINUITY')
  ? 'PAGED_SOURCE_101_MISSING_FROM_CONTINUITY'
  : 'UNEXPECTED_MUTATION_FAILURE'
const productionDiffAfterRevert = git(['diff', '--', 'src-tauri/src']) === ''
const workingTreeCleanAfter = git(['status', '--porcelain', '--untracked-files=all']) === ''
const result = {
  mutationId: 'MUT-R7-01',
  purpose: 'truncate active_foreshadows_complete to its first 100-row page',
  targetReader: 'NarrativeRepository::active_foreshadows_complete',
  patchSha256,
  introducedCommand: command.join(' '),
  introducedExitCode: introduced.status ?? 1,
  introducedFailureClass: failureClass,
  introducedFailureMatchedExpected: introduced.status !== 0 && failureClass === 'PAGED_SOURCE_101_MISSING_FROM_CONTINUITY',
  revertedCommand: command.join(' '),
  revertedExitCode: reverted.status ?? 1,
  productionDiffAfterRevert,
  workingTreeCleanAfter,
  status: introduced.status !== 0 && failureClass === 'PAGED_SOURCE_101_MISSING_FROM_CONTINUITY' && reverted.status === 0 && productionDiffAfterRevert && workingTreeCleanAfter ? 'PASS' : 'FAIL'
}
fs.writeFileSync(path.join(mutationDir, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
const gates = { schema: 'b14-r7-mutation-gates', mutations: [result], status: result.status }
fs.writeFileSync(path.join(evidenceDir, 'B14_R7_MUTATION_GATES.json'), `${JSON.stringify(gates, null, 2)}\n`)
console.log(JSON.stringify(gates, null, 2))
if (result.status !== 'PASS') process.exitCode = 1
