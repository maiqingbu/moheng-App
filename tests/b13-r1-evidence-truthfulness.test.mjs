import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { evaluateEvidenceTruthfulness } from '../scripts/verify-b13-evidence-truthfulness.mjs'

const png = Buffer.from('89504e470d0a1a0a', 'hex')
const commit = 'a'.repeat(40)

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'b13-r1-truthfulness-'))
  fs.mkdirSync(path.join(root, 'MANUAL_BROWSER'), { recursive: true })
  fs.mkdirSync(path.join(root, 'MUTATIONS'), { recursive: true })
  fs.mkdirSync(path.join(root, 'PERFORMANCE'), { recursive: true })
  for (let index = 1; index <= 12; index += 1) {
    const screenshot = `MANUAL_BROWSER/M${String(index).padStart(2, '0')}.png`
    fs.writeFileSync(path.join(root, screenshot), png)
  }
  fs.writeFileSync(path.join(root, 'B13_R1_MANUAL_WALKTHROUGH.json'), JSON.stringify({
    sourceCommit: commit,
    items: Array.from({ length: 12 }, (_, index) => ({
      id: `M${String(index + 1).padStart(2, '0')}`,
      route: '/projects/proj-moheng-demo/health',
      fixture: 'browser-demo fixture',
      trigger: 'open health page',
      expectedState: 'visible state',
      actualVisibleText: `browser captured text ${index + 1}`,
      actualVisibleTextSource: 'playwright-dom-innerText',
      domTextSha256: 'b'.repeat(64),
      internalLeakageCheck: true,
      b14LeakageCheck: true,
      screenshotPath: `MANUAL_BROWSER/M${String(index + 1).padStart(2, '0')}.png`,
      result: 'PASS',
    })),
    status: 'PASS',
  }))
  for (const id of ['MUT-R1-01', 'MUT-R1-02', 'MUT-R1-03', 'MUT-R1-04']) {
    const directory = path.join(root, 'MUTATIONS', id)
    fs.mkdirSync(directory, { recursive: true })
    fs.writeFileSync(path.join(directory, 'patch.diff'), `diff --git a/${id} b/${id}\n`)
    fs.writeFileSync(path.join(directory, 'introduced.log'), 'introduced failure\n')
    fs.writeFileSync(path.join(directory, 'reverted.log'), 'reverted pass\n')
    fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify({
      sourceCommit: commit,
      patchApplied: true,
      introducedExitCode: 1,
      revertedExitCode: 0,
      introducedStatus: 'FAIL',
      revertedStatus: 'PASS',
      reverted: true,
      status: 'PASS',
    }))
  }
  for (const name of ['H01', 'H02', 'H03', 'H04', 'H07', 'ISSUES']) {
    fs.writeFileSync(path.join(root, 'PERFORMANCE', `${name}.explain.txt`), `EXPLAIN QUERY PLAN ${name}: SEARCH fixture\n`)
  }
  fs.writeFileSync(path.join(root, 'B13_R1_NO_REMOTE_CALL.json'), JSON.stringify({ sourceCommit: commit, modelRunsBefore: 0, modelRunsAfter: 0, transportCallsBefore: 0, transportCallsAfter: 0, status: 'PASS', logPath: 'AUTOMATIC/no-remote.log' }))
  fs.writeFileSync(path.join(root, 'B13_R1_NO_AUTHORITY_MUTATION.json'), JSON.stringify({ sourceCommit: commit, before: { canonFacts: 0 }, after: { canonFacts: 0 }, unchanged: true, status: 'PASS', logPath: 'AUTOMATIC/no-authority.log' }))
  fs.mkdirSync(path.join(root, 'AUTOMATIC'), { recursive: true })
  fs.writeFileSync(path.join(root, 'AUTOMATIC/no-remote.log'), 'before=0 after=0\n')
  fs.writeFileSync(path.join(root, 'AUTOMATIC/no-authority.log'), 'unchanged=true\n')
  fs.writeFileSync(path.join(root, 'B13_R1_SCALE.json'), JSON.stringify({ sourceCommit: commit, words: 10000000, chapters: 4000, obligations: 100, routeScopes: 102, tasks: 202, readerDesires: 52, contextRuns: 16, costRuns: 16, qualitySamples: 20, status: 'PASS' }))
  fs.writeFileSync(path.join(root, 'B13_R1_STALE_CACHE.json'), JSON.stringify({ sourceCommit: commit, cacheMode: 'ON_DEMAND', staleCacheTest: 'NOT_APPLICABLE', reason: 'formal health truth does not read persisted cache', status: 'PASS' }))
  fs.writeFileSync(path.join(root, 'B13_R1_SOURCE_BINDING.json'), JSON.stringify({ sourceCommit: commit, evidenceSourceCommit: commit, sourceCommitAfter: commit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [], status: 'PASS' }))
  return root
}

test('B13 R1 evidence truthfulness fails when a required browser screenshot is missing', () => {
  const root = fixture()
  fs.unlinkSync(path.join(root, 'MANUAL_BROWSER/M05.png'))
  const result = evaluateEvidenceTruthfulness(root, commit)
  assert.equal(result.status, 'FAIL')
  assert.match(result.failures.join('\n'), /M05\.png/)
})

test('B13 R1 evidence truthfulness rejects a fabricated stale-cache claim', () => {
  const root = fixture()
  fs.writeFileSync(path.join(root, 'B13_R1_STALE_CACHE.json'), JSON.stringify({
    sourceCommit: commit,
    cacheMode: 'PROJECTION_ONLY',
    staleSourceRevisionRejected: true,
    status: 'PASS',
  }))
  const result = evaluateEvidenceTruthfulness(root, commit)
  assert.equal(result.status, 'FAIL')
  assert.match(result.failures.join('\n'), /stale cache/)
})
