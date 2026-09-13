import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B02 capability coverage has no exposed backend-only aliases', () => {
  const contracts = read('src/application/contracts.ts')
  const commands = read('src-tauri/src/commands.rs')
  const production = read('src/ui/pages/ProductionCenter.vue')
  const productCommands = contracts.match(/export type CommandType =([\s\S]*?)\n\nexport interface CommandRequest/)?.[1] ?? ''
  assert.match(productCommands, /'opening_evaluate'/)
  assert.match(productCommands, /'book_plan_revise'/)
  assert.doesNotMatch(productCommands, /planning_finalize_book_plan/)
  assert.doesNotMatch(productCommands, /signing_retry/)
  assert.match(commands, /planning_finalize_book_plan.*INTERNAL_COMPAT|INTERNAL_COMPAT.*planning_finalize_book_plan/s)
  assert.match(commands, /signing_retry.*INTERNAL_COMPAT|INTERNAL_COMPAT.*signing_retry/s)
  assert.equal((production.match(/type:\s*'opening_evaluate'/g) ?? []).length, 1)
  assert.equal((production.match(/type:\s*'book_plan_revise'/g) ?? []).length, 1)
})
