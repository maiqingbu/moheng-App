import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B13_R1_EVIDENCE_DIR ?? '')
const browserDir = path.join(evidenceDir, 'MANUAL_BROWSER')
const port = Number(process.env.B13_R1_BROWSER_PORT ?? 1457)
const baseUrl = `http://127.0.0.1:${port}`

const INTERNAL_ID_PATTERN = /(?:\b(?:chapter|volume|artifact|obligation|project|fact|desire|task|model-run|state):[^\s\u3000]+|\b(?:projectId|artifactId|obligationId|volumeId|volume_id|payloadHash|stateDigest)\b)/i
const B14_PATTERN = /终局推进|完成度|可以完结|Endgame|Completion|BookExitState|CompletionGate|ending readiness/i

function loadPlaywright() {
  const require = createRequire(import.meta.url)
  const candidates = [
    process.env.PLAYWRIGHT_MODULE,
    '/Users/mac/.npm-global/lib/node_modules/playwright',
  ].filter(Boolean)
  for (const candidate of candidates) {
    try { return require(candidate) } catch { /* try the next configured runtime */ }
  }
  throw new Error('B13_R1_PLAYWRIGHT_UNAVAILABLE')
}

function sha256Text(value) {
  return crypto.createHash('sha256').update(value, 'utf8').digest('hex')
}

function waitForServer(url, timeoutMs = 30_000) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const check = () => {
      const request = http.get(url, response => {
        response.resume()
        if (response.statusCode && response.statusCode < 500) {
          resolve()
          return
        }
        retry()
      })
      request.on('error', retry)
      request.setTimeout(1000, () => { request.destroy(); retry() })
    }
    const retry = () => {
      if (Date.now() - started >= timeoutMs) reject(new Error(`B13_R1_BROWSER_SERVER_TIMEOUT:${url}`))
      else setTimeout(check, 200)
    }
    check()
  })
}

async function capture(page, scenario, filename) {
  const screenshotPath = path.join(browserDir, filename)
  await page.screenshot({ path: screenshotPath, fullPage: true })
  const actualVisibleText = await page.locator('body').innerText()
  const currentUrl = new URL(page.url())
  const internalLeakageCheck = !INTERNAL_ID_PATTERN.test(actualVisibleText)
  const b14LeakageCheck = !B14_PATTERN.test(actualVisibleText)
  return {
    ...scenario,
    route: `${currentUrl.pathname}${currentUrl.search}`,
    actualVisibleText,
    actualVisibleTextSource: 'playwright-dom-innerText',
    domTextSha256: sha256Text(actualVisibleText),
    internalLeakageCheck,
    b14LeakageCheck,
    screenshotPath: `MANUAL_BROWSER/${filename}`,
    result: internalLeakageCheck && b14LeakageCheck ? 'PASS' : 'FAIL',
  }
}

