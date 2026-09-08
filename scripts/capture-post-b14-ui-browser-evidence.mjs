import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_EVIDENCE_DIR ?? '')
const port = Number(process.env.POST_B14_UI_BROWSER_PORT ?? 1488)
const baseUrl = `http://127.0.0.1:${port}`
const project = 'proj-moheng-demo'
const browserDir = path.join(evidenceDir, 'UI')
const domDir = path.join(evidenceDir, 'BROWSER', 'DOM')
const sourceCommit = process.env.POST_B14_UI_SOURCE_COMMIT ?? null

function loadPlaywright() {
  const require = createRequire(import.meta.url)
  for (const item of [process.env.PLAYWRIGHT_MODULE, '/Users/mac/.npm-global/lib/node_modules/playwright'].filter(Boolean)) {
    try { return require(item) } catch { /* try next configured runtime */ }
  }
  throw new Error('POST_B14_UI_PLAYWRIGHT_UNAVAILABLE')
}
function waitFor(url) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const retry = () => Date.now() - started > 60_000 ? reject(new Error(`BROWSER_SERVER_TIMEOUT:${url}`)) : setTimeout(check, 150)
    const check = () => {
      const request = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() })
      request.on('error', retry)
      request.setTimeout(1000, () => { request.destroy(); retry() })
    }
    check()
  })
}
function sha256(value) { return crypto.createHash('sha256').update(value, 'utf8').digest('hex') }
function run(command, args, env = {}) { return spawn(command, args, { cwd: root, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] }) }
function streamText(child) {
  let value = ''
  child.stdout.on('data', chunk => { value += chunk.toString() })
  child.stderr.on('data', chunk => { value += chunk.toString() })
  return () => value
}
function write(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`)
}

const routes = [
  ['U01', `/projects/${project}`, '打开总览'],
  ['U02', `/projects/${project}/production`, '打开创作'],
  ['U03', `/projects/${project}/directions`, '打开规划'],
  ['U04', `/projects/${project}/volumes`, '打开当前卷'],
  ['U05', `/projects/${project}/decisions`, '打开待处理'],
  ['U06', `/projects/${project}/health`, '打开长篇健康'],
  ['U07', `/projects/${project}/read`, '打开资料'],
  ['U08', `/projects/${project}/completion`, '打开完结准备'],
  ['U09', `/projects/${project}/settings`, '打开作品设置'],
]

async function main() {
  if (!evidenceDir) throw new Error('POST_B14_UI_EVIDENCE_DIR_REQUIRED')
  fs.mkdirSync(browserDir, { recursive: true })
  fs.mkdirSync(domDir, { recursive: true })
  let buildOutput = () => 'REUSED_PREBUILT_DIST'
  if (process.env.POST_B14_UI_SKIP_BUILD !== 'true') {
    const build = run('npm', ['run', 'build:fast'], { VITE_BROWSER_DEMO: 'true' })
    buildOutput = streamText(build)
    await new Promise((resolve, reject) => { build.once('close', code => code === 0 ? resolve() : reject(new Error(`BUILD_FAILED:${buildOutput()}`))) })
  }
  const server = run('npm', ['run', 'preview', '--', '--host', '127.0.0.1', '--port', String(port)], { VITE_BROWSER_DEMO: 'true' })
  const serverOutput = streamText(server)
  const { chromium } = loadPlaywright()
  const browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 1440, height: 840 } })
  const consoleErrors = []
  const pageErrors = []
  page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()) })
  page.on('pageerror', error => pageErrors.push(String(error)))
  const entries = []
  async function capture(id, route, action, expected, fixture = 'BrowserDemo project fixture') {
    await page.locator('.prototype-workspace').waitFor({ state: 'visible', timeout: 15_000 })
    await page.waitForTimeout(120)
    const text = await page.locator('body').innerText()
    const png = `${id}.png`
    const dom = `${id}.txt`
    await page.screenshot({ path: path.join(browserDir, png), fullPage: true })
    write(path.join(domDir, dom), text)
    entries.push({ id, route: new URL(page.url()).pathname + new URL(page.url()).search, viewport: await page.evaluate(() => ({ width: innerWidth, height: innerHeight })), projectFixture: fixture, userAction: action, expectedVisibleResult: expected, pngPath: `UI/${png}`, domSnapshotPath: `BROWSER/DOM/${dom}`, domTextSha256: sha256(text), sourceCommit, consoleErrors: consoleErrors.length, pageErrors: pageErrors.length, result: consoleErrors.length === 0 && pageErrors.length === 0 ? 'PASS' : 'FAIL' })
  }
  try {
    await waitFor(`${baseUrl}/`)
    for (const [id, route, action] of routes) {
      await page.goto(`${baseUrl}${route}`, { waitUntil: 'networkidle' })
      await capture(id, route, action, `${id} 对应的正式 Vue 路由和新 AppShell`)
    }

    await page.goto(`${baseUrl}/projects/${project}`, { waitUntil: 'networkidle' })
    const assistant = page.getByRole('complementary', { name: '项目助手' })
    await assistant.waitFor({ state: 'visible' })
    await capture('U10', `/projects/${project}`, '打开总览并观察常驻助手', '右侧助手展开且输入框可见')
    await page.getByTitle('折叠助手').click()
    await capture('U11', `/projects/${project}`, '点击折叠助手', '助手折叠为窄栏，主工作区仍可用')
    await page.getByTitle('展开助手').click()
    const question = assistant.locator('textarea')
    await question.fill('请核对当前作品的进度和故事核心是否一致')
    await page.getByRole('button', { name: '发送' }).click()
    await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 })
    await capture('U12', `/projects/${project}`, '发送自然语言查询', '返回基于当前作品记录的中文回答')
    await page.getByRole('button', { name: '查看依据' }).click()
    await capture('U13', `/projects/${project}`, '点击查看依据', '展开依据卡且不显示 Raw ID')
    await page.getByTitle('新对话').click()
    await question.fill('请修复当前作品展示')
    await page.getByRole('button', { name: '发送' }).click()
    await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 })
    await page.getByRole('button', { name: '查看修改方案' }).click()
    await page.locator('.prototype-assistant-plan').waitFor({ state: 'visible' })
    await capture('U14', `/projects/${project}`, '查询后点击查看修改方案', '显示影响范围、风险和备份要求')
    await capture('U15', `/projects/${project}`, '停留在授权前状态', '显示确认执行但尚未授权')
    await page.getByRole('button', { name: '确认执行' }).click()
    await page.waitForTimeout(2500)
    const executionText = await page.locator('body').innerText()
    if (!/修改已执行，并已重新核对结果。|修改没有完成/.test(executionText)) throw new Error(`ASSISTANT_EXECUTION_RESULT_MISSING:${executionText.slice(-1200)}`)
    await capture('U16', `/projects/${project}`, '明确授权并执行', '显示执行回执后的中文结果')

    await page.setViewportSize({ width: 1100, height: 840 })
    await page.goto(`${baseUrl}/projects/${project}`, { waitUntil: 'networkidle' })
    await capture('U17', `/projects/${project}`, '将窗口缩小到 1180px 以下', '助手转为可用的窄屏布局')
    await page.setViewportSize({ width: 880, height: 840 })
    await page.goto(`${baseUrl}/projects/${project}`, { waitUntil: 'networkidle' })
    await capture('U18', `/projects/${project}`, '将窗口缩小到 900px 以下', '左侧导航折叠，工作区和助手不溢出')
  } finally {
    await browser.close()
    server.kill('SIGTERM')
  }
  const report = { schema: 'post-b14-ui-browser-evidence-r1', sourceCommit, captureMethod: 'Playwright Chromium against Vite production build with real Vue routes/components and explicit browser fixture adapter', buildOutput: buildOutput(), serverOutput: serverOutput(), entries, consoleErrors, pageErrors, status: entries.length === 18 && consoleErrors.length === 0 && pageErrors.length === 0 ? 'PASS' : 'FAIL', runtimeAdapter: 'browser-demo-fixture; not Tauri authority evidence' }
  write(path.join(evidenceDir, 'UI_SCREENSHOT_MANIFEST.json'), report)
  write(path.join(evidenceDir, 'BROWSER_MATRIX.json'), { sourceCommit, browser: 'Chromium', entries: entries.map(item => ({ id: item.id, route: item.route, pngPath: item.pngPath, domSnapshotPath: item.domSnapshotPath, consoleErrors: item.consoleErrors, pageErrors: item.pageErrors, result: item.result })), consoleErrors, pageErrors, status: report.status, runtimeAdapter: report.runtimeAdapter })
  console.log(JSON.stringify({ status: report.status, itemCount: entries.length, consoleErrors: consoleErrors.length, pageErrors: pageErrors.length }, null, 2))
  if (report.status !== 'PASS') process.exitCode = 1
}

main().catch(error => { console.error(error.stack ?? String(error)); process.exitCode = 1 })
