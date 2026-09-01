import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = file => fs.readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')
const commands = read('src-tauri/src/commands.rs')
const production = read('src-tauri/src/production.rs')
const main = read('src-tauri/src/main.rs')
const provider = read('src-tauri/src/provider.rs')
const runtime = read('src-tauri/src/repositories/runtime.rs')
const acceptance = read('scripts/run-p01-acceptance.mjs')
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

test('P01 formal provider evidence is source-bound to raw, unknown, and budget paths', () => {
  assert.match(provider, /pub raw_response: String/)
  assert.doesNotMatch(provider, /let body_value: Value = serde_json::from_str/)
  assert.match(production, /fn invoke_typed\(/)
  assert.match(production, /invoke_typed\(self\.capability, &self\.input\)/)
  assert.match(production, /execute_production_model/)
  assert.match(production, /budget_id: Some\(production_budget_id/)
  assert.match(production, /predicted_cost_cents: gateway\.predicted_cost_cents/)
  assert.match(acceptance, /p01_formal_provider_raw_response_persists_before_model_payload_parse/)
  assert.match(acceptance, /p01_formal_configured_gateway_timeout_preserves_unknown_remote_outcome/)
  assert.match(acceptance, /p01_formal_budget_binding_blocks_provider_before_send_and_settles_success_cost/)
  assert.match(acceptance, /p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist/)
  assert.match(acceptance, /P01_FORMAL_RAW_RESPONSE\.json/)
  assert.match(acceptance, /P01_FORMAL_PROVIDER_USAGE\.json/)
  assert.match(acceptance, /READY_FOR_REACCEPTANCE/)
})
