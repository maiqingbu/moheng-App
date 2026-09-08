import crypto from 'node:crypto'
import http from 'node:http'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const port = Number(process.env.POST_B14_UI_R4_BRIDGE_PORT ?? 1494)
const token = process.env.POST_B14_UI_R4_BRIDGE_TOKEN ?? crypto.randomBytes(18).toString('hex')
const fixtureRoot = process.env.MOHENG_R4_FIXTURE_ROOT ?? fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-r4-fixture-'))
const binary = path.join(root, 'src-tauri', 'target', 'debug', 'post-b14-ui-r4-bridge')
if (!fs.existsSync(binary)) throw new Error(`R4_BRIDGE_BINARY_MISSING:${binary}`)

const child = spawn(binary, [], { cwd: root, env: { ...process.env, MOHENG_R4_FIXTURE_ROOT: fixtureRoot }, stdio: ['pipe', 'pipe', 'pipe'] })
child.on('exit', code => { if (!server.listening) return; server.close(() => process.exit(code ?? 1)) })
let buffer = ''
let ready = null
const pending = []
child.stdout.on('data', chunk => {
  buffer += chunk.toString()
  let newline
  while ((newline = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, newline).trim()
    buffer = buffer.slice(newline + 1)
    if (!line) continue
    const value = JSON.parse(line)
    if (value.event === 'ready') { ready = value; continue }
    const next = pending.shift()
    next?.(value)
  }
})
child.stderr.on('data', chunk => process.stderr.write(chunk))
const waitReady = () => new Promise((resolve, reject) => {
  if (ready) return resolve(ready)
  const timer = setInterval(() => { if (ready) { clearInterval(timer); resolve(ready) } }, 20)
  setTimeout(() => { clearInterval(timer); reject(new Error('R4_BRIDGE_READY_TIMEOUT')) }, 30_000)
})
function invoke(command, payload) {
  return waitReady().then(() => new Promise((resolve, reject) => {
    pending.push(value => value.ok ? resolve(value.value) : reject(new Error(value.error ?? 'R4_BRIDGE_ERROR')))
    child.stdin.write(`${JSON.stringify({ command, payload })}\n`)
  }))
}
function body(request) { return new Promise((resolve, reject) => { let value = ''; request.on('data', chunk => { value += chunk }); request.on('end', () => { try { resolve(value ? JSON.parse(value) : {}) } catch (error) { reject(error) } }); request.on('error', reject) }) }
function failedCommandReceipt(input, error) {
  const request = input?.payload?.request ?? input?.payload ?? {}
  const commandId = typeof request.commandId === 'string' ? request.commandId : `R4_FAILED_${Date.now()}`
  const projectId = typeof request.projectId === 'string' ? request.projectId : undefined
  return {
    commandId,
    ...(projectId ? { projectId, resultProjectId: projectId } : {}),
    acceptedAt: new Date().toISOString(),
    finalStatus: 'failed',
    message: error instanceof Error ? error.message : String(error),
    receiptId: `failed:${commandId}`,
  }
}
const server = http.createServer(async (request, response) => {
  if (request.method === 'OPTIONS' && request.url === '/invoke') {
    response.writeHead(204, { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'POST, OPTIONS', 'access-control-allow-headers': 'content-type, x-r4-bridge-token' })
    response.end()
    return
  }
  if (request.method !== 'POST' || request.url !== '/invoke' || request.headers['x-r4-bridge-token'] !== token) { response.writeHead(404); response.end(); return }
  try {
    const input = await body(request)
    const result = await invoke(input.command, input.payload)
    response.writeHead(200, { 'content-type': 'application/json', 'access-control-allow-origin': '*' })
    response.end(JSON.stringify(result))
  } catch (error) {
    if (input?.command === 'execute_command' && (input?.payload?.request?.type || input?.payload?.type)) {
      response.writeHead(200, { 'content-type': 'application/json', 'access-control-allow-origin': '*' })
      response.end(JSON.stringify(failedCommandReceipt(input, error)))
      return
    }
    response.writeHead(500, { 'content-type': 'application/json', 'access-control-allow-origin': '*' })
    response.end(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }))
  }
})
await waitReady()
server.listen(port, '127.0.0.1', () => console.log(JSON.stringify({ bridgeType: 'R4_LOCAL_PRODUCTION_BACKEND_HARNESS', port, token, fixtureRoot, projectId: ready.projectId, testOnly: true, productionDefaultEnabled: false })))
function shutdown() { server.close(); child.kill('SIGTERM') }
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
