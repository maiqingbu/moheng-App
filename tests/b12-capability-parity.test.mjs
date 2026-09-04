import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B12 History remains active and B13 Health now uses its formal workspace', () => {
  const router = read('src/app/router.ts')
  const shell = read('src/ui/components/AppShell.vue')
  assert.match(router, /HistoryCenter/)
  assert.match(router, /path:\s*['"]\/projects\/:id\/history['"]/) 
  assert.match(router, /path:\s*['"]\/projects\/:id\/health['"].*LongFormHealth/)
  assert.match(shell, /版本与变更记录/)
  assert.match(shell, /match: 'history'/)
  assert.match(shell, /长篇健康/)
  assert.match(shell, /\/health/)
})

test('B12 has one formal Change/Retcon owner and no future batch owner', () => {
  const owner = read('src-tauri/src/change_retcon.rs')
  const commands = read('src-tauri/src/commands.rs')
  assert.match(owner, /B12::ChangeRetconOwner/)
  assert.match(commands, /crate::change_retcon::execute/)
  assert.doesNotMatch(owner, /B13|B14|B15|RC1|HealthOwner|EndgameOwner|ExportOwner/)
})

test('B12 keeps assistant and UI on execute_command rather than a second write path', () => {
  const assistant = read('src/ui/pages/ProjectAssistant.vue')
  const history = read('src/ui/pages/HistoryCenter.vue')
  assert.match(assistant, /change_request_create/)
  assert.match(history, /change_request_create/)
  assert.match(history, /impact_preflight/)
  assert.match(history, /change_request_decide/)
  assert.match(history, /change_request_apply/)
  assert.doesNotMatch(`${assistant}\n${history}`, /SELECT\s|INSERT\s|UPDATE\s|DELETE\s|rusqlite|libsql/i)
})
