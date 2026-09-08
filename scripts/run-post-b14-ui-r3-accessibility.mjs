import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.POST_B14_UI_R3_ACCESSIBILITY_DIR ?? '')
const port = Number(process.env.POST_B14_UI_R3_ACCESSIBILITY_PORT ?? 1491)
const baseUrl = `http://127.0.0.1:${port}`
const sourceCommit = process.env.POST_B14_UI_R3_SOURCE_COMMIT ?? null

function playwright() { const require = createRequire(import.meta.url); return require(process.env.PLAYWRIGHT_MODULE || '/Users/mac/.npm-global/lib/node_modules/playwright') }
function waitFor(url) { const started = Date.now(); return new Promise((resolve, reject) => { const retry = () => Date.now() - started > 30_000 ? reject(new Error('ACCESSIBILITY_SERVER_TIMEOUT')) : setTimeout(check, 100); const check = () => { const request = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() }); request.on('error', retry) }; check() }) }
function write(file, value) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`) }

async function main() {
  if (!evidenceDir) throw new Error('POST_B14_UI_R3_ACCESSIBILITY_DIR_REQUIRED')
  const server = spawn('npm', ['run', 'preview', '--', '--host', '127.0.0.1', '--port', String(port)], { cwd: root, env: { ...process.env, VITE_BROWSER_DEMO: 'true' }, stdio: 'ignore' })
  const { chromium } = playwright()
  const browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({ viewport: { width: 1440, height: 840 } })
  const tests = []
  try {
    await waitFor(`${baseUrl}/`)
    await page.goto(`${baseUrl}/projects/proj-moheng-demo`, { waitUntil: 'networkidle' })
    const assistant = page.getByRole('complementary', { name: '项目助手' })
    tests.push({ id: 'INPUT_LABEL', expected: '助手输入框有可访问名称', actual: await assistant.locator('textarea').getAttribute('aria-label'), status: (await assistant.locator('textarea').getAttribute('aria-label')) ? 'PASS' : 'FAIL' })
    const collapse = page.getByRole('button', { name: '折叠助手' })
    tests.push({ id: 'ASSISTANT_TOGGLE_NAME', expected: '折叠按钮有可访问名称', actual: await collapse.getAttribute('aria-label'), status: (await collapse.getAttribute('aria-label')) === '折叠助手' ? 'PASS' : 'FAIL' })
    await page.keyboard.press('Tab')
    tests.push({ id: 'KEYBOARD_NAV', expected: 'Tab 后存在可见焦点', actual: await page.evaluate(() => ({ tag: document.activeElement?.tagName, focusVisible: document.activeElement?.matches(':focus-visible') ?? false })), status: await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.matches(':focus-visible')) ? 'PASS' : 'FAIL' })

    await page.goto(`${baseUrl}/projects`, { waitUntil: 'networkidle' })
    const copyButton = page.getByRole('button', { name: '复制' }).first()
    await copyButton.click()
    const dialog = page.getByRole('dialog')
    await dialog.waitFor({ state: 'visible' })
    const activeInsideBefore = await dialog.evaluate(node => node.contains(document.activeElement))
    for (let index = 0; index < 8; index += 1) await page.keyboard.press('Tab')
    const activeInsideAfter = await dialog.evaluate(node => node.contains(document.activeElement))
    tests.push({ id: 'DIALOG_FOCUS_TRAP', expected: '对话框内 Tab 焦点不逃逸', actual: { activeInsideBefore, activeInsideAfter }, status: activeInsideBefore && activeInsideAfter ? 'PASS' : 'FAIL' })
    await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'hidden' })
    tests.push({ id: 'ESCAPE_AND_RESTORE', expected: 'Escape 关闭并恢复触发按钮焦点', actual: await page.evaluate(() => document.activeElement?.textContent?.trim()), status: await page.evaluate(() => document.activeElement?.textContent?.trim() === '复制') ? 'PASS' : 'FAIL' })
    tests.push({ id: 'BUTTON_NAMES', expected: '正式交互按钮有 accessible name', actual: await page.locator('button').evaluateAll(buttons => buttons.filter(button => !(button.getAttribute('aria-label') || button.textContent?.trim() || button.getAttribute('title'))).length), status: await page.locator('button').evaluateAll(buttons => buttons.filter(button => !(button.getAttribute('aria-label') || button.textContent?.trim() || button.getAttribute('title'))).length === 0) ? 'PASS' : 'FAIL' })
  } finally { await browser.close(); server.kill('SIGTERM') }
  const violations = tests.filter(test => test.status !== 'PASS')
  write(path.join(evidenceDir, 'ACCESSIBILITY_SMOKE_R3.json'), { schema: 'post-b14-ui-r3-accessibility-smoke', browser: 'Chromium', sourceCommit, tests, violations, status: violations.length === 0 ? 'PASS' : 'FAIL' })
  console.log(JSON.stringify({ status: violations.length === 0 ? 'PASS' : 'FAIL', tests: tests.length, violations: violations.length }, null, 2))
  if (violations.length) process.exitCode = 1
}
main().catch(error => { console.error(error.stack ?? String(error)); process.exitCode = 1 })