const scenarios = [
  { id: 'M01', project: 'proj-river-archive', mode: 'm01', expectedState: '状态稳定', fixture: '完整项目健康示例', trigger: '打开长篇健康页面，读取总体状态', routeLabel: '/projects/proj-river-archive/health' },
  { id: 'M02', project: 'proj-moheng-demo', mode: 'm02', expectedState: '总体稳定，有关注项', fixture: '存在长期关注项的项目', trigger: '打开长篇健康页面，读取关注项', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M03', project: 'proj-moheng-demo', mode: 'm03', expectedState: '需要优先处理', fixture: '确定性高影响项', trigger: '读取高优先级剧情义务', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M04', project: 'proj-moheng-demo', mode: 'm04', expectedState: '质量趋势需要关注', fixture: '创作建议质量趋势', trigger: '读取质量趋势建议', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M05', project: 'proj-moheng-demo', mode: 'm05', expectedState: '有读者期待到了回应节点', fixture: '读者期待提醒', trigger: '读取读者期待关注项', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M06', project: 'proj-moheng-demo', mode: 'm06', expectedState: '上下文样本仍在积累', fixture: '上下文样本不足', trigger: '读取上下文规模因素', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M07', project: 'proj-moheng-demo', mode: 'm07', expectedState: '数据仍在积累', fixture: '新建作品无正式章节', trigger: '打开新作品的长篇健康页面', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M08', project: 'proj-moheng-demo', mode: 'm08', expectedState: '部分来源需要核对', fixture: '来源记录缺失', trigger: '读取来源不可用提示', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M09', project: 'proj-moheng-demo', mode: 'm09', expectedState: '共 60 项', fixture: '60 条关注事项分页', trigger: '打开关注事项并点击下一页', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M10', project: 'proj-moheng-demo', mode: 'm10', expectedState: '按因素筛选', fixture: '多因素关注事项', trigger: '选择因素筛选并确认结果', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M11', project: 'proj-moheng-demo', mode: 'm11', expectedState: '卷工作区', fixture: '建议处理方向跳转', trigger: '点击卷工作区处理方向', routeLabel: '/projects/proj-moheng-demo/health' },
  { id: 'M12', project: 'proj-moheng-demo', mode: 'm12', expectedState: '高级详情不显示工程标识', fixture: '高级详情产品展示', trigger: '打开高级详情并检查用户可见文本', routeLabel: '/projects/proj-moheng-demo/health' },
]

async function main() {
  if (!evidenceDir || !fs.existsSync(evidenceDir)) throw new Error('B13_R1_BROWSER_EVIDENCE_DIR_REQUIRED')
  fs.mkdirSync(browserDir, { recursive: true })
  const server = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', String(port)], {
    cwd: root,
    env: { ...process.env, VITE_BROWSER_DEMO: 'true', B13_R1_BROWSER_EVIDENCE: 'true' },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let serverOutput = ''
  server.stdout.on('data', chunk => { serverOutput += chunk.toString() })
  server.stderr.on('data', chunk => { serverOutput += chunk.toString() })
  const { chromium } = loadPlaywright()
  const browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } })
  const items = []
  try {
    await waitForServer(`${baseUrl}/`)
    for (const scenario of scenarios) {
      const url = `${baseUrl}${scenario.routeLabel}?healthEvidence=${scenario.mode}`
      await page.goto(url, { waitUntil: 'domcontentloaded' })
      await page.getByRole('heading', { name: '长篇健康' }).waitFor({ state: 'visible', timeout: 10_000 })
      await page.waitForTimeout(250)
      let item
      if (scenario.id === 'M09') {
        await page.getByRole('button', { name: '下一页' }).click()
        await page.waitForTimeout(150)
        item = await capture(page, scenario, 'M09.png')
        await page.screenshot({ path: path.join(browserDir, 'M09-page2.png'), fullPage: true })
        item.additionalScreenshotPaths = ['MANUAL_BROWSER/M09-page2.png']
      } else if (scenario.id === 'M10') {
        await page.getByLabel('因素').selectOption({ label: '剧情义务' })
        item = await capture(page, scenario, 'M10.png')
      } else if (scenario.id === 'M11') {
        await page.getByRole('button', { name: '卷工作区' }).first().click()
        await page.waitForURL(/\/volumes$/)
        item = await capture(page, { ...scenario, routeLabel: new URL(page.url()).pathname }, 'M11.png')
      } else if (scenario.id === 'M12') {
        await page.getByRole('button', { name: '高级详情' }).click()
        await page.waitForTimeout(100)
        item = await capture(page, scenario, 'M12.png')
      } else {
        item = await capture(page, scenario, `${scenario.id}.png`)
      }
      items.push(item)
    }
  } finally {
    await browser.close()
    server.kill('SIGTERM')
  }
  fs.writeFileSync(path.join(evidenceDir, 'B13_R1_MANUAL_WALKTHROUGH.json'), `${JSON.stringify({
    schema: 'b13-r1-manual-walkthrough-r1',
    captureMethod: 'Playwright Chromium DOM innerText + PNG screenshot',
    command: `VITE_BROWSER_DEMO=true npm run dev -- --host 127.0.0.1 --port ${port}`,
    startedAt: new Date().toISOString(),
    sourceCommit: process.env.B13_R1_SOURCE_COMMIT ?? null,
    items,
    serverOutput,
    status: items.length === scenarios.length && items.every(item => item.result === 'PASS') ? 'PASS' : 'FAIL',
  }, null, 2)}\n`)
  console.log(JSON.stringify({ status: items.every(item => item.result === 'PASS') ? 'PASS' : 'FAIL', itemCount: items.length, evidenceDir }, null, 2))
  if (items.length !== scenarios.length || items.some(item => item.result !== 'PASS')) process.exitCode = 1
}

main().catch(error => {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
})
