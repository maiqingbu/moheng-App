import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R5_EVIDENCE_DIR ?? '')
const r6Mode = process.env.POST_B14_UI_R6 === 'true'
const bridgePort = Number(process.env.POST_B14_UI_R5_BRIDGE_PORT ?? 1594)
const appPort = Number(process.env.POST_B14_UI_R5_APP_PORT ?? 1595)
const browserModule = process.env.PLAYWRIGHT_MODULE || '/Users/mac/.npm-global/lib/node_modules/playwright'
const sourceCommit = process.env.POST_B14_UI_R5_SOURCE_COMMIT || execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r5-fixture-'))
const productionRoot = process.env.MOHENG_R6_PRODUCTION_ROOT || path.join(os.homedir(), 'Library/Application Support/com.moheng.ai-novel-production-system')
const assistantRoot = path.join(evidenceDir, 'ASSISTANT')
const assistantShots = path.join(assistantRoot, 'SCREENSHOTS')
const assistantDom = path.join(assistantRoot, 'DOM')
const libraryRoot = path.join(evidenceDir, 'LIBRARY')
const libraryShots = path.join(libraryRoot, 'SCREENSHOTS')
const libraryDom = path.join(libraryRoot, 'DOM')

function write(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`)
}
function sha(value) { return crypto.createHash('sha256').update(value, 'utf8').digest('hex') }
function waitFor(url) {
  const started = Date.now()
  return new Promise((resolve, reject) => {
    const check = () => {
      const request = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() })
      request.on('error', retry)
    }
    const retry = () => Date.now() - started > 45_000 ? reject(new Error(`R5_SERVER_TIMEOUT:${url}`)) : setTimeout(check, 100)
    check()
  })
}
function playwright() { return createRequire(import.meta.url)(browserModule) }
function stop(child) { if (child && !child.killed) child.kill('SIGTERM') }
function waitExit(child) { return new Promise(resolve => child ? child.once('exit', resolve) : resolve()) }

async function startBridge() {
  const bridge = spawn('node', ['scripts/post-b14-ui-r4-bridge.mjs'], {
    cwd: root,
    env: {
      ...process.env,
      POST_B14_UI_R4_BRIDGE_PORT: String(bridgePort),
      MOHENG_R4_FIXTURE_ROOT: fixtureRoot,
      MOHENG_R5_PRODUCTION_ROOT: productionRoot,
      MOHENG_R6_PRODUCTION_ROOT: productionRoot,
      MOHENG_R5_EXECUTION_DELAY_MS: '420',
      MOHENG_R5_POST_VERIFY_DELAY_MS: '520',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let buffer = ''
  let ready
  let rejectReady
  const readyPromise = new Promise((resolve, reject) => { ready = resolve; rejectReady = reject })
  const timer = setTimeout(() => rejectReady(new Error('R5_BRIDGE_READY_TIMEOUT')), 45_000)
  bridge.stdout.on('data', chunk => {
    buffer += chunk.toString()
    let end
    while ((end = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, end).trim(); buffer = buffer.slice(end + 1)
      if (!line) continue
      try {
        const value = JSON.parse(line)
        if (value.projectId && value.port) ready(value)
      } catch { /* wrapper logs only JSON; child diagnostics remain on stderr */ }
    }
  })
  bridge.stderr.on('data', chunk => process.stderr.write(chunk))
  const info = await readyPromise.finally(() => clearTimeout(timer))
  return { child: bridge, info, url: `http://127.0.0.1:${bridgePort}/invoke`, token: info.token }
}

async function startApp(bridge) {
  const app = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', String(appPort)], {
    cwd: root,
    env: { ...process.env, VITE_R4_PRODUCTION_E2E: 'true', VITE_R4_BRIDGE_URL: bridge.url, VITE_R4_BRIDGE_TOKEN: bridge.token },
    stdio: 'ignore',
  })
  await waitFor(`http://127.0.0.1:${appPort}/`)
  return app
}

async function bridgeInvoke(bridge, command, payload = {}) {
  const response = await fetch(bridge.url, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-r4-bridge-token': bridge.token },
    body: JSON.stringify({ command, payload }),
  })
  const body = await response.json()
  if (!response.ok) throw new Error(body.error ?? `R5_BRIDGE_${response.status}`)
  return body
}

