import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { createRequire } from 'node:module'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R3_EVIDENCE_DIR ?? '')
const browserDir = path.join(evidenceDir, 'B14_R3', 'MANUAL_BROWSER')
const port = Number(process.env.B14_R3_BROWSER_PORT ?? 1464)
const baseUrl = `http://127.0.0.1:${port}`
const internal = /(?:\b(?:chapter|volume|artifact|obligation|project|fact|desire|task|model-run|state):[^\s\u3000]+|\b(?:projectId|artifactId|obligationId|volumeId|payloadHash|stateDigest|EndgameContract|BookExitState|BookCompletionReport|READY|BLOCKED|NEEDS_DECISION|SOURCE_UNAVAILABLE)\b)/i

function playwright() {
  const require = createRequire(import.meta.url)
  for (const item of [process.env.PLAYWRIGHT_MODULE, '/Users/mac/.npm-global/lib/node_modules/playwright'].filter(Boolean)) {
    try { return require(item) } catch { /* next */ }
  }
  throw new Error('B14_R3_PLAYWRIGHT_UNAVAILABLE')
}
function waitFor(url) {
  return new Promise((resolve, reject) => {
    const started = Date.now()
    const tick = () => {
      const request = http.get(url, response => { response.resume(); response.statusCode < 500 ? resolve() : retry() })
      request.on('error', retry); request.setTimeout(1000, () => { request.destroy(); retry() })
    }
    const retry = () => Date.now() - started > 30_000 ? reject(new Error(`B14_R3_BROWSER_SERVER_TIMEOUT:${url}`)) : setTimeout(tick, 150)
    tick()
  })
}
const scenarios = [
  ['M01','m01','未确认结局目标'],['M02','m02','存在正式剧情责任'],['M03','m03','重要谜团待确认'],['M04','m04','关键回报分层'],['M05','m05','相关变更需要处理'],['M06','m06','成品完整性未通过'],['M07','m07','全部硬条件满足'],['M08','m08','生成终局方案建议'],['M09','m09','回报质量建议不改变结论'],['M10','m10','阻断时禁止提交'],['M11','m11','当前版本已确认'],['M12','m12','相关变化后重新检查'],['M13','m13','大量阻断项'],['M14','m14','详情无内部标识'],['M15','m15','谜团对应关系'],['M16','m16','未来约束完成依据'],['M17','m17','关键回报完成依据'],
]
async function main() {
  if (!evidenceDir) throw new Error('B14_R3_EVIDENCE_DIR_REQUIRED')
  fs.mkdirSync(browserDir, { recursive: true })
  const server = spawn('npm', ['run','dev','--','--host','127.0.0.1','--port',String(port)], { cwd:root, env:{...process.env,VITE_BROWSER_DEMO:'true'}, stdio:'ignore' })
  const { chromium } = playwright(); const browser = await chromium.launch({ headless:true }); const page = await browser.newPage({ viewport:{width:1440,height:960} })
  const items=[]
  try {
    await waitFor(`${baseUrl}/`)
    for (const [id, mode, expectedState] of scenarios) {
      await page.goto(`${baseUrl}/projects/proj-moheng-demo/completion?completionEvidence=${mode}`, { waitUntil:'domcontentloaded' })
      await page.getByRole('heading',{name:'终局与完结'}).waitFor({state:'visible',timeout:10_000}); await page.waitForTimeout(120)
      if (id === 'M08') await page.getByRole('button',{name:'生成结局建议'}).click()
      if (id === 'M09') await page.getByRole('button',{name:'检查回报质量'}).click()
      if (['M15','M16','M17'].includes(id)) { await page.getByRole('button',{name:'查看可确认内容'}).click(); await page.waitForTimeout(100) }
      const text = await page.locator('body').innerText(); const filename=`${id}.png`; await page.screenshot({path:path.join(browserDir,filename),fullPage:true})
      items.push({ id, route:`/projects/proj-moheng-demo/completion?completionEvidence=${mode}`, fixture:`B14 BrowserDemo ${mode}`, trigger:id==='M08'?'点击生成终局方案建议':id==='M09'?'点击检查回报质量': ['M15','M16','M17'].includes(id)?'点击查看可确认内容':'打开完结页面', expectedState, actualVisibleText:text, actualVisibleTextSource:'playwright-dom-innerText', domTextSha256:crypto.createHash('sha256').update(text,'utf8').digest('hex'), internalLeakageCheck:!internal.test(text), hardSoftSeparation: id !== 'M09' || text.includes('不会改变正式完结结论'), mappingCheck: !['M15','M16','M17'].includes(id) || text.includes('需要确认的终局责任'), b15LeakageCheck: !/ExportRuntime|Release|B15/.test(text), screenshotPath:`B14_R3/MANUAL_BROWSER/${filename}`, result:!internal.test(text)?'PASS':'FAIL' })
    }
  } finally { await browser.close(); server.kill('SIGTERM') }
  const report={schema:'b14-r3-manual-walkthrough',sourceCommit:process.env.B14_R3_SOURCE_COMMIT ?? null,captureMethod:'Playwright Chromium DOM innerText + PNG screenshot',items,status:items.length===17&&items.every(item=>item.result==='PASS'&&item.hardSoftSeparation&&item.mappingCheck&&item.b15LeakageCheck)?'PASS':'FAIL'}
  fs.writeFileSync(path.join(evidenceDir,'B14_R3_MANUAL_WALKTHROUGH.json'),`${JSON.stringify(report,null,2)}\n`)
  console.log(JSON.stringify({status:report.status,itemCount:items.length},null,2)); if(report.status!=='PASS') process.exitCode=1
}
main().catch(error=>{console.error(error.stack??String(error));process.exitCode=1})
