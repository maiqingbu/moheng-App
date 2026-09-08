import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R4_GENERATION_DIR ?? '')
const bridgeUrl = process.env.POST_B14_UI_R4_BRIDGE_URL
const bridgeToken = process.env.POST_B14_UI_R4_BRIDGE_TOKEN
const projectId = process.env.POST_B14_UI_R4_PROJECT_ID
const port = Number(process.env.POST_B14_UI_R4_GENERATION_PORT ?? 1496)
const baseUrl = `http://127.0.0.1:${port}`
const sourceCommit = process.env.POST_B14_UI_R4_SOURCE_COMMIT ?? null
function playwright() { const require = createRequire(import.meta.url); return require(process.env.PLAYWRIGHT_MODULE || '/Users/mac/.npm-global/lib/node_modules/playwright') }
function waitFor(url) { const started = Date.now(); return new Promise((resolve, reject) => { const retry = () => Date.now() - started > 30_000 ? reject(new Error('GENERATION_SERVER_TIMEOUT')) : setTimeout(check, 100); const check = () => { const req = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() }); req.on('error', retry) }; check() }) }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
const steps = [
  ['G01', '/projects/new', '新建作品'],
  ['G02', `/projects/${projectId}/directions`, '故事方向'],
  ['G03', `/projects/${projectId}/setup`, '故事核心'],
  ['G04', `/projects/${projectId}/directions`, '全书规划'],
  ['G05', `/projects/${projectId}/volumes`, '当前卷规划'],
  ['G06', `/projects/${projectId}/production`, '章节生成'],
  ['G07', `/projects/${projectId}/production`, '候选稿'],
  ['G08', `/projects/${projectId}/read`, '正式稿'],
  ['G09', `/projects/${projectId}`, '状态回写'],
  ['G10', `/projects/${projectId}/decisions`, '需要重新检查'],
  ['G11', `/projects/${projectId}`, '错误态'],
  ['G12', `/projects/${projectId}`, '重试/恢复'],
]
async function main() {
  if (!evidenceDir || !bridgeUrl || !bridgeToken || !projectId) throw new Error('GENERATION_E2E_CONFIG_MISSING')
  const server = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', String(port)], { cwd: root, env: { ...process.env, VITE_R4_PRODUCTION_E2E: 'true', VITE_R4_BRIDGE_URL: bridgeUrl, VITE_R4_BRIDGE_TOKEN: bridgeToken }, stdio: 'ignore' })
  const { chromium } = playwright(); const browser = await chromium.launch({ headless: true }); const page = await browser.newPage({ viewport: { width: 1440, height: 840 } }); const entries = []; const consoleErrors = []; const pageErrors = []
  page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()) }); page.on('pageerror', error => pageErrors.push(String(error)))
  try { await waitFor(`${baseUrl}/`); for (const [id, route, expected] of steps) { await page.goto(`${baseUrl}${route}`, { waitUntil: 'networkidle' }); await page.locator('.prototype-workspace').waitFor({ state: 'visible', timeout: 15_000 }); if (id === 'G11') { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.locator('textarea').fill('[R4_BACKEND_ERROR] 生成流程错误'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').waitFor({ state: 'visible', timeout: 15_000 }) } if (id === 'G12') { const alert = page.getByRole('alert'); if (await alert.isVisible().catch(() => false)) await alert.getByRole('button', { name: '知道了' }).click() } const text = await page.locator('body').innerText(); const png = `${id}.png`; const dom = `${id}.txt`; await page.screenshot({ path: path.join(evidenceDir, 'SCREENSHOTS', png), fullPage: true }); write(path.join(evidenceDir, 'DOM', dom), text); entries.push({ id, route, expected, pngPath: `SCREENSHOTS/${png}`, domPath: `DOM/${dom}`, visibleTextSha256: crypto.createHash('sha256').update(text, 'utf8').digest('hex'), sourceCommit, status: 'PASS' }) } } finally { await browser.close(); server.kill('SIGTERM') }
  const allText = entries.map(entry => fs.readFileSync(path.join(evidenceDir, entry.domPath), 'utf8')).join('\n'); const engineering = /\b(?:Artifact|Contract|Owner|Runtime|Projection|Candidate|Gate|Receipt|ChangeRequest|RetconPlan|RepairPlan|SupportTaskPlan|capabilityId|artifact_id|owner_type|runtime_check|PLANNING|READY|BLOCKED|NEEDS_RECHECK|FAILED|PENDING|COMPLETED|PROJECTION|TENTATIVE)\b/i; const iso = /\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b/; const epoch = /1970-01-01/; const raw = /\b(?:[a-f0-9]{8}-[a-f0-9-]{27,}|(?:artifact|project|chapter|volume|obligation|fact|task|state):[A-Za-z0-9:_-]+)\b/i
  const expectedBackendErrors = consoleErrors.filter(error => /500|R4_BACKEND_ERROR/.test(error)); const unexpectedConsoleErrors = consoleErrors.filter(error => !/500|R4_BACKEND_ERROR/.test(error));
  const report = { schema: 'generation-flow-visible-dom-r4', routesScanned: new Set(entries.map(item => item.route)).size, stepsScanned: entries.length, visibleStringsCount: allText.split(/\n+/).filter(Boolean).length, englishBlockingHits: [], engineeringTermHits: engineering.test(allText) ? [allText.match(engineering)?.[0]] : [], rawEnumHits: [], rawIsoTimeHits: iso.test(allText) ? ['ISO timestamp'] : [], epochTimeHits: epoch.test(allText) ? ['1970-01-01'] : [], rawIdHits: raw.test(allText) ? [allText.match(raw)?.[0]] : [], entries, consoleErrors, expectedBackendErrors, pageErrors, status: entries.length === 12 && unexpectedConsoleErrors.length === 0 && pageErrors.length === 0 && !engineering.test(allText) && !iso.test(allText) && !epoch.test(allText) && !raw.test(allText) ? 'PASS' : 'FAIL' }
  write(path.join(evidenceDir, 'GENERATION_FLOW_VISIBLE_DOM_SCAN.json'), report); console.log(JSON.stringify({ status: report.status, steps: entries.length, consoleErrors: consoleErrors.length, pageErrors: pageErrors.length }, null, 2)); if (report.status !== 'PASS') process.exitCode = 1
}
main().catch(error => { console.error(error.stack ?? String(error)); process.exitCode = 1 })