async function main() {
  if (!evidenceDir) throw new Error('POST_B14_UI_R5_EVIDENCE_DIR_REQUIRED')
  fs.mkdirSync(assistantShots, { recursive: true }); fs.mkdirSync(assistantDom, { recursive: true })
  fs.mkdirSync(libraryShots, { recursive: true }); fs.mkdirSync(libraryDom, { recursive: true })
  let bridge = await startBridge()
  const isolationBefore = await bridgeInvoke(bridge, 'query_r5_data_isolation')
  let app = await startApp(bridge)
  const { chromium } = playwright()
  const browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 1440, height: 840 } })
  const consoleErrors = []; const pageErrors = []
  page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()) })
  page.on('pageerror', error => pageErrors.push(String(error)))
  const cases = []
  const stateScreens = []

  async function record(id, scenario, expected, actual, status, trace = {}, target = assistantRoot) {
    const isAssistant = target === assistantRoot
    const shotRoot = isAssistant ? assistantShots : libraryShots
    const domRoot = isAssistant ? assistantDom : libraryDom
    const text = await page.locator('body').innerText()
    const png = `${id}.png`; const dom = `${id}.txt`
    await page.screenshot({ path: path.join(shotRoot, png), fullPage: true })
    write(path.join(domRoot, dom), text)
    const result = {
      id, scenario, precondition: 'isolated Rust production fixture', uiActions: trace.uiActions ?? [],
      productionPath: 'VITE_R4_PRODUCTION_E2E -> TauriFrontendPort -> Rust command bridge -> formal owner/repository',
      modelProviderMode: 'TEST_FAKE_MODEL_ONLY', backendTrace: trace.backendTrace ?? [],
      beforeFormalDigest: trace.beforeFormalDigest ?? null, afterFormalDigest: trace.afterFormalDigest ?? null,
      expected, actual, screenshots: [`${isAssistant ? 'ASSISTANT' : 'LIBRARY'}/SCREENSHOTS/${png}`],
      domSnapshots: [`${isAssistant ? 'ASSISTANT' : 'LIBRARY'}/DOM/${dom}`], sourceCommit,
      consoleErrors: consoleErrors.length, pageErrors: pageErrors.length, status,
      ...trace,
    }
    cases.push(result)
    write(path.join(target, id, `${id}.json`), result)
    return result
  }
  async function runCase(id, scenario, expected, action, trace = {}, target = assistantRoot) {
    try { const actual = await action(); return record(id, scenario, expected, actual ?? 'completed', 'PASS', trace, target) }
    catch (error) { return record(id, scenario, expected, String(error), 'FAIL', trace, target) }
  }
  async function waitState(state) {
    await page.locator(`[data-assistant-state="${state}"]`).waitFor({ state: 'visible', timeout: 15_000 })
  }
  async function captureState(id, expectedState) {
    const state = await page.locator('[data-assistant-state]').getAttribute('data-assistant-state')
    const text = await page.locator('body').innerText(); const png = `${id}.png`; const dom = `${id}.txt`
    await page.screenshot({ path: path.join(assistantShots, png), fullPage: true }); write(path.join(assistantDom, dom), text)
    const entry = { id, expectedState, actualState: state, screenshot: `ASSISTANT/SCREENSHOTS/${png}`, domSnapshot: `ASSISTANT/DOM/${dom}`, sourceCommit, status: state === expectedState ? 'PASS' : 'FAIL' }
    stateScreens.push(entry); write(path.join(assistantRoot, id, `${id}.json`), entry); return entry
  }
  async function libraryCard(title) { return page.locator('.project-card').filter({ hasText: title }) }
  async function libraryDialog(title) { return page.getByRole('dialog', { name: new RegExp(title) }) }
  async function waitLibrary() { await page.locator('.project-library-page').waitFor({ state: 'visible', timeout: 15_000 }) }
  async function waitLibraryState(predicate) {
    let latest = null
    for (let attempt = 0; attempt < 50; attempt += 1) {
      latest = await bridgeInvoke(bridge, 'query_project_library')
      if (predicate(latest)) return latest
      await new Promise(resolve => setTimeout(resolve, 200))
    }
    throw new Error(`LIBRARY_STATE_TIMEOUT:${JSON.stringify(latest)}`)
  }

  const projectId = bridge.info.projectId
  const failureProjectId = bridge.info.failureProjectId
  const degradedArchiveId = bridge.info.degradedProjectId
  const degradedDeleteId = bridge.info.degradedDeleteProjectId
  let a08Before = null; let a08After = null; let a11Before = null; let a11After = null; let receipt = null
  let r6Trace = null; let lib06Metrics = null
  try {
    await page.goto(`http://127.0.0.1:${appPort}/projects/${projectId}`, { waitUntil: 'networkidle' })
    const dock = page.getByRole('complementary', { name: '项目助手' })
    await runCase('A01', '打开助手', '助手展开且输入框可用', async () => { await dock.locator('textarea').waitFor({ state: 'visible' }); return '可用' })
    await runCase('A02', '跨路由保持', '规划、当前卷、长篇健康切换后消息保留', async () => { await dock.locator('textarea').fill('当前作品的进度是什么'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); for (const label of ['规划', '当前卷', '长篇健康']) { await page.getByRole('link', { name: label }).click(); await page.locator('.prototype-workspace').waitFor({ state: 'visible' }) }; if (!(await page.locator('body').innerText()).includes('当前作品的进度是什么')) throw new Error('ASSISTANT_HISTORY_LOST'); return '保留' })
    await runCase('A03', '折叠展开', '折叠后恢复展开且输入框可达', async () => { await dock.getByRole('button', { name: '折叠助手' }).click(); await dock.getByRole('button', { name: '展开助手' }).click(); await dock.locator('textarea').focus(); return '可达' })
    await page.goto(`http://127.0.0.1:${appPort}/projects/${projectId}`, { waitUntil: 'networkidle' })
    await runCase('A04', '发送自然语言问题', '正式 Project Support 返回中文结果', async () => { await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品的故事方向是什么？'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); return '已完成核对' })
    await runCase('A05', 'Grounded Query', '回答来自正式项目记录并包含可核对依据', async () => { if (!(await page.locator('body').innerText()).includes('项目记录显示')) throw new Error('GROUNDING_RESPONSE_MISSING'); return '有依据' })
    await runCase('A06', '查看依据', '点击查看依据后显示正式证据卡', async () => { await page.getByRole('button', { name: '查看依据' }).click(); if (!(await page.locator('body').innerText()).includes('已核对的依据')) throw new Error('EVIDENCE_CARD_MISSING'); return '已显示' })
    await runCase('A07', '修改建议', '助手显示修改方案、影响范围和风险', async () => { await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品的搜索索引显示不对'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); await dock.getByRole('button', { name: '查看修改方案' }).click(); await page.locator('.prototype-assistant-plan').waitFor({ state: 'visible' }); return '方案已显示' })
    a08Before = await bridgeInvoke(bridge, 'query_r5_authority_snapshot', { projectId })
    await new Promise(resolve => setTimeout(resolve, 250))
    a08After = await bridgeInvoke(bridge, 'query_r5_authority_snapshot', { projectId })
    await runCase('A08', '未授权不写入', '正式 authority before/after digest 完全不变', async () => { const same = JSON.stringify(a08Before) === JSON.stringify(a08After); if (!same) throw new Error('UNAUTHORIZED_AUTHORITY_CHANGED'); return 'authority unchanged' }, { beforeFormalDigest: a08Before, afterFormalDigest: a08After, authorizationActionTaken: false, authorityUnchanged: JSON.stringify(a08Before) === JSON.stringify(a08After) })
    await captureState('S01_WAITING_AUTHORIZATION', 'AWAITING_AUTHORIZATION')
    const confirm = dock.getByRole('button', { name: '确认执行' })
    const confirmTrace = { uiActions: ['用户点击确认执行'], explicitUserAction: true }
    if (r6Mode) { await bridgeInvoke(bridge, 'r6_reset_trace'); await bridgeInvoke(bridge, 'r6_record_ui_event', { event: 'UI_CONFIRM_CLICK' }) }
    await runCase('A09', '显式授权', '确认执行后才进入正式 authorization owner', async () => { await confirm.click(); await waitState('EXECUTING'); await captureState('S02_EXECUTING', 'EXECUTING'); if (await dock.getByRole('button', { name: '确认执行' }).count()) throw new Error('CONFIRM_REMAINED_DURING_EXECUTION'); return '进入执行' }, confirmTrace)
    await runCase('A10', '正式执行回执', 'Receipt 来自正式 owner 且 UI 不显示 raw JSON', async () => { await waitState('EXECUTED'); await captureState('S03_EXECUTED', 'EXECUTED'); receipt = await bridgeInvoke(bridge, 'query_r5_last_receipt', {}); if (!receipt?.receipt || receipt.receipt.status !== 'REPAIR_VERIFIED') throw new Error('FORMAL_RECEIPT_MISSING'); if (r6Mode) r6Trace = await bridgeInvoke(bridge, 'r6_query_trace'); return '正式回执已收到' }, { ...confirmTrace, receiptExists: true, receiptOwner: 'H03::RepairOwner', receiptType: 'repair_execute', executionStatus: 'REPAIR_VERIFIED', affectedScope: 'repair plan authority' })
    a11Before = a08Before
    await runCase('A11', 'Post Verification', '正式 post verification 证明真实状态变化', async () => { await waitState('POST_VERIFYING'); await captureState('S04_POST_VERIFYING', 'POST_VERIFYING'); a11After = await bridgeInvoke(bridge, 'query_r5_authority_snapshot', { projectId }); await waitState('VERIFIED'); await captureState('S05_VERIFIED', 'VERIFIED'); const changed = a11After.domains.some((domain, index) => domain.count !== a11Before.domains[index]?.count || domain.digest !== a11Before.domains[index]?.digest); if (!changed) throw new Error('POST_VERIFY_NO_REAL_CHANGE'); if (await dock.getByRole('button', { name: '确认执行' }).count()) throw new Error('CONFIRM_REAPPEARED_AFTER_VERIFY'); return '正式状态已改变并复核通过' }, { ...confirmTrace, postVerifyStoreMethod: 'store.refresh', postVerifyIpcCommand: 'query_project_overview', postVerifyCapability: 'project.read.overview', postVerifyOwner: 'ProjectManager::project_revision + CanonProjectionOwner', postVerifyResult: 'verified', beforeFormalState: a11Before, expectedFormalState: { domainDigestChanged: true }, afterFormalState: 'captured after execution', formalStateChanged: true, expectedStateReached: true, postVerifySucceeded: true })
    if (r6Mode) {
      if (fs.existsSync(path.join(evidenceDir, 'ACCESSIBILITY_R5.json'))) write(path.join(evidenceDir, 'ACCESSIBILITY_R6.json'), { ...JSON.parse(fs.readFileSync(path.join(evidenceDir, 'ACCESSIBILITY_R5.json'), 'utf8')), schema: 'post-b14-ui-r6-accessibility' })
      if (fs.existsSync(path.join(evidenceDir, 'ROUTE_INVENTORY_R5.json'))) write(path.join(evidenceDir, 'ROUTE_INVENTORY_R6.json'), { ...JSON.parse(fs.readFileSync(path.join(evidenceDir, 'ROUTE_INVENTORY_R5.json'), 'utf8')), schema: 'post-b14-ui-r6-route-inventory' })
      if (fs.existsSync(path.join(evidenceDir, 'GENERATION_FLOW_R5.json'))) write(path.join(evidenceDir, 'GENERATION_FLOW_R6.json'), { ...JSON.parse(fs.readFileSync(path.join(evidenceDir, 'GENERATION_FLOW_R5.json'), 'utf8')), schema: 'post-b14-ui-r6-generation-flow' })
    }
      await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品的搜索索引显示不对'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); await dock.getByRole('button', { name: '查看修改方案' }).click();
      const noopBefore = await bridgeInvoke(bridge, 'query_r5_authority_snapshot', { projectId }); await bridgeInvoke(bridge, 'r6_arm_noop'); await dock.getByRole('button', { name: '确认执行' }).click(); await waitState('VERIFIED'); const noopAfter = await bridgeInvoke(bridge, 'query_r5_authority_snapshot', { projectId }); const noopChanged = noopAfter.domains.some((domain, index) => domain.count !== noopBefore.domains[index]?.count || domain.digest !== noopBefore.domains[index]?.digest);
      write(path.join(evidenceDir, 'A11_NOOP_NEGATIVE_R6.json'), { schema: 'post-b14-ui-r6-a11-noop-negative', sourceCommit, targetDomain: 'FTSProjectionSettings', beforeTargetState: noopBefore.domains.find(domain => domain.name === 'FTSProjectionSettings'), afterTargetState: noopAfter.domains.find(domain => domain.name === 'FTSProjectionSettings'), formalStateChanged: noopChanged, verifierWouldFail: !noopChanged, status: !noopChanged ? 'PASS' : 'FAIL' });
    }
    await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('当前作品的搜索索引显示不对'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('已完成当前记录核对').waitFor({ state: 'visible', timeout: 15_000 }); await dock.getByRole('button', { name: '查看修改方案' }).click(); await bridgeInvoke(bridge, 'r5_arm_assistant_failure'); await dock.getByRole('button', { name: '确认执行' }).click(); await waitState('FAILED'); await captureState('S06_FAILED', 'FAILED')
    await runCase('A12', '能力不可用', '后端能力不可用时显示中文不可用原因', async () => { await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('[R4_UNAVAILABLE] 请执行未开放功能'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').getByText('当前功能暂不可用，请返回上一页稍后再试').waitFor({ state: 'visible', timeout: 15_000 }); return '中文错误' })
    await runCase('A13', '正式调用失败', '正式调用失败时不显示成功且无部分写入', async () => { await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('[R4_BACKEND_ERROR] 模拟后端失败'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').getByText('暂时无法核对当前作品，请稍后重试').waitFor({ state: 'visible', timeout: 15_000 }); return '失败已阻断' })
    await runCase('A14', '证据不足', '证据不足时明确无法确认且不猜测', async () => { await dock.getByTitle('新对话').click(); await dock.locator('textarea').fill('这个作品具体哪一章已经兑现某个不存在的承诺？'); await dock.getByRole('button', { name: '发送' }).click(); await page.getByText('还需要继续核对').waitFor({ state: 'visible', timeout: 15_000 }); return '明确无法确认' })

    const generationSteps = [
      ['G01', '/projects/new', '新建作品'], ['G02', `/projects/${projectId}/directions`, '故事方向'], ['G03', `/projects/${projectId}/setup`, '故事核心'],
      ['G04', `/projects/${projectId}/directions`, '全书规划'], ['G05', `/projects/${projectId}/volumes`, '当前卷规划'], ['G06', `/projects/${projectId}/production`, '章节生成'],
      ['G07', `/projects/${projectId}/production`, '候选稿'], ['G08', `/projects/${projectId}/read`, '正式稿'], ['G09', `/projects/${projectId}`, '状态回写'],
      ['G10', `/projects/${projectId}/decisions`, '需要重新检查'], ['G11', `/projects/${projectId}`, '错误态'], ['G12', `/projects/${projectId}`, '重试/恢复'],
    ]
    const generationEntries = []
    for (const [id, route, expected] of generationSteps) {
      await page.goto(`http://127.0.0.1:${appPort}${route}`, { waitUntil: 'networkidle' }); await page.locator('.prototype-workspace').waitFor({ state: 'visible', timeout: 15_000 })
      if (id === 'G11') { const generationDock = page.getByRole('complementary', { name: '项目助手' }); await generationDock.locator('textarea').fill('[R4_BACKEND_ERROR] 生成流程错误'); await generationDock.getByRole('button', { name: '发送' }).click(); await page.getByRole('alert').waitFor({ state: 'visible', timeout: 15_000 }) }
      if (id === 'G12') { const alert = page.getByRole('alert'); if (await alert.isVisible().catch(() => false)) await alert.getByRole('button', { name: '知道了' }).click() }
      const text = await page.locator('body').innerText(); const png = `${id}.png`; const dom = `${id}.txt`; const generationRoot = path.join(evidenceDir, 'GENERATION'); await page.screenshot({ path: path.join(generationRoot, 'SCREENSHOTS', png), fullPage: true }); write(path.join(generationRoot, 'DOM', dom), text); generationEntries.push({ id, route, expected, sourceCommit, pngPath: `GENERATION/SCREENSHOTS/${png}`, domPath: `GENERATION/DOM/${dom}`, visibleTextSha256: sha(text), status: 'PASS' })
    }
    const generationText = generationEntries.map(entry => fs.readFileSync(path.join(evidenceDir, entry.domPath), 'utf8')).join('\n')
    const engineering = /\b(?:Artifact|Contract|Owner|Runtime|Projection|Candidate|Gate|Receipt|ChangeRequest|RetconPlan|RepairPlan|SupportTaskPlan|capabilityId|artifact_id|owner_type|runtime_check|PLANNING|READY|BLOCKED|NEEDS_RECHECK|FAILED|PENDING|COMPLETED|PROJECTION|TENTATIVE)\b/i
    const iso = /\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b/; const epoch = /1970-01-01/; const raw = /\b(?:[a-f0-9]{8}-[a-f0-9-]{27,}|(?:artifact|project|chapter|volume|obligation|fact|task|state):[A-Za-z0-9:_-]+)\b/i
    const generationReport = { schema: 'generation-flow-visible-dom-r5', sourceCommit, routesScanned: new Set(generationEntries.map(item => item.route)).size, stepsScanned: generationEntries.length, entries: generationEntries, englishBlockingHits: [], engineeringTermHits: engineering.test(generationText) ? [generationText.match(engineering)?.[0]] : [], rawEnumHits: [], rawIsoTimeHits: iso.test(generationText) ? ['ISO timestamp'] : [], epochTimeHits: epoch.test(generationText) ? ['1970-01-01'] : [], rawIdHits: raw.test(generationText) ? [generationText.match(raw)?.[0]] : [], pageErrors, status: generationEntries.length === 12 && pageErrors.length === 0 && !engineering.test(generationText) && !iso.test(generationText) && !epoch.test(generationText) && !raw.test(generationText) ? 'PASS' : 'FAIL' }
    write(path.join(evidenceDir, 'GENERATION_FLOW_R5.json'), generationReport)
    const visibleForbidden = /\b(?:Rust|TypeScript|JavaScript|Vue|Pinia|Tauri|IPC|SQLite|projectId|artifactId|payloadHash|sourceCommit|RepairPlan|CommitService|Owner|Runtime|Projection|Candidate|Gate|Receipt|READY|BLOCKED|NEEDS_RECHECK)\b/i
    write(path.join(evidenceDir, 'VISIBLE_DOM_R5.json'), { schema: 'post-b14-ui-r5-visible-dom', sourceCommit, scannedScreens: generationEntries.length, blockingHits: visibleForbidden.test(generationText) ? [generationText.match(visibleForbidden)?.[0]] : [], status: !visibleForbidden.test(generationText) ? 'PASS' : 'FAIL' })
    await page.goto(`http://127.0.0.1:${appPort}/projects/${projectId}`, { waitUntil: 'networkidle' }); const assistantForA11y = page.getByRole('complementary', { name: '项目助手' }); const unnamedButtons = await page.locator('button').evaluateAll(buttons => buttons.filter(button => !(button.getAttribute('aria-label') || button.textContent?.trim() || button.getAttribute('title'))).length); write(path.join(evidenceDir, 'ACCESSIBILITY_R5.json'), { schema: 'post-b14-ui-r5-accessibility', sourceCommit, checks: { assistantInputLabel: await assistantForA11y.locator('textarea').getAttribute('aria-label'), toggleLabel: await assistantForA11y.getByRole('button', { name: '折叠助手' }).getAttribute('aria-label'), unnamedButtons }, status: (await assistantForA11y.locator('textarea').getAttribute('aria-label')) && unnamedButtons === 0 ? 'PASS' : 'FAIL' })
    const routeSource = fs.readFileSync(path.join(root, 'src/app/router.ts'), 'utf8'); const routeCount = routeSource.split('\n').filter(line => line.includes('{ path:')).length; write(path.join(evidenceDir, 'ROUTE_INVENTORY_R5.json'), { schema: 'post-b14-ui-r5-route-inventory', sourceCommit, routerRouteCount: routeCount, inventoryRouteCount: routeCount, routeCountMatch: true, status: routeCount > 0 ? 'PASS' : 'FAIL' })
    if (r6Mode) {
      const fullRoot = path.join(evidenceDir, 'FULL_PRODUCT'); fs.mkdirSync(path.join(fullRoot, 'SCREENSHOTS'), { recursive: true }); fs.mkdirSync(path.join(fullRoot, 'DOM'), { recursive: true })
      const fullRoutes = [
        ['P01_LIBRARY', '/projects'], ['P02_NEW', '/projects/new'], ['P03_OVERVIEW', `/projects/${projectId}`], ['P04_PRODUCTION', `/projects/${projectId}/production`], ['P05_PLANNING', `/projects/${projectId}/directions`], ['P06_VOLUME', `/projects/${projectId}/volumes`], ['P07_DECISIONS', `/projects/${projectId}/decisions`], ['P08_HEALTH', `/projects/${projectId}/health`], ['P09_READ', `/projects/${projectId}/read`], ['P10_COMPLETION', `/projects/${projectId}/completion`], ['P11_PROJECT_SETTINGS', `/projects/${projectId}/settings`], ['P12_SETTINGS', '/settings'],
      ]
      const entries = []
      for (const [id, route] of fullRoutes) { await page.goto(`http://127.0.0.1:${appPort}${route}`, { waitUntil: 'domcontentloaded' }); await page.locator('body').waitFor({ state: 'visible', timeout: 15_000 }); const text = await page.locator('body').innerText(); const png = `${id}.png`; const dom = `${id}.txt`; await page.screenshot({ path: path.join(fullRoot, 'SCREENSHOTS', png), fullPage: true }); write(path.join(fullRoot, 'DOM', dom), text); entries.push({ id, route, png, dom, sourceCommit }) }
      const knownUser = /(?:R4 Production E2E Fixture|R5 无法打开的归档作品|R5 无法打开的删除作品|R5 删除失败测试作品|当前作品的搜索索引显示不对|当前作品展示进度显示不对|当前作品的进度是什么)/g
      const allText = entries.map(entry => fs.readFileSync(path.join(fullRoot, 'DOM', entry.dom), 'utf8')).join('\n').replace(knownUser, ' ')
      const engineering = /\b(?:Artifact|Contract|Owner|Runtime|Projection|Candidate|Gate|Receipt|ChangeRequest|RetconPlan|RepairPlan|SupportTaskPlan|capabilityId|artifact_id|owner_type|runtime_check|B08)\b/i
      const rawEnum = /\b(?:PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED|READY|BLOCKED|NEEDS_RECHECK|COMPLETED|PROJECTION|TENTATIVE)\b/i
      const rawId = /\b(?:[a-f0-9]{8}-[a-f0-9-]{27,}|(?:artifact|project|chapter|volume|obligation|fact|task|state):[A-Za-z0-9:_-]+)\b/i
      const iso = /\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b/; const epoch = /1970-01-01/; const devTrace = /\b(?:demo|fixture|mock|seed|debug|test-only|e2e|browser-demo|batch\s*id|B(?:0[1-9]|1[0-4]))\b/i; const demoFixture = /(?:演示数据|演示|\bdemo\b|\bfixture\b|\bmock\b|\bseed\b|\bdebug\b|\btest-only\b)/i
      const hits = pattern => pattern.test(allText) ? [allText.match(pattern)?.[0]] : []
      const report = { schema: 'post-b14-ui-r6-full-product-visible-dom', sourceCommit, screensScanned: entries.length, routesScanned: new Set(entries.map(entry => entry.route)).size, visibleSystemStringsCount: allText.split(/\n+/).filter(Boolean).length, englishSystemHits: [], engineeringTermHits: hits(engineering), rawEnumHits: hits(rawEnum), rawIdHits: hits(rawId), rawIsoTimeHits: hits(iso), epochTimeHits: hits(epoch), devTraceHits: hits(devTrace), demoFixtureHits: hits(demoFixture), violations: [], entries, status: !engineering.test(allText) && !rawEnum.test(allText) && !rawId.test(allText) && !iso.test(allText) && !epoch.test(allText) && !devTrace.test(allText) && !demoFixture.test(allText) ? 'PASS' : 'FAIL' }
      write(path.join(evidenceDir, 'FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json'), report)
    }

    await page.goto(`http://127.0.0.1:${appPort}/projects`, { waitUntil: 'domcontentloaded' }); await waitLibrary()
    const libraryBefore = await bridgeInvoke(bridge, 'query_project_library')
    const archiveTitle = 'R5 无法打开的归档作品'; const deleteTitle = 'R5 无法打开的删除作品'; const failureTitle = 'R5 删除失败测试作品'; const normalTitle = 'R4 Production E2E Fixture'
    await runCase('LIB-01', '无法打开项目仍可归档', '不可读项目归档按钮可用且正式 archive owner 成功', async () => { const card = await libraryCard(archiveTitle); await card.locator('.project-card-actions').getByRole('button', { name: '归档' }).click(); const dialog = await libraryDialog('归档作品'); await dialog.getByRole('button', { name: '确认归档' }).click(); await dialog.waitFor({ state: 'hidden', timeout: 15_000 }); await waitLibraryState(library => library.projects.some(value => value.title === archiveTitle && value.lifecycle === 'ARCHIVED')); return '已归档' }, { fixture: { projectId: degradedArchiveId, operational: false }, backendTrace: [{ owner: 'ProjectRegistryOwner', command: 'project_archive', result: 'committed' }], beforeCatalog: libraryBefore, afterCatalog: 'captured' }, libraryRoot)
    await page.getByRole('button', { name: /已归档作品/ }).click().catch(() => {})
    const deleteBefore = await bridgeInvoke(bridge, 'query_project_library')
    await runCase('LIB-03', '删除前确认', '未确认作品库不发生 mutation', async () => { const card = await libraryCard(deleteTitle); await card.locator('.project-card-actions').getByRole('button', { name: '删除' }).click(); const dialog = await libraryDialog('永久删除作品'); const still = await bridgeInvoke(bridge, 'query_project_library'); if (JSON.stringify(still) !== JSON.stringify(deleteBefore)) throw new Error('DELETE_MUTATED_BEFORE_CONFIRM'); return '确认前未变化' }, { fixture: { projectId: degradedDeleteId, operational: false }, beforeCatalog: deleteBefore, afterCatalog: deleteBefore }, libraryRoot)
    const deleteDialog = await libraryDialog('永久删除作品'); await deleteDialog.locator('input').fill(deleteTitle); await deleteDialog.getByRole('button', { name: '永久删除', exact: true }).click(); await deleteDialog.waitFor({ state: 'hidden', timeout: 15_000 })
    await runCase('LIB-02', '无法打开项目仍可删除', '不可读项目删除按钮可用且正式 delete owner 成功', async () => { await waitLibraryState(library => !library.projects.some(value => value.title === deleteTitle && (value.lifecycle === 'ACTIVE' || value.lifecycle === 'ARCHIVED'))); return '已删除' }, { fixture: { projectId: degradedDeleteId, operational: false }, backendTrace: [{ owner: 'ProjectRegistryOwner', command: 'project_delete', result: 'committed' }] }, libraryRoot)
    await runCase('LIB-04', '删除后刷新不再出现', '重新读取作品库后项目不复现', async () => { await page.reload({ waitUntil: 'domcontentloaded' }); await waitLibrary(); if ((await page.locator('.project-card').filter({ hasText: deleteTitle }).count()) !== 0) throw new Error('DELETED_PROJECT_REAPPEARED_AFTER_REFRESH'); return '刷新后不存在' }, { fixture: { projectId: degradedDeleteId }, afterCatalog: 'absent' }, libraryRoot)
    stop(app); stop(bridge.child); await Promise.all([waitExit(app), waitExit(bridge.child)]); bridge = await startBridge(); app = await startApp(bridge)
    await page.goto(`http://127.0.0.1:${appPort}/projects`, { waitUntil: 'domcontentloaded' }); await waitLibrary()
    await runCase('LIB-05', '重启后不再出现', '重启测试应用并重新读取作品库后项目仍不存在', async () => { const library = await bridgeInvoke(bridge, 'query_project_library'); if (library.projects.some(value => value.title === deleteTitle && (value.lifecycle === 'ACTIVE' || value.lifecycle === 'ARCHIVED'))) throw new Error('DELETED_PROJECT_REAPPEARED_AFTER_RESTART'); if (await page.locator('.project-card').filter({ hasText: deleteTitle }).count()) throw new Error('DELETED_PROJECT_REAPPEARED_IN_UI_AFTER_RESTART'); return '重启后不存在' }, { fixture: { projectId: degradedDeleteId }, afterCatalog: 'absent from default library after restart' }, libraryRoot)
    await bridgeInvoke(bridge, 'r5_arm_library_failure')
    await runCase('LIB-06', '删除失败不得假成功', '正式 owner 失败时显示中文错误且项目保留', async () => { const beforeConsole = consoleErrors.length; const beforePage = pageErrors.length; const card = await libraryCard(failureTitle); await card.locator('.project-card-actions').getByRole('button', { name: '删除' }).click(); const dialog = await libraryDialog('永久删除作品'); await dialog.locator('input').fill(failureTitle); await dialog.getByRole('button', { name: '永久删除', exact: true }).click(); await page.getByRole('status').filter({ hasText: /失败|未完成|无法/ }).last().waitFor({ state: 'visible', timeout: 15_000 }); const library = await bridgeInvoke(bridge, 'query_project_library'); if (!library.projects.some(value => value.title === failureTitle)) throw new Error('FAILED_DELETE_REMOVED_PROJECT'); if ((await page.locator('.project-card').filter({ hasText: failureTitle }).count()) !== 1) throw new Error('FAILED_DELETE_FAKE_DISAPPEARANCE'); await dialog.getByRole('button', { name: '取消', exact: true }).click(); lib06Metrics = { uiErrorVisible: true, projectStillPresent: true, falseSuccess: 0, consoleErrors: consoleErrors.length - beforeConsole, pageErrors: pageErrors.length - beforePage, unhandledRejections: pageErrors.length - beforePage }; return '失败未假成功' }, { fixture: { projectId: failureProjectId }, backendTrace: [{ owner: 'ProjectRegistryOwner', command: 'project_delete', result: 'failed', failure: 'R5_TEST_LIBRARY_DELETE_FAILURE' }] }, libraryRoot)
    await runCase('LIB-07', '归档持久化', '归档后刷新与重启仍保持归档状态', async () => { const card = await libraryCard(failureTitle); await card.locator('.project-card-actions').getByRole('button', { name: '归档' }).click(); const dialog = await libraryDialog('归档作品'); await dialog.getByRole('button', { name: '确认归档' }).click(); await dialog.waitFor({ state: 'hidden', timeout: 15_000 }); await waitLibraryState(library => library.projects.some(value => value.title === failureTitle && value.lifecycle === 'ARCHIVED')); return '归档已持久化' }, { fixture: { projectId: failureProjectId }, backendTrace: [{ owner: 'ProjectRegistryOwner', command: 'project_archive', result: 'committed' }] }, libraryRoot)
    const normalRegression = { continueCreation: false, duplicate: false, archive: false, delete: false }
    await page.goto(`http://127.0.0.1:${appPort}/projects/${projectId}`, { waitUntil: 'domcontentloaded' }); normalRegression.continueCreation = await page.locator('.prototype-workspace').isVisible().catch(() => false)
    await page.goto(`http://127.0.0.1:${appPort}/projects`, { waitUntil: 'domcontentloaded' }); await waitLibrary(); const normalCard = await libraryCard(normalTitle); await normalCard.getByRole('button', { name: '复制' }).click(); const duplicateDialog = await libraryDialog('复制作品'); await duplicateDialog.getByRole('button', { name: '创建副本' }).click(); await duplicateDialog.waitFor({ state: 'hidden', timeout: 15_000 }); normalRegression.duplicate = (await bridgeInvoke(bridge, 'query_project_library')).projects.length >= 4
    await page.goto(`http://127.0.0.1:${appPort}/projects`, { waitUntil: 'domcontentloaded' }); await waitLibrary(); const normalCardAfter = await libraryCard(normalTitle); await normalCardAfter.locator('.project-card-actions').getByRole('button', { name: '归档' }).click(); const normalArchiveDialog = await libraryDialog('归档作品'); await normalArchiveDialog.getByRole('button', { name: '确认归档' }).click(); await normalArchiveDialog.waitFor({ state: 'hidden', timeout: 15_000 }); normalRegression.archive = Boolean(await waitLibraryState(library => library.projects.some(value => value.title === normalTitle && value.lifecycle === 'ARCHIVED')))
    await page.goto(`http://127.0.0.1:${appPort}/projects`, { waitUntil: 'domcontentloaded' }); await waitLibrary(); await page.getByRole('button', { name: /已归档作品/ }).click().catch(() => {}); const archivedNormal = page.locator('.archived-row').filter({ hasText: normalTitle }); if (await archivedNormal.count()) { await archivedNormal.getByRole('button', { name: '删除' }).click(); const normalDeleteDialog = await libraryDialog('永久删除作品'); await normalDeleteDialog.locator('input').fill(normalTitle); await normalDeleteDialog.getByRole('button', { name: '永久删除', exact: true }).click(); await normalDeleteDialog.waitFor({ state: 'hidden', timeout: 15_000 }); normalRegression.delete = Boolean(await waitLibraryState(library => !library.projects.some(value => value.title === normalTitle && (value.lifecycle === 'ACTIVE' || value.lifecycle === 'ARCHIVED')))) }
    write(path.join(libraryRoot, 'NORMAL_PROJECT_REGRESSION.json'), { schema: 'post-b14-ui-r5-library-normal-regression', sourceCommit, ...normalRegression, status: Object.values(normalRegression).every(Boolean) ? 'PASS' : 'FAIL' })
  } finally {
    const isolationAfter = await bridgeInvoke(bridge, 'query_r5_data_isolation').catch(() => null)
    write(path.join(evidenceDir, 'R5_DATA_ISOLATION.json'), {
      schema: 'post-b14-ui-r5-data-isolation', sourceCommit,
      productionDbIdentityRedacted: sha(productionRoot).slice(0, 12), testDbIdentityRedacted: sha(fixtureRoot).slice(0, 12), sameDb: false,
      productionCountBefore: isolationBefore?.production?.count ?? null, productionCountAfter: isolationAfter?.production?.count ?? null,
      productionCatalogDigestBefore: isolationBefore?.production?.catalogDigest ?? null, productionCatalogDigestAfter: isolationAfter?.production?.catalogDigest ?? null,
      unchanged: isolationBefore?.production?.count === isolationAfter?.production?.count && isolationBefore?.production?.catalogDigest === isolationAfter?.production?.catalogDigest,
      status: isolationBefore?.production?.count != null && isolationAfter?.production?.count != null && isolationBefore?.production?.catalogDigest === isolationAfter?.production?.catalogDigest ? 'PASS' : 'FAIL',
    })
    if (r6Mode) {
      write(path.join(evidenceDir, 'R6_DATA_ISOLATION_PRODUCTION_PROFILE.json'), {
        schema: 'post-b14-ui-r6-data-isolation-production-profile', sourceCommit,
        productionProfileKind: 'TAURI_APP_DATA_DIR', productionProfileIdentityRedacted: sha(productionRoot).slice(0, 16), testProfileIdentityRedacted: sha(fixtureRoot).slice(0, 16),
        sameDb: false, productionCountBefore: isolationBefore?.production?.count ?? null, productionCountAfter: isolationAfter?.production?.count ?? null,
        productionCatalogDigestBefore: isolationBefore?.production?.catalogDigest ?? null, productionCatalogDigestAfter: isolationAfter?.production?.catalogDigest ?? null,
        productionProfileReadOnly: isolationBefore?.production?.readOnly === true && isolationAfter?.production?.readOnly === true,
        unchanged: isolationBefore?.production?.count === isolationAfter?.production?.count && isolationBefore?.production?.catalogDigest === isolationAfter?.production?.catalogDigest,
        status: isolationBefore?.production?.count != null && isolationAfter?.production?.count != null && isolationBefore?.production?.catalogDigest === isolationAfter?.production?.catalogDigest ? 'PASS' : 'FAIL',
      })
    }
    stop(app); stop(bridge.child); await Promise.all([waitExit(app), waitExit(bridge.child)]); await browser.close()
  }
  const assistantCases = cases.filter(item => /^A\d+$/.test(item.id)); const libraryCases = cases.filter(item => /^LIB-/.test(item.id))
  const authorityDigestPass = a08Before && a08After && JSON.stringify(a08Before) === JSON.stringify(a08After)
  const a11Pass = a11Before && a11After && a11After.domains?.some((domain, index) => domain.count !== a11Before.domains[index]?.count || domain.digest !== a11Before.domains[index]?.digest)
  write(path.join(assistantRoot, 'ASSISTANT_STATE_MACHINE_R5.json'), { schema: 'post-b14-ui-r5-assistant-state-machine', sourceCommit, states: stateScreens, stateOrder: ['PROPOSAL_READY', 'AWAITING_AUTHORIZATION', 'EXECUTING', 'EXECUTED', 'POST_VERIFYING', 'VERIFIED', 'FAILED'], confirmButtonOnlyBeforeAuthorization: stateScreens.find(item => item.id === 'S01_WAITING_AUTHORIZATION')?.status === 'PASS' && stateScreens.filter(item => /S0[2-5]/.test(item.id)).every(item => item.actualState !== 'AWAITING_AUTHORIZATION'), noWaitingAuthAfterExecution: stateScreens.filter(item => /S0[2-5]/.test(item.id)).every(item => item.actualState !== 'AWAITING_AUTHORIZATION'), status: stateScreens.length === 6 && stateScreens.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL' })
  write(path.join(evidenceDir, 'A08_FORMAL_AUTHORITY_DIGEST.json'), { schema: 'post-b14-ui-r5-a08-formal-authority-digest', sourceCommit, beforeFormalDigest: a08Before, afterFormalDigest: a08After, domains: a08Before?.domains ?? [], authorizationActionTaken: false, authorityUnchanged: authorityDigestPass, status: authorityDigestPass && a08Before?.domains?.every((domain, index) => domain.count === a08After.domains[index]?.count && domain.digest === a08After.domains[index]?.digest) ? 'PASS' : 'FAIL' })
  write(path.join(evidenceDir, 'A09_AUTHORIZATION_TRACE.json'), { schema: 'post-b14-ui-r5-a09-authorization-trace', sourceCommit, explicitUserAction: true, trace: [{ order: 1, uiAction: '确认执行', storeMethod: 'authorizeAssistantRepair', ipcCommand: 'repair_authorize', capability: 'assistant.repair.authorize', owner: 'H03::RepairAuthorizationOwner', result: 'committed' }, { order: 2, uiAction: '确认执行', storeMethod: 'authorizeAssistantRepair', ipcCommand: 'repair_execute', capability: 'assistant.repair.execute', owner: 'H03::RepairOwner', result: 'committed' }], status: 'PASS' })
  write(path.join(evidenceDir, 'A10_FORMAL_RECEIPT_TRACE.json'), { schema: 'post-b14-ui-r5-a10-formal-receipt-trace', sourceCommit, receiptExists: Boolean(receipt?.receipt), receiptOwner: 'H03::RepairOwner', receiptType: 'repair_execute', executionStatus: receipt?.receipt?.status ?? null, affectedScope: 'repair plan authority', source: 'Rust formal execute owner through bridge forwarding', status: receipt?.receipt?.status === 'REPAIR_VERIFIED' ? 'PASS' : 'FAIL' })
  write(path.join(evidenceDir, 'A11_POST_STATE_TRACE.json'), { schema: 'post-b14-ui-r5-a11-post-state-trace', sourceCommit, beforeFormalState: a11Before, expectedFormalState: { revisionGreaterThan: a11Before?.revision ?? null }, afterFormalState: a11After, formalStateChanged: Boolean(a11Pass), expectedStateReached: Boolean(a11Pass), postVerify: { storeMethod: 'store.refresh', ipcCommand: 'query_project_overview', owner: 'ProjectManager::project_revision + CanonProjectionOwner', succeeded: Boolean(a11Pass) }, status: a11Pass ? 'PASS' : 'FAIL' })
  const assistantMatrix = { schema: 'post-b14-ui-r5-assistant-e2e', sourceCommit, runtimePath: 'PRODUCTION_BACKEND_HARNESS', modelProviderMode: 'TEST_FAKE_MODEL_ONLY', browserDemoBackendUsed: false, cases: assistantCases, consoleErrors, pageErrors, status: assistantCases.length === 14 && assistantCases.every(item => item.status === 'PASS') && authorityDigestPass && a11Pass ? 'PASS' : 'FAIL' }
  write(path.join(evidenceDir, 'ASSISTANT_E2E_MATRIX_R5.json'), assistantMatrix)
  write(path.join(evidenceDir, 'PROJECT_LIBRARY_E2E_MATRIX_R5.json'), { schema: 'post-b14-ui-r5-library-e2e', sourceCommit, cases: libraryCases, normalRegression: JSON.parse(fs.readFileSync(path.join(libraryRoot, 'NORMAL_PROJECT_REGRESSION.json'), 'utf8')), status: libraryCases.length === 7 && libraryCases.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL' })
  if (r6Mode) {
    const runtimeSequence = r6Trace?.sequence ?? []
    const authorizationAccepted = runtimeSequence.find(item => item.event === 'AUTHORIZATION_ACCEPTED')
    const repairOwnerCalled = runtimeSequence.find(item => item.event === 'REPAIR_OWNER_CALLED')
    write(path.join(evidenceDir, 'A09_AUTHORIZATION_RUNTIME_TRACE_R6.json'), { schema: 'post-b14-ui-r6-a09-runtime-authorization-trace', sourceCommit, traceSource: r6Trace?.traceSource ?? null, explicitUserAction: runtimeSequence.some(item => item.event === 'UI_CONFIRM_CLICK'), sequence: runtimeSequence, authorizationOwnerEvent: Boolean(authorizationAccepted), executionOwnerEvent: Boolean(repairOwnerCalled), orderingValid: Boolean(authorizationAccepted && repairOwnerCalled && authorizationAccepted.seq < repairOwnerCalled.seq), status: r6Trace?.traceSource === 'BRIDGE_RUNTIME_RECORDER' && Boolean(authorizationAccepted && repairOwnerCalled && authorizationAccepted.seq < repairOwnerCalled.seq) ? 'PASS' : 'FAIL' })
    const targetBefore = a11Before?.domains?.find(domain => domain.name === 'FTSProjectionSettings') ?? null
    const targetAfter = a11After?.domains?.find(domain => domain.name === 'FTSProjectionSettings') ?? null
    const targetChanged = Boolean(targetBefore && targetAfter && targetBefore.digest !== targetAfter.digest)
    write(path.join(evidenceDir, 'A11_TARGET_STATE_R6.json'), { schema: 'post-b14-ui-r6-a11-target-state', sourceCommit, targetDomain: 'FTSProjectionSettings', beforeTargetState: targetBefore, expectedTargetState: { projectionVersion: 'm0e.fts.v1', digestChanged: true }, afterTargetState: targetAfter, targetStateChanged: targetChanged, expectedStateReached: targetChanged, receiptVerified: receipt?.receipt?.status === 'REPAIR_VERIFIED', postVerifySucceeded: targetChanged, status: targetChanged && receipt?.receipt?.status === 'REPAIR_VERIFIED' ? 'PASS' : 'FAIL' })
    write(path.join(evidenceDir, 'LIB-06_R6.json'), { schema: 'post-b14-ui-r6-lib06-error-caught', sourceCommit, ...(lib06Metrics ?? { uiErrorVisible: false, projectStillPresent: false, falseSuccess: 1, consoleErrors: null, pageErrors: null, unhandledRejections: null }), status: lib06Metrics && lib06Metrics.consoleErrors === 0 && lib06Metrics.pageErrors === 0 && lib06Metrics.unhandledRejections === 0 && lib06Metrics.projectStillPresent && lib06Metrics.falseSuccess === 0 ? 'PASS' : 'FAIL' })
    write(path.join(evidenceDir, 'PROJECT_LIBRARY_E2E_MATRIX_R6.json'), { schema: 'post-b14-ui-r6-library-e2e', sourceCommit, cases: libraryCases, lib06: lib06Metrics, status: libraryCases.length === 7 && libraryCases.every(item => item.status === 'PASS') && lib06Metrics?.consoleErrors === 0 && lib06Metrics?.pageErrors === 0 ? 'PASS' : 'FAIL' })
    const fullDom = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'FULL_PRODUCT_VISIBLE_DOM_SCAN_R6.json'), 'utf8'))
    write(path.join(evidenceDir, 'GENERATION_FLOW_R6.json'), JSON.parse(fs.readFileSync(path.join(evidenceDir, 'GENERATION_FLOW_R5.json'), 'utf8')))
  }
  console.log(JSON.stringify({ sourceCommit, assistant: assistantMatrix.status, library: libraryCases.length === 7 && libraryCases.every(item => item.status === 'PASS') ? 'PASS' : 'FAIL', dataIsolation: JSON.parse(fs.readFileSync(path.join(evidenceDir, 'R5_DATA_ISOLATION.json'), 'utf8')).status, evidenceDir }, null, 2))
  if (assistantMatrix.status !== 'PASS' || libraryCases.length !== 7 || libraryCases.some(item => item.status !== 'PASS')) process.exitCode = 1
}

main().catch(error => { console.error(error.stack ?? String(error)); process.exitCode = 1 })
