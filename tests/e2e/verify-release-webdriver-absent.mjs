import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

assert.equal(process.platform, 'darwin', 'The release isolation check is intended for macOS')

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const appBinary = path.join(root, 'src-tauri/target/release/ai-novel-production-system')
const strings = spawnSync('strings', ['-a', appBinary], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 })
assert.equal(strings.status, 0, strings.stderr || 'Could not inspect the release binary')

for (const marker of [
  'tauri-plugin-wdio-webdriver',
  'tauri_plugin_wdio_webdriver',
  'TAURI_WEBDRIVER_PORT',
  'E2E WebDriver 已启用',
  '127.0.0.1:4445',
]) {
  assert.ok(!strings.stdout.includes(marker), `Release binary contains WebDriver marker: ${marker}`)
}

const portBefore = listeningPort(4445)
assert.equal(portBefore, '', `Port 4445 was already occupied before launch: ${portBefore}`)

const appDataDir = await mkdtemp(path.join(os.tmpdir(), 'moheng-release-e2e-check-'))
const env = { ...process.env, MOHENG_REAL_APP_DATA_DIR: appDataDir }
for (const key of Object.keys(env)) {
  if (/API_KEY|ACCESS_TOKEN|PROVIDER_SECRET/i.test(key)) delete env[key]
}

let app
let appOutput = ''
try {
  app = spawn(appBinary, [], { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] })
  app.stdout.on('data', chunk => { appOutput += chunk.toString() })
  app.stderr.on('data', chunk => { appOutput += chunk.toString() })
  await new Promise(resolve => setTimeout(resolve, 3_000))
  assert.equal(app.exitCode, null, `Release app exited before the port check: ${appOutput}`)

  const portAfter = listeningPort(4445)
  assert.equal(portAfter, '', `Port 4445 is listening while the release app is running: ${portAfter}`)
  process.stdout.write('PASS: release binary has no WebDriver markers and port 4445 remains unlistened while the app runs\n')
} finally {
  await stopProcess(app)
  await rm(appDataDir, { recursive: true, force: true })
}

function listeningPort(port) {
  const result = spawnSync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN'], { encoding: 'utf8' })
  assert.ok([0, 1].includes(result.status), result.stderr || 'lsof failed')
  return result.stdout.split('\n').filter(line => line.includes('(LISTEN)')).join('\n')
}

async function stopProcess(child) {
  if (!child || child.exitCode !== null) return
  child.kill('SIGTERM')
  await Promise.race([
    new Promise(resolve => child.once('exit', resolve)),
    new Promise(resolve => setTimeout(resolve, 5_000)),
  ])
  if (child.exitCode === null) child.kill('SIGKILL')
}
