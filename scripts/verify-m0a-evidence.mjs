import fs from 'node:fs'
import path from 'node:path'
import { hasKernelStateManageCall } from './m0a-kernel-state-check.mjs'

const root = process.cwd()
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')
const errors = []
const required = [
  'src-tauri/Cargo.toml',
  'src-tauri/src/main.rs',
  'src-tauri/src/commands.rs',
  'src-tauri/tauri.conf.json',
  'src/app/providers/runtimePort.ts',
  'src/infrastructure/tauri/frontendPort.ts',
]
for (const file of required) if (!fs.existsSync(path.join(root, file))) errors.push(`missing ${file}`)

const runtime = read('src/app/providers/runtimePort.ts')
if (!runtime.includes('const invoke = getTauriInvoke()')) errors.push('runtime must inspect Tauri first')
if (runtime.indexOf('if (invoke)') > runtime.indexOf('VITE_BROWSER_DEMO')) errors.push('runtime demo branch precedes Tauri branch')
if (!runtime.includes("import.meta.env.VITE_BROWSER_DEMO === 'true'")) errors.push('demo requires explicit VITE_BROWSER_DEMO=true')
if (runtime.includes('import.meta.env.DEV')) errors.push('DEV must not implicitly enable browser demo')

const frontend = read('src/infrastructure/tauri/frontendPort.ts')
for (const marker of [
  "'query_project_library'",
  "'query_project_overview', { request }",
  "'execute_command', { request }",
]) {
  if (!frontend.includes(marker)) errors.push(`missing real frontend IPC argument chain: ${marker}`)
}

const commands = read('src-tauri/src/commands.rs')
for (const symbol of [
  'query_project_library_impl',
  'query_project_overview_impl',
  'execute_command_impl',
  'CatalogRepository',
  'ProjectManager',
]) {
  if (!commands.includes(symbol)) errors.push(`missing Rust IPC service wiring: ${symbol}`)
}
for (const forbidden of [
  'IPC_UNSUPPORTED:query_project_library',
  'IPC_UNSUPPORTED:query_project_overview',
  'IPC_UNSUPPORTED:project_create_from_idea',
  'IPC_UNSUPPORTED:project_rename',
  'IPC_UNSUPPORTED:project_archive',
]) {
  if (commands.includes(forbidden)) errors.push(`minimum IPC path is still unsupported: ${forbidden}`)
}
if (!commands.includes('IPC_UNSUPPORTED')) errors.push('unsupported non-M0A commands must fail closed')
for (const marker of ['idempotencyKey', 'commit_project_command', 'REVISION_CONFLICT', 'IDEMPOTENCY_CONFLICT']) {
  if (!commands.includes(marker)) errors.push(`missing authoritative Project Command protocol: ${marker}`)
}

const main = read('src-tauri/src/main.rs')
if (!hasKernelStateManageCall(main)) errors.push('Tauri host does not initialize KernelState')
if (!main.includes('commands::handler()')) errors.push('Tauri host does not register the command handler')

const migration = read('src-tauri/migrations/0002_project_authority.sql')
for (const table of ['authority_registry', 'table_family_registry', 'receipt_taxonomy', 'retention_policies', 'gc_receipts']) {
  if (!migration.includes(`CREATE TABLE IF NOT EXISTS ${table}`)) errors.push(`missing M0A registry/table: ${table}`)
}
const projectCommandMigration = read('src-tauri/migrations/0003_project_command.sql')
for (const table of ['project_command_receipts', 'copy_receipts']) {
  if (!projectCommandMigration.includes(`CREATE TABLE IF NOT EXISTS ${table}`)) {
    errors.push(`missing Project Command authority table: ${table}`)
  }
}

if (errors.length) {
  console.error('M0A evidence gate failed:\n- ' + errors.join('\n- '))
  process.exit(1)
}
console.log('M0A evidence gate passed: real Tauri service wiring, explicit demo isolation, authority/receipt/retention registries, and fail-closed unsupported commands verified.')
