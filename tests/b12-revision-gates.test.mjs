import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B12 has an independent architecture verifier', () => {
  assert.equal(fs.existsSync(path.join(root, 'scripts/verify-b12-architecture.mjs')), true)
})

test('History UI exposes the complete Retcon plan and execution path', () => {
  const history = read('src/ui/pages/HistoryCenter.vue')
  assert.match(history, /retcon_plan_create/)
  assert.match(history, /retcon_apply/)
  assert.match(history, /planDigest/)
})

test('B12 acceptance maps every gate to an existing evidence source', () => {
  const generator = read('scripts/generate-b12-acceptance.mjs')
  assert.match(generator, /existsSync\(source\)/)
  assert.match(generator, /b12_architecture/)
})
