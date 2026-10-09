import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const tauriConfig = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
const marketSource = await readFile(new URL('../src-tauri/src/market_source.rs', import.meta.url), 'utf8')

function directives(policy) {
  return Object.fromEntries(policy.split(';').map(part => {
    const [name, ...values] = part.trim().split(/\s+/)
    return [name, values]
  }))
}

test('Tauri production CSP allows only app assets, IPC, and required inline styles', async () => {
  const policy = tauriConfig.app.security.csp
  assert.equal(typeof policy, 'string')
  const csp = directives(policy)

  assert.deepEqual(csp['default-src'], ["'self'", 'customprotocol:', 'asset:'])
  assert.deepEqual(csp['script-src'], ["'self'"])
  assert.ok(csp['style-src'].includes("'self'"))
  assert.ok(csp['style-src'].includes("'unsafe-inline'"))
  assert.deepEqual(csp['connect-src'], ['ipc:', 'http://ipc.localhost'])
  assert.ok(csp['object-src'].includes("'none'"))
  assert.ok(csp['base-uri'].includes("'self'"))
  assert.ok(csp['form-action'].includes("'self'"))
  assert.doesNotMatch(policy, /unsafe-eval|script-src[^;]*(https?:|data:|\*)/i)

  const devPolicy = tauriConfig.app.security.devCsp
  assert.equal(typeof devPolicy, 'string')
  const dev = directives(devPolicy)
  assert.ok(dev['script-src'].includes('http://localhost:1450'))
  assert.ok(dev['connect-src'].includes('ws://localhost:1450'))
  assert.ok(dev['connect-src'].includes('ipc:'))
  assert.doesNotMatch(devPolicy, /unsafe-eval|https:|\*\./i)
})

test('market source uses bounded HTTPS-only reqwest requests without redirects', async () => {
  assert.match(marketSource, /reqwest::blocking::Client/)
  assert.match(marketSource, /\.https_only\(true\)/)
  assert.match(marketSource, /\.redirect\(reqwest::redirect::Policy::none\(\)\)/)
  assert.match(marketSource, /\.connect_timeout\(Duration::from_secs\(4\)\)/)
  assert.match(marketSource, /\.timeout\(Duration::from_secs\(10\)\)/)
  assert.match(marketSource, /take\(\(MAX_SOURCE_BYTES \+ 1\) as u64\)/)
  assert.doesNotMatch(marketSource, /Command::new\(["']curl["']\)/)
})
