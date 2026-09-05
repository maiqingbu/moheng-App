import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R1_EVIDENCE_DIR ?? '')
const sourceCommit = process.env.B13_R1_SOURCE_COMMIT
const cargoTarget = process.env.CARGO_TARGET_DIR ?? '/Users/mac/Library/Caches/moheng-product-ux-cargo-target'

const mutations = [
  {
    id: 'MUT-R1-01',
    test: 'b13_r1_opaque_chapter_references_are_resolved_by_chapter_row',
    patch: `diff --git a/src-tauri/src/health/mod.rs b/src-tauri/src/health/mod.rs
--- a/src-tauri/src/health/mod.rs
+++ b/src-tauri/src/health/mod.rs
@@ -376,3 +376,8 @@
-        let Some(due) = obligation.due_chapter_no else {
+        let Some(due) = obligation
+            .deadline
+            .as_ref()
+            .and_then(|value| value.strip_prefix("chapter:"))
+            .and_then(|value| value.parse::<i64>().ok())
+        else {
             continue;
         };
`,
  },
  {
    id: 'MUT-R1-02',
    test: 'b13_r1_revision_only_route_change_does_not_become_route_drift',
    patch: `diff --git a/src-tauri/src/health/mod.rs b/src-tauri/src/health/mod.rs
--- a/src-tauri/src/health/mod.rs
+++ b/src-tauri/src/health/mod.rs
@@ -339,3 +339,3 @@
-    if route.formal_stale {
+    if route.formal_stale || route.generated_revision < snapshot.project_revision {
         return true;
     }
`,
  },
  {
    id: 'MUT-R1-03',
    test: 'b13_r1_hidden_a_obligation_and_reader_desire_change_global_truth_only',
    patch: `diff --git a/src-tauri/src/health/mod.rs b/src-tauri/src/health/mod.rs
--- a/src-tauri/src/health/mod.rs
+++ b/src-tauri/src/health/mod.rs
@@ -404,2 +404,2 @@
-    let severity = if snapshot.overdue_obligations_authority_ab > 0 {
+    let severity = if issues.iter().any(|issue| issue.severity == "risk") {
         "risk"
`,
  },
  {
    id: 'MUT-R1-04',
    test: 'b13_r1_task_truth_covers_required_blockage_beyond_detail_page',
    replacement: {
      relative: 'src-tauri/src/health/mod.rs',
      oldText: '    let title = title.map(str::trim).filter(|value| !value.is_empty());',
      newText: '    let title = Some("volume:mutation");',
    },
    patch: `diff --git a/src-tauri/src/health/mod.rs b/src-tauri/src/health/mod.rs
--- a/src-tauri/src/health/mod.rs
+++ b/src-tauri/src/health/mod.rs
@@ -332,4 +332,4 @@
-        || format!("第 {volume_no} 卷"),
-        |title| format!("第 {volume_no} 卷 · {title}"),
+        || format!("volume:{volume_no}"),
+        |title| format!("volume:{volume_no}:{title}"),
`,
  },
]

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: cargoTarget },
    maxBuffer: 128 * 1024 * 1024,
  })
  return {
    command: [command, ...args].join(' '),
    exitCode: result.status ?? 1,
    output: `${result.stdout ?? ''}${result.stderr ?? ''}`,
  }
}

function makeReplacementPatch(worktree, replacement) {
  const sourcePath = path.join(worktree, replacement.relative)
  const sourceText = fs.readFileSync(sourcePath, 'utf8')
  if (sourceText.split(replacement.oldText).length - 1 !== 1) {
    throw new Error(`B13_R1_MUTATION_REPLACEMENT_NOT_UNIQUE:${replacement.relative}`)
  }
  const mutatedPath = path.join(os.tmpdir(), `moheng-b13-r1-mutated-${Date.now()}-${Math.random().toString(16).slice(2)}.tmp`)
  fs.writeFileSync(mutatedPath, sourceText.replace(replacement.oldText, replacement.newText))
  const diff = run('git', ['diff', '--no-index', '--unified=3', sourcePath, mutatedPath], root)
  fs.unlinkSync(mutatedPath)
  if (diff.exitCode !== 1) throw new Error(`B13_R1_MUTATION_DIFF_FAILED:${diff.output}`)
  const lines = diff.output.split('\n')
  lines[0] = `diff --git a/${replacement.relative} b/${replacement.relative}`
  const oldHeader = lines.findIndex(line => line.startsWith('--- '))
  const newHeader = lines.findIndex(line => line.startsWith('+++ '))
  if (oldHeader < 0 || newHeader < 0) throw new Error(`B13_R1_MUTATION_DIFF_HEADERS_MISSING:${replacement.relative}`)
  lines[oldHeader] = `--- a/${replacement.relative}`
  lines[newHeader] = `+++ b/${replacement.relative}`
  return lines.join('\n')
}

