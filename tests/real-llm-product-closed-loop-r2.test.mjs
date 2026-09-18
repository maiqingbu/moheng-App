import test from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/**
 * Real-provider harness entry point.
 *
 * This is intentionally opt-in because it consumes the configured external
 * model budget. The Rust integration test opens the same desktop app-data
 * root, reads the endpoint/model from provider-settings.json, obtains the
 * secret from OsKeychainSecretStore, and creates three committed chapters.
 * No API key is accepted through an environment variable or written to logs.
 */
test('R2 real provider harness generates and reloads exactly three chapters', {
  timeout: 20 * 60 * 1000,
}, async (t) => {
  if (process.env.MOHENG_RUN_REAL_THREE_CHAPTERS !== '1') {
    t.skip('set MOHENG_RUN_REAL_THREE_CHAPTERS=1 to spend the configured API budget')
    return
  }
  const appDataRoot = process.env.MOHENG_REAL_APP_DATA_DIR
  assert.ok(appDataRoot, 'MOHENG_REAL_APP_DATA_DIR must point at the configured desktop app data root')
  const output = await new Promise((resolve, reject) => {
    const child = spawn(
      'cargo',
      ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'real_provider_three_chapters', '--', '--ignored', '--nocapture'],
      {
        cwd: root,
        env: {
          ...process.env,
          MOHENG_RUN_REAL_THREE_CHAPTERS: '1',
          MOHENG_RUN_REAL_CLOSED_LOOP: '0',
          MOHENG_REAL_APP_DATA_DIR: appDataRoot,
        },
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    )
    let text = ''
    child.stdout.on('data', chunk => { text += chunk.toString() })
    child.stderr.on('data', chunk => { text += chunk.toString() })
    child.on('error', reject)
    child.on('close', code => code === 0 ? resolve(text) : reject(new Error(`real provider harness exited ${code}\n${text}`)))
  })
  const result = String(output).match(/REAL_THREE_CHAPTER_RESULT\s+projectId=(\S+)\s+finalChapters=(\d+)\s+productionReceipts=(\d+)\s+completedModelRuns=(\d+).*status=PASS/)
  assert.ok(result, `missing REAL_THREE_CHAPTER_RESULT in harness output\n${output}`)
  assert.ok(Number(result[2]) >= 3, 'the authoritative DB must contain at least three committed chapters')
  assert.ok(Number(result[3]) >= 3, 'each chapter must have a durable ProductionReceipt')
  assert.ok(Number(result[4]) >= 9, 'three chapters require PLAN/RENDER/AUDIT model runs')
  assert.match(result[1], /^prj:/)
})
