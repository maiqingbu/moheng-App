import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { remote } from 'webdriverio'

assert.equal(process.platform, 'darwin', 'The settings smoke test uses the macOS WKWebView')

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const appBinary = path.join(root, 'src-tauri/target/release/ai-novel-production-system')
const port = await findFreePort()
const appDataDir = await mkdtemp(path.join(os.tmpdir(), 'moheng-c1-settings-release-'))
const env = { ...process.env, MOHENG_REAL_APP_DATA_DIR: appDataDir, TAURI_WEBDRIVER_PORT: String(port) }
for (const key of Object.keys(env)) {
  if (/API_KEY|ACCESS_TOKEN|PROVIDER_SECRET/i.test(key)) delete env[key]
}

let app
let appOutput = ''
let appExit = null
let browser

try {
  app = spawn(appBinary, [], { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] })
  app.once('exit', (code, signal) => { appExit = { code, signal } })
  app.stdout.on('data', chunk => { appOutput += chunk.toString() })
  app.stderr.on('data', chunk => { appOutput += chunk.toString() })
  await waitForHttp(`http://127.0.0.1:${port}/status`, app)
  await waitForOutput(() => appOutput.includes('E2E WebDriver 已启用'), app)

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
    timeoutMsg: '打包前端的作品库页面未加载',
  })
  await browser.$('button[aria-label="作品设置"]').click()
  await browser.waitUntil(async () => (await browser.$('h1').getText()).includes('应用设置'), {
    timeout: 10_000,
    timeoutMsg: '应用设置页未响应或未加载',
  })

  await browser.waitUntil(async () => (await browser.$('#setting-cost h3.card-title').getText()).includes('模型价格表'), {
    timeout: 10_000,
    timeoutMsg: '设置查询没有返回模型价格表',
  })
  await browser.$('//section[@id="setting-cost"]//button[normalize-space(.)="添加模型"]').click()
  const priceInputs = await browser.$$('#setting-cost .pricing-entry input')
  assert.equal(priceInputs.length, 5, 'The settings page should render all five price fields')
  for (const [index, value] of ['c1-test', 'c1-test-model', '1', '2', '0.5'].entries()) {
    await priceInputs[index].setValue(value)
  }
  await browser.$('//section[@id="setting-cost"]//button[normalize-space(.)="保存价格表新版本"]').click()
  await new Promise(resolve => setTimeout(resolve, 1_000))
  try {
    await browser.deleteSession()
  } catch {
    // The packaged WebView may recreate its automation session after a settings write.
  }
  browser = await remote({
    hostname: '127.0.0.1',
    port,
    path: '/',
    capabilities: { browserName: 'tauri' },
    logLevel: 'warn',
    connectionRetryTimeout: 10_000,
    connectionRetryCount: 0,
  })
  await browser.waitUntil(async () => (await browser.$('h1').getText()).includes('应用设置'), {
    timeout: 10_000,
    timeoutMsg: '保存后 WebDriver 未能重新连接到应用设置页',
  })
  await browser.waitUntil(async () => {
    const heading = await browser.$('#setting-cost h3.card-title').getText()
    const savedInputs = await browser.$$('#setting-cost .pricing-entry input')
    return heading.includes('v1') && savedInputs.length === 5 && await savedInputs[1].getValue() === 'c1-test-model'
  }, {
    timeout: 10_000,
    timeoutMsg: '应用设置页未显示保存后的模型价格版本和模型 ID',
  })
  if (process.env.MOHENG_C1_EVIDENCE_DIR) {
    await mkdir(process.env.MOHENG_C1_EVIDENCE_DIR, { recursive: true })
    await browser.saveScreenshot(path.join(process.env.MOHENG_C1_EVIDENCE_DIR, 'settings-pricing-release.png'))
  }

  process.stdout.write('PASS: packaged release opened Settings, read its projection, and saved a local test price row; no Vite server or Provider request\n')
} catch (error) {
  const listener = spawnSync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN'], { encoding: 'utf8' })
  process.stderr.write(`${error.stack ?? error}\nappExit=${JSON.stringify(appExit)}\nlistener=${listener.stdout.trim()}\n--- app output ---\n${appOutput}\n`)
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
    if (child.exitCode !== null) throw new Error(`Release app exited before ${url} was ready (exit=${child.exitCode})`)
    try {
      const response = await fetch(url)
      if (response.ok) return
    } catch {
      // The release app is still starting.
    }
    await new Promise(resolve => setTimeout(resolve, 200))
  }
  throw new Error(`Timed out waiting for ${url}`)
}

async function waitForOutput(readOutput, child) {
  const deadline = Date.now() + 10_000
  while (Date.now() < deadline) {
    if (readOutput()) return
    if (child.exitCode !== null) throw new Error(`Release app exited before WebDriver startup (exit=${child.exitCode})`)
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  throw new Error('Missing startup marker: E2E WebDriver 已启用')
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