function writeResult(directory, result) {
  fs.writeFileSync(path.join(directory, 'result.json'), `${JSON.stringify(result, null, 2)}\n`)
  fs.writeFileSync(path.join(directory, 'introduced.log'), result.introducedOutput)
  fs.writeFileSync(path.join(directory, 'reverted.log'), result.revertedOutput)
}

function main() {
  if (!evidenceDir || !sourceCommit) throw new Error('B13_R1_MUTATION_INPUT_REQUIRED')
  fs.mkdirSync(path.join(evidenceDir, 'MUTATIONS'), { recursive: true })
  const worktree = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r1-mutation-worktree-'))
  const added = run('git', ['worktree', 'add', '--detach', worktree, sourceCommit], root)
  if (added.exitCode !== 0) throw new Error(`B13_R1_MUTATION_WORKTREE_ADD_FAILED:${added.output}`)
  const results = []
  try {
    for (const mutation of mutations) {
      const directory = path.join(evidenceDir, 'MUTATIONS', mutation.id)
      fs.mkdirSync(directory, { recursive: true })
      const patchText = mutation.replacement
        ? makeReplacementPatch(worktree, mutation.replacement)
        : mutation.patch
      fs.writeFileSync(path.join(directory, 'patch.diff'), patchText)
      const patchPath = path.join(directory, 'patch.diff')
      const applied = run('git', ['apply', patchPath], worktree)
      const introduced = applied.exitCode === 0
        ? run('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'b13_health', mutation.test, '--', '--nocapture'], worktree)
        : { command: 'not run: patch application failed', exitCode: 1, output: applied.output }
      const reverted = applied.exitCode === 0
        ? run('git', ['apply', '--reverse', patchPath], worktree)
        : { command: 'not run: patch application failed', exitCode: 1, output: applied.output }
      const clean = run('git', ['status', '--porcelain'], worktree)
      const result = {
        schema: 'b13-r1-mutation-result',
        id: mutation.id,
        test: mutation.test,
        sourceCommit,
        patchApplied: applied.exitCode === 0,
        patchApplyOutput: applied.output,
        introducedCommand: introduced.command,
        introducedExitCode: introduced.exitCode,
        introducedStatus: introduced.exitCode === 0 ? 'PASS' : 'FAIL',
        revertedCommand: reverted.command,
        revertedExitCode: reverted.exitCode,
        revertedStatus: reverted.exitCode === 0 ? 'PASS' : 'FAIL',
        worktreeCleanAfterRevert: clean.exitCode === 0 && clean.output.trim() === '',
        reverted: reverted.exitCode === 0 && clean.output.trim() === '',
        status: applied.exitCode === 0
          && introduced.exitCode !== 0
          && reverted.exitCode === 0
          && clean.output.trim() === ''
          ? 'PASS'
          : 'FAIL',
        introducedOutput: introduced.output,
        revertedOutput: reverted.output,
      }
      writeResult(directory, result)
      results.push(result)
    }
  } finally {
    run('git', ['worktree', 'remove', '--force', worktree], root)
  }
  const status = results.every(result => result.status === 'PASS') ? 'PASS' : 'FAIL'
  console.log(JSON.stringify({ schema: 'b13-r1-mutation-run-r1', sourceCommit, results, status }, null, 2))
  if (status !== 'PASS') process.exitCode = 1
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
