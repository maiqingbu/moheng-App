import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = file => fs.readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')
const commands = read('src-tauri/src/commands.rs')
const production = read('src-tauri/src/production.rs')
const main = read('src-tauri/src/main.rs')
const provider = read('src-tauri/src/provider.rs')
const runtime = read('src-tauri/src/repositories/runtime.rs')
const settings = read('src/ui/pages/SettingsPage.vue')
const contracts = read('src/application/contracts.ts')

test('P01 production composition binds configured provider gateway', () => {
  assert.match(commands, /ConfiguredProductionGateway::for_desktop/)
  assert.doesNotMatch(commands, /UnavailableProductionGateway/)
  assert.doesNotMatch(main, /UnavailableProductionGateway/)
  assert.match(production, /impl ProductionModelGateway for ConfiguredProductionGateway/)
  assert.match(production, /DurableModelRunner/)
  assert.match(provider, /impl ProviderAdapter for OpenAiCompatibleProvider/)
})

test('P01 settings and secret boundary are explicit', () => {
  assert.match(commands, /ProviderSettingsRepository::read_global/)
  assert.match(commands, /ProviderSettingsRepository::write_project/)
  assert.match(commands, /OsKeychainSecretStore/)
  assert.match(runtime, /pub trait SecretStore: Send \+ Sync/)
  assert.match(settings, /autocomplete="new-password"/)
  assert.match(settings, /apiKey:apiKey\.value/)
  assert.doesNotMatch(contracts, /interface SettingsProjection \{[^}]*apiKey\??:/)
  assert.match(contracts, /interface ProviderSettingsInput[\s\S]*apiKey\??:/)
})

test('P01 provider errors and capability truthfulness are typed', () => {
  for (const code of ['AUTHENTICATION_ERROR', 'RATE_LIMITED', 'TIMEOUT', 'TRANSPORT_ERROR', 'PROVIDER_REJECTED', 'MALFORMED_RESPONSE', 'UNKNOWN_REMOTE_OUTCOME']) {
    assert.match(provider, new RegExp(`"${code}"`))
  }
  assert.doesNotMatch(commands, /IPC_UNSUPPORTED:test_provider/)
  assert.match(commands, /P01_PROVIDER_NOT_CONFIGURED/)
})
