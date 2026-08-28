import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

const root = new URL('..', import.meta.url).pathname
const manifest = JSON.parse(fs.readFileSync(`${root}contracts/manifests/schema-manifest.json`, 'utf8'))

test('B05 manifest registers all R1 payload contracts', () => {
  const names = new Set(manifest.schemas.map(schema => schema.name))
  for (const name of ['book-plan-r1', 'volume-intent-r1', 'volume-plan-r1', 'context-manifest-r1', 'chapter-plan-r1', 'chapter-exit-state-r1', 'volume-exit-state-r1']) {
    assert.ok(names.has(name), name)
  }
})

test('B05 invalid narrative fixtures are not accepted by the R1 schema boundary', () => {
  const invalid = fs.readdirSync(`${root}contracts/fixtures/invalid`).filter(name => name.startsWith('narrative-'))
  assert.ok(invalid.length >= 7)
  for (const file of invalid) {
    const value = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/invalid/${file}`, 'utf8'))
    assert.notEqual(value.payloadSchema, undefined, file)
  }
})
