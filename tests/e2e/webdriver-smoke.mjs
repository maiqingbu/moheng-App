import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtemp, rm } from 'node:fs/promises'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { remote } from 'webdriverio'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const appBinary = path.join(root, 'src-tauri/target/debug/ai-novel-production-system')
assert.equal(process.platform, 'darwin', 'The WebDriver smoke test verifies the macOS WKWebView path')
const [nodeMajor, nodeMinor] = process.versions.node.split('.').map(Number)
assert.ok(nodeMajor > 22 || (nodeMajor === 22 && nodeMinor >= 19), 'WebdriverIO 10 requires Node.js >=22.19.0')

const port = await findFreePort()
const appDataDir = await mkdtemp(path.join(os.tmpdir(), 'moheng-e2e-webdriver-'))
const env = { ...process.env, MOHENG_REAL_APP_DATA_DIR: appDataDir, TAURI_WEBDRIVER_PORT: String(port) }

for (const key of Object.keys(env)) {
  if (/API_KEY|ACCESS_TOKEN|PROVIDER_SECRET/i.test(key)) delete env[key]
}

let appOutput = ''
let viteOutput = ''
let app
let vite
let browser

try {
  vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1450', '--strictPort'], {
    cwd: root,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  vite.stdout.on('data', chunk => { viteOutput += chunk.toString() })
  vite.stderr.on('data', chunk => { viteOutput += chunk.toString() })
  await waitForHttp('http://127.0.0.1:1450/', vite)

  app = spawn(appBinary, [], { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] })
  app.stdout.on('data', chunk => { appOutput += chunk.toString() })
  app.stderr.on('data', chunk => { appOutput += chunk.toString() })
  await waitForHttp(`http://127.0.0.1:${port}/status`, app)
  await waitForOutput(() => appOutput.includes('E2E WebDriver 已启用'), app)
  const listenerEvidence = await assertLoopbackListener(port)

  browser = await remote({
    hostname: '127.0.0.1',
    port,
    path: '/',
    capabilities: { browserName: 'tauri' },
    logLevel: 'warn',
    connectionRetryTimeout: 10_000,
    connectionRetryCount: 0,
  })

  await browser.waitUntil(async () => (await browser.$('h1').getText()).includes('作品库'), {
    timeout: 30_000,
    timeoutMsg: '作品库页面未加载',
  })
  const filterButton = await browser.$('//button[normalize-space(.)="生产中"]')
  await filterButton.click()
  await browser.waitUntil(async () => (await filterButton.getAttribute('class')).includes('active'), {
    timeout: 5_000,
    timeoutMsg: '点击后筛选按钮没有进入选中状态',
  })
  const pageText = await browser.$('main').getText()
  assert.match(pageText, /我的作品/)

  process.stdout.write(`PASS: WebDriver clicked “生产中” and read “我的作品”; ${listenerEvidence}\n`)
} catch (error) {
  process.stderr.write(`${error.stack ?? error}\n`)
  process.stderr.write(`--- app output ---\n${appOutput}\n--- vite output ---\n${viteOutput}\n`)
  process.exitCode = 1
} finally {
  if (browser) {
    try {
      await browser.deleteSession()
    } catch {
      // The app may already have exited after a failed startup.
    }
  }
  await stopProcess(app)
  await stopProcess(vite)
  await rm(appDataDir, { recursive: true, force: true })
}

function findFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const { port: freePort } = server.address()
      server.close(error => error ? reject(error) : resolve(freePort))
    })
  })
}

async function waitForHttp(url, child) {
  const deadline = Date.now() + 60_000
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Process exited before ${url} was ready (exit=${child.exitCode})`)
    try {
      const response = await fetch(url)
      if (response.ok) return
    } catch {
      // The service is still starting.
    }
    await new Promise(resolve => setTimeout(resolve, 200))
  }
  throw new Error(`Timed out waiting for ${url}`)
}

async function waitForOutput(readOutput, child) {
  const deadline = Date.now() + 10_000
  while (Date.now() < deadline) {
    if (readOutput()) return
    if (child.exitCode !== null) throw new Error(`Process exited before startup marker (exit=${child.exitCode})`)
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  throw new Error('Missing startup marker: E2E WebDriver 已启用')
}

function assertLoopbackListener(listenPort) {
  const result = spawn('lsof', ['-nP', `-iTCP:${listenPort}`, '-sTCP:LISTEN'])
  let output = ''
  result.stdout.on('data', chunk => { output += chunk.toString() })
  result.stderr.on('data', chunk => { output += chunk.toString() })
  return new Promise((resolve, reject) => {
    result.once('close', code => {
      const listeners = output.split('\n').filter(line => line.includes('(LISTEN)'))
      try {
        assert.equal(code, 0, `lsof failed: ${output}`)
        assert.ok(listeners.length > 0, `No WebDriver listener found on port ${listenPort}`)
        assert.ok(listeners.every(line => line.includes(`127.0.0.1:${listenPort}`)), `Listener is not loopback-only: ${listeners.join('\n')}`)
        resolve(listeners.join('\n'))
      } catch (error) {
        reject(error)
      }
    })
  })
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
