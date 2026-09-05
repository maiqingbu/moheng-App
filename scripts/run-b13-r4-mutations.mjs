import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R4_EVIDENCE_DIR || '')
const mutationRoot = path.join(evidenceDir, 'B13_R4', 'MUTATIONS')
const sourceCommit = process.env.B13_R4_SOURCE_COMMIT || git(['rev-parse', 'HEAD'])
const repairBaselineCommit = process.env.B13_R4_REPAIR_BASELINE_COMMIT || git(['rev-parse', 'a479b28^{commit}'])

function git(args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  if (result.status !== 0) throw new Error('R4_GIT_FAILED:' + (result.stderr || ''))
  return result.stdout.trim()
}

function run(command, args, env = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, ...env },
    maxBuffer: 128 * 1024 * 1024,
  })
  return { exitCode: result.status === null ? 1 : result.status, output: (result.stdout || '') + (result.stderr || '') }
}

function sha256(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex') }

function writeResult(directory, id, target, description, introduced, reverted, original, restoredBytes) {
  const result = {
    schema: 'b13-r4-mutation-result',
    id,
    sourceCommit,
    repairBaselineCommit,
    target,
    description,
    patchApplied: true,
    introducedExitCode: introduced.exitCode,
    introducedStatus: introduced.exitCode === 0 ? 'PASS' : 'FAIL',
    revertedExitCode: reverted.exitCode,
    revertedStatus: reverted.exitCode === 0 ? 'PASS' : 'FAIL',
    reverted: true,
    originalSha256: sha256(original),
    restoredSha256: sha256(restoredBytes),
    status: introduced.exitCode !== 0 && reverted.exitCode === 0 && Buffer.compare(original, restoredBytes) === 0 ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify(result, null, 2) + '\n')
  return result
}

function runPathScan(sourceDir) {
  return run(process.execPath, ['scripts/verify-b13-r4-paths.mjs', '--source', sourceDir, '--baseline', repairBaselineCommit])
}

function sourceMutation(id, filename, description, mutate) {
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r4-mutation-'))
  const skillRoot = path.join(tempRoot, 'creative-skills', 'v1.2.0')
  fs.cpSync(path.join(root, 'creative-skills', 'v1.2.0'), skillRoot, { recursive: true })
  const original = fs.readFileSync(path.join(skillRoot, 'sources', 'articles', filename))
  fs.writeFileSync(path.join(directory, 'introduced-path.txt'), filename + '\n')
  mutate(skillRoot)
  const introduced = runPathScan(skillRoot)
  fs.writeFileSync(path.join(directory, 'introduced.log'), introduced.output)
  fs.rmSync(tempRoot, { recursive: true, force: true })
  fs.cpSync(path.join(root, 'creative-skills', 'v1.2.0'), skillRoot, { recursive: true })
  const reverted = runPathScan(skillRoot)
  fs.writeFileSync(path.join(directory, 'reverted.log'), reverted.output)
  const restored = fs.readFileSync(path.join(skillRoot, 'sources', 'articles', filename))
  fs.rmSync(tempRoot, { recursive: true, force: true })
  return writeResult(directory, id, 'creative-skills/v1.2.0/sources/articles/' + filename, description, introduced, reverted, original, restored)
}

function runTruthfulness() {
  return run(process.execPath, ['scripts/verify-b13-r4-evidence-truthfulness.mjs'], {
    B13_R4_EVIDENCE_DIR: evidenceDir,
    B13_R4_SOURCE_COMMIT: sourceCommit,
    B13_R4_REPAIR_BASELINE_COMMIT: repairBaselineCommit,
    B13_R4_REQUIRE_MUTATIONS: 'false',
  })
}

function evidenceMutation() {
  const id = 'MUT-R4-EVIDENCE-TRUTHFULNESS'
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const relative = 'B13_R4/PATH_VALIDATION/final-zip-path-scan.log'
  const target = path.join(evidenceDir, relative)
  const original = fs.readFileSync(target)
  fs.writeFileSync(path.join(directory, 'patch.diff'), 'append a forged CP437 finding to the final raw scan log\n')
  let introduced
  try {
    fs.appendFileSync(target, 'CP437_MOJIBAKE_CANDIDATE path=forged\n')
    introduced = runTruthfulness()
    fs.writeFileSync(path.join(directory, 'introduced.log'), introduced.output)
  } finally {
    fs.writeFileSync(target, original)
  }
  const reverted = runTruthfulness()
  fs.writeFileSync(path.join(directory, 'reverted.log'), reverted.output)
  return writeResult(directory, id, relative, 'forge a raw CP437 finding while summary remains zero; truthfulness must fail', introduced, reverted, original, fs.readFileSync(target))
}

function main() {
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R4_MUTATION_EVIDENCE_DIR_REQUIRED')
  fs.mkdirSync(mutationRoot, { recursive: true })
  const articleDirectory = path.join(root, 'creative-skills', 'v1.2.0', 'sources', 'articles')
  const oldArticlePath = git(['-c', 'core.quotePath=false', 'ls-tree', '-r', '--name-only', repairBaselineCommit, '--', 'creative-skills/v1.2.0/sources/articles'])
    .split('\n').find(file => /\/01-.*\.md$/.test(file))
  if (!oldArticlePath) throw new Error('B13_R4_OLD_ARTICLE_PATH_NOT_FOUND')
  const oldFilename = path.basename(oldArticlePath)
  const results = []
  results.push(sourceMutation(
    'MUT-R4-01',
    '01-世界观深化技巧.md',
    'restore one article filename to its frozen CP437-mojibake spelling; path Gate must fail',
    skillRoot => {
      fs.renameSync(path.join(skillRoot, 'sources', 'articles', '01-世界观深化技巧.md'), path.join(skillRoot, 'sources', 'articles', oldFilename))
    },
  ))
  results.push(sourceMutation(
    'MUT-R4-02',
    '16-节奏控制.md',
    'rename one article to a legal but non-frozen Chinese path; exact frozen path Gate must fail',
    skillRoot => {
      const correct = path.join(skillRoot, 'sources', 'articles', '16-节奏控制.md')
      const wrong = path.join(skillRoot, 'sources', 'articles', '16-节奏控制技巧.md')
      if (fs.existsSync(correct)) fs.renameSync(correct, wrong)
      else fs.renameSync(wrong, correct)
    },
  ))
  results.push(evidenceMutation())
  const report = {
    schema: 'b13-r4-mutation-gates',
    sourceCommit,
    repairBaselineCommit,
    mutations: results,
    status: results.length === 3 && results.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R4_MUTATION_GATES.json'), JSON.stringify(report, null, 2) + '\n')
  console.log(JSON.stringify(report, null, 2))
  if (report.status !== 'PASS') process.exitCode = 1
}

try { main() } catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
