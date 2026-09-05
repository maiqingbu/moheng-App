import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R2_EVIDENCE_DIR ?? '')
const mutationRoot = path.join(evidenceDir, 'MUTATIONS')
const repairBaselineShort = '3a291f4'
const sourceCommit = process.env.B13_R2_SOURCE_COMMIT ?? exec('git', ['rev-parse', 'HEAD']).stdout.trim()

function exec(command, args) {
  return spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? '/Users/mac/Library/Caches/moheng-product-ux-cargo-target' },
    maxBuffer: 256 * 1024 * 1024,
  })
}

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex')
}

function runTest(args) {
  const result = exec('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', ...args, '--', '--nocapture'])
  return { exitCode: result.status ?? 1, output: `${result.stdout ?? ''}${result.stderr ?? ''}` }
}

function writeMutation(id, targetRelative, original, mutated, testArgs, description) {
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const target = path.join(root, targetRelative)
  const originalPath = path.join(os.tmpdir(), `b13-r2-${id}-original-${process.pid}`)
  const mutatedPath = path.join(os.tmpdir(), `b13-r2-${id}-mutated-${process.pid}`)
  fs.writeFileSync(originalPath, original)
  fs.writeFileSync(mutatedPath, mutated)
  const diff = exec('diff', ['-u', originalPath, mutatedPath])
  fs.writeFileSync(path.join(directory, 'patch.diff'), diff.stdout || `${id}: mutation changed ${targetRelative}\n`)
  let introduced
  let reverted
  let restored = false
  try {
    fs.writeFileSync(target, mutated)
    introduced = runTest(testArgs)
    fs.writeFileSync(path.join(directory, 'introduced.log'), introduced.output)
  } finally {
    fs.writeFileSync(target, original)
    restored = true
  }
  reverted = runTest(testArgs)
  fs.writeFileSync(path.join(directory, 'reverted.log'), reverted.output)
  const result = {
    schema: 'b13-r2-mutation-result',
    id,
    sourceCommit,
    target: targetRelative,
    description,
    patchApplied: true,
    introducedExitCode: introduced.exitCode,
    introducedStatus: introduced.exitCode === 0 ? 'PASS' : 'FAIL',
    revertedExitCode: reverted.exitCode,
    revertedStatus: reverted.exitCode === 0 ? 'PASS' : 'FAIL',
    reverted: restored,
    originalSha256: sha256(original),
    restoredSha256: sha256(fs.readFileSync(target)),
    status: introduced.exitCode !== 0 && reverted.exitCode === 0 && restored ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(directory, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
  return result
}

function writeForbiddenScopeMutation() {
  const id = 'MUT-R2-03'
  const targetRelative = 'src-tauri/src/health/mod.rs'
  const directory = path.join(mutationRoot, id)
  fs.mkdirSync(directory, { recursive: true })
  const target = path.join(root, targetRelative)
  const original = fs.readFileSync(target)
  const mutated = Buffer.concat([original, Buffer.from('\nstruct CompletionGate;\n', 'utf8')])
  fs.writeFileSync(path.join(directory, 'patch.diff'), `diff --git a/${targetRelative} b/${targetRelative}\n+struct CompletionGate;\n`)
  let introduced
  let reverted
  try {
    fs.writeFileSync(target, mutated)
    introduced = exec('node', ['scripts/verify-b13-r2-forbidden-scope.mjs', repairBaselineShort])
    fs.writeFileSync(path.join(directory, 'introduced.log'), `${introduced.stdout ?? ''}${introduced.stderr ?? ''}`)
  } finally {
    fs.writeFileSync(target, original)
  }
  reverted = exec('node', ['scripts/verify-b13-r2-forbidden-scope.mjs', repairBaselineShort])
  fs.writeFileSync(path.join(directory, 'reverted.log'), `${reverted.stdout ?? ''}${reverted.stderr ?? ''}`)
  const result = {
    schema: 'b13-r2-mutation-result', id, sourceCommit, target: targetRelative,
    description: 'add a B14 CompletionGate marker to formal source; forbidden-scope gate must fail',
    patchApplied: true, introducedExitCode: introduced.status ?? 1, introducedStatus: introduced.status === 0 ? 'PASS' : 'FAIL',
    revertedExitCode: reverted.status ?? 1, revertedStatus: reverted.status === 0 ? 'PASS' : 'FAIL', reverted: true,
    status: introduced.status !== 0 && reverted.status === 0 ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(directory, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
  return result
}

function main() {
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R2_MUTATION_EVIDENCE_DIR_REQUIRED')
  if (exec('git', ['status', '--porcelain', '--untracked-files=all']).stdout.trim()) throw new Error('B13_R2_MUTATION_WORKTREE_MUST_START_CLEAN')
  fs.mkdirSync(mutationRoot, { recursive: true })
  const healthPath = 'src-tauri/src/repositories/health.rs'
  const healthFile = path.join(root, healthPath)
  const healthOriginal = fs.readFileSync(healthFile)
  const healthText = healthOriginal.toString('utf8')
  const healthMarker = 'let all_routes = formal_route_rows(connection, project_id)?;'
  if (!healthText.includes(healthMarker)) throw new Error('B13_R2_MUTATION_FORMAL_ROUTE_MARKER_MISSING')
  const healthMutated = Buffer.from(healthText.replace(healthMarker, 'let all_routes = Vec::new();'), 'utf8')
  const formalMutation = writeMutation(
    'MUT-R2-01',
    healthPath,
    healthOriginal,
    healthMutated,
    ['--test', 'b13_r2_integration'],
    'disconnect H03/H04 from the formal B07/B09 adapter; integration must fail closed',
  )

  const queryPath = 'src-tauri/src/commands.rs'
  const queryFile = path.join(root, queryPath)
  const queryOriginal = fs.readFileSync(queryFile)
  const queryText = queryOriginal.toString('utf8')
  const queryMarker = 'pub fn query_book_health_impl(state: &KernelState, request: Value) -> Result<Value, String> {\n    let project_id = required_string(&request, "projectId")?;\n    let cursor = request.get("cursor").and_then(Value::as_str);'
  if (!queryText.includes(queryMarker)) throw new Error('B13_R2_MUTATION_AUTHORITY_MARKER_MISSING')
  const queryMutated = Buffer.from(queryText.replace(
    queryMarker,
    `${queryMarker}\n    let mutation_handle = state.manager.open(project_id, "", "1970-01-01T00:00:00Z").map_err(|error| error.to_string())?;\n    mutation_handle.execute("UPDATE narrative_obligations SET description=description || ' [MUT-R2-02]' WHERE project_id=?1", [project_id]).map_err(|error| error.to_string())?;\n    drop(mutation_handle);`,
  ), 'utf8')
  const authorityMutation = writeMutation(
    'MUT-R2-02',
    queryPath,
    queryOriginal,
    queryMutated,
    ['--test', 'b13_r2_authority'],
    'inject an authority-table UPDATE into the formal query command path; count/digest gate must fail',
  )
  const forbiddenScopeMutation = writeForbiddenScopeMutation()

  const status = [formalMutation, authorityMutation, forbiddenScopeMutation].every(item => item.status === 'PASS') ? 'PASS' : 'FAIL'
  const result = { schema: 'b13-r2-mutation-gates', sourceCommit, mutations: [formalMutation, authorityMutation, forbiddenScopeMutation], status }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R2_MUTATION_GATES.json'), `${JSON.stringify(result, null, 2)}\n`)
  console.log(JSON.stringify(result, null, 2))
  if (status !== 'PASS') process.exitCode = 1
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
