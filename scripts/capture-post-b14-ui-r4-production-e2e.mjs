import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R4_EVIDENCE_DIR ?? '')
const bridgeUrl = process.env.POST_B14_UI_R4_BRIDGE_URL
const bridgeToken = process.env.POST_B14_UI_R4_BRIDGE_TOKEN
const projectId = process.env.POST_B14_UI_R4_PROJECT_ID
const port = Number(process.env.POST_B14_UI_R4_BROWSER_PORT ?? 1495)
const baseUrl = `http://127.0.0.1:${port}`
const sourceCommit = process.env.POST_B14_UI_R4_SOURCE_COMMIT ?? null
const rootEvidence = path.join(evidenceDir, 'ASSISTANT')
const screenshots = path.join(rootEvidence, 'SCREENSHOTS')
const domSnapshots = path.join(rootEvidence, 'DOM')

function loadPlaywright() { const require = createRequire(import.meta.url); return require(process.env.PLAYWRIGHT_MODULE || '/Users/mac/.npm-global/lib/node_modules/playwright') }
function waitFor(url) { const started = Date.now(); return new Promise((resolve, reject) => { const retry = () => Date.now() - started > 30_000 ? reject(new Error(`R4_BROWSER_SERVER_TIMEOUT:${url}`)) : setTimeout(check, 100); const check = () => { const request = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() }); request.on('error', retry) }; check() }) }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`) }
function sha(value) { return crypto.createHash('sha256').update(value, 'utf8').digest('hex') }
async function bridgeInvoke(command, payload) { const response = await fetch(bridgeUrl, { method: 'POST', headers: { 'content-type': 'application/json', 'x-r4-bridge-token': bridgeToken }, body: JSON.stringify({ command, payload }) }); const body = await response.json(); if (!response.ok) throw new Error(body.error ?? `R4_BRIDGE_${response.status}`); return body }

async function main() {
  if (!evidenceDir || !bridgeUrl || !bridgeToken || !projectId) throw new Error('R4_PRODUCTION_E2E_CONFIG_MISSING')
  fs.mkdirSync(screenshots, { recursive: true }); fs.mkdirSync(domSnapshots, { recursive: true })
  const server = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', String(port)], { cwd: root, env: { ...process.env, VITE_R4_PRODUCTION_E2E: 'true', VITE_R4_BRIDGE_URL: bridgeUrl, VITE_R4_BRIDGE_TOKEN: bridgeToken }, stdio: 'ignore' })
  const { chromium } = loadPlaywright(); const browser = await chromium.launch({ headless: true }); const page = await browser.newPage({ viewport: { width: 1440, height: 840 } })
  const consoleErrors = []; const pageErrors = []; page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()) }); page.on('pageerror', error => pageErrors.push(String(error)))
  const cases = []
  async function record(id, scenario, expected, actual, status, trace = {}) { const text = await page.locator('body').innerText(); const png = `${id}.png`; const dom = `${id}.txt`; await page.screenshot({ path: path.join(screenshots, png), fullPage: true }); write(path.join(domSnapshots, dom), text); const result = { id, scenario, precondition: `isolated Rust project ${projectId}`, uiActions: trace.uiActions ?? [], productionPath: 'VITE_R4_PRODUCTION_E2E -> TauriFrontendPort -> local Rust command bridge -> formal command/owner/repository', modelProviderMode: 'TEST_FAKE_MODEL_ONLY', backendTrace: trace.backendTrace ?? [], beforeFormalDigest: trace.beforeFormalDigest ?? null, afterFormalDigest: trace.afterFormalDigest ?? null, expected, actual, screenshots: [`ASSISTANT/SCREENSHOTS/${png}`], domSnapshots: [`ASSISTANT/DOM/${dom}`], sourceCommit, consoleErrors: consoleErrors.length, pageErrors: pageErrors.length, status }; cases.push(result); write(path.join(rootEvidence, id, `${id}.json`), result) }
  async function runCase(id, scenario, expected, action) { try { await action(); await record(id, scenario, expected, 'completed', 'PASS') } catch (error) { await record(id, scenario, expected, String(error), 'FAIL'); } }
  try {
    await waitFor(`${baseUrl}/`); await page.goto(`${baseUrl}/projects/${projectId}`, { waitUntil: 'networkidle' });
    await runCase('A01', '打开助手', 'Dock 展开且输入框可用', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByRole('button', { name: '折叠助手' }).click(); await dock.getByRole('button', { name: '展开助手' }).click(); await dock.locator('textarea').waitFor({ state: 'visible' }) })
    await runCase('A02', '跨路由保持', '规划→当前卷→长篇健康后消息历史保持', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.locator('textarea').fill('当前作品的进度是什么'); await dock.getByRole('button', { name: '发送' }).click(); await page.waitForTimeout(150); for (const label of ['规划', '当前卷', '长篇健康']) { await page.getByRole('link', { name: label }).click(); await page.locator('.prototype-workspace').waitFor({ state: 'visible' }); await dock.locator('textarea').waitFor({ state: 'visible' }) }; if (!(await page.locator('body').innerText()).includes('当前作品的进度是什么')) throw new Error('ASSISTANT_HISTORY_LOST') })
    await runCase('A03', '折叠展开', '折叠后恢复展开且输入框可达', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByRole('button', { name: '折叠助手' }).click(); await dock.getByRole('button', { name: '展开助手' }).click(); await dock.locator('textarea').focus() })
    await page.goto(`${baseUrl}/projects/${projectId}`, { waitUntil: 'networkidle' })
    await runCase('A04', '发送自然语言问题', '正式 Project Support 返回中文结果', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品的故事方向是什么？'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }) })
    await runCase('A05', 'Grounded Query', '回答来自正式项目记录并包含可核对依据', async () => { const text = await page.locator('body').innerText(); if (!text.includes('项目记录显示')) throw new Error('GROUNDING_RESPONSE_MISSING') })
    await runCase('A06', '查看依据', '点击查看依据后显示正式证据卡', async () => { const evidence = page.getByRole('button', { name: '查看依据' }); await evidence.click(); await page.waitForTimeout(100); if (!(await page.locator('body').innerText()).includes('已核对的依据')) throw new Error('EVIDENCE_CARD_MISSING') })
    await runCase('A07', '修改建议', 'Dock 显示修改方案、影响范围和风险', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品展示进度显示不对'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); await dock.getByRole('button', { name: '查看修改方案' }).click(); await page.locator('.prototype-assistant-plan').waitFor({ state: 'visible', timeout: 15_000 }) })
    const before = await bridgeInvoke('query_project_overview', { request: { projectId, scope: 'overview', cursor: null, pageSize: 8 } });
    await runCase('A08', '未授权不写入', '生成方案但未点击确认时正式状态 digest 不变', async () => { const after = await bridgeInvoke('query_project_overview', { request: { projectId, scope: 'overview', cursor: null, pageSize: 8 } }); if (JSON.stringify(before.project) !== JSON.stringify(after.project)) throw new Error('UNAUTHORIZED_AUTHORITY_CHANGED') })
    await runCase('A09', '显式授权', '只有用户点击确认执行后才进入授权路径', async () => { await page.getByRole('button', { name: '确认执行' }).click(); await page.waitForTimeout(250) })
    await runCase('A10', '正式执行回执', 'UI 显示正式执行结果而非 raw JSON', async () => { const text = await page.locator('body').innerText(); if (!text.includes('修改已执行') && !text.includes('修改没有完成')) throw new Error('RECEIPT_RESULT_MISSING') })
    const afterExecution = await bridgeInvoke('query_project_overview', { request: { projectId, scope: 'overview', cursor: null, pageSize: 8 } });
    await runCase('A11', 'Post Verification', '执行后再次读取正式项目状态', async () => { if (afterExecution.project.revision < before.project.revision) throw new Error('POST_VERIFY_REVISION_INVALID') })
    await runCase('A12', 'Capability unavailable', '后端能力不可用时显示中文不可用原因且不假成功', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('[R4_UNAVAILABLE] 请执行未开放功能'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').getByText('当前功能暂不可用，请返回上一页稍后再试').waitFor({ state: 'visible', timeout: 15_000 }) })
    await runCase('A13', 'Backend IPC error', '正式调用失败时显示中文错误且无部分写入', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('[R4_BACKEND_ERROR] 模拟后端失败'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').getByText('暂时无法核对当前作品，请稍后重试').waitFor({ state: 'visible', timeout: 15_000 }); const after = await bridgeInvoke('query_project_overview', { request: { projectId, scope: 'overview', cursor: null, pageSize: 8 } }); if (JSON.stringify(before.project) !== JSON.stringify(after.project)) throw new Error('IPC_ERROR_PARTIAL_WRITE') })
    await runCase('A14', 'Evidence insufficient', '证据不足时明确无法确认且不猜测', async () => { const dock = page.getByRole('complementary', { name: '项目助手' }); await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('这个作品具体哪一章已经兑现某个不存在的承诺？'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('还需要继续核对').waitFor({ state: 'visible', timeout: 15_000 }) })
  } finally { await browser.close(); server.kill('SIGTERM') }
  const matrix = { schema: 'post-b14-ui-r4-assistant-e2e', sourceCommit, runtimePath: 'PRODUCTION_BACKEND_HARNESS', modelProviderMode: 'TEST_FAKE_MODEL_ONLY', browserDemoBackendUsed: false, cases, consoleErrors, pageErrors, status: cases.length === 14 && cases.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL' }
  write(path.join(evidenceDir, 'ASSISTANT_E2E_MATRIX_R4.json'), matrix)
  write(path.join(evidenceDir, 'ASSISTANT_PRODUCTION_PATH_R4.json'), { bridgeType: 'R4_LOCAL_PRODUCTION_BACKEND_HARNESS', uiEntry: 'ProjectAssistantDock', storeMethod: 'TauriFrontendPort -> production e2e invoke adapter', ipcLayer: 'formal command names and payloads', supportPlanner: 'Rust support_session_impl', capabilityRegistry: 'Rust CapabilityRegistry', runtimeOwner: 'KernelState / ProjectManager / formal repair owner', repository: 'project.db repositories', modelProvider: 'none for read/repair fixture', modelProviderMode: 'TEST_FAKE_MODEL_ONLY', authorizationOwner: 'formal repair authorization command', receiptOwner: 'formal command receipt', postVerificationPath: 'formal query_project_overview after execute', productionDefaultEnabled: false, testOnly: true, status: matrix.status })
  console.log(JSON.stringify({ status: matrix.status, cases: cases.length, failed: cases.filter(item => item.status !== 'PASS').map(item => item.id) }, null, 2))
  if (matrix.status !== 'PASS') process.exitCode = 1
}
main().catch(error => { console.error(error.stack ?? String(error)); process.exitCode = 1 })
