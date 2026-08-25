import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

const read = path => fs.readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')

test('should enforce every M0A HARD gate in CI without overwriting drift first', () => {
  const ci = read('.github/workflows/ci.yml')
  for (const command of [
    'npm run contracts:drift-check',
    'cargo fmt --manifest-path src-tauri/Cargo.toml -- --check',
    'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings',
    'cargo test --manifest-path src-tauri/Cargo.toml --test tauri_ipc_smoke',
  ]) {
    assert.ok(ci.includes(command), command)
  }
  assert.ok(ci.indexOf('contracts:drift-check') < ci.indexOf('contracts:generate'))
})

test('should inspect Rust command implementations and authoritative SQL boundaries', () => {
  const evidence = read('scripts/verify-m0a-evidence.mjs')
  const architecture = read('scripts/verify-architecture.mjs')
  assert.match(evidence, /src-tauri\/src\/commands\.rs/)
  assert.match(evidence, /IPC_UNSUPPORTED/)
  assert.match(architecture, /src-tauri/)
  assert.match(architecture, /authoritative write/)
})
