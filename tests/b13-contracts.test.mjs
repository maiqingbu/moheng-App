import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const readJson = relative => JSON.parse(fs.readFileSync(path.join(root, relative), 'utf8'))

test('B13 registers the additive LongFormHealthProjectionR1 contract', () => {
  const manifest = readJson('contracts/manifests/schema-manifest.json')
  const entry = manifest.schemas.find(item => item.name === 'long-form-health-projection-r1')
  assert.ok(entry)
  assert.equal(entry.path, 'schemas/health/long-form-health-projection-r1.schema.json')
  const schema = readJson(`contracts/${entry.path}`)
  assert.equal(schema.title, 'LongFormHealthProjectionR1')
  assert.deepEqual(schema.properties.payloadSchema.const, 'long-form-health-projection.r1')
  assert.deepEqual(schema.properties.factors.items.$ref, '#/$defs/HealthFactor')
  assert.deepEqual(schema.properties.coverage.properties.totalFactors.const, 8)
  for (const field of ['projectId', 'projectRevision', 'checkedAt', 'score', 'level', 'coverage', 'factors', 'issues', 'counters', 'provenance']) {
    assert.ok(schema.required.includes(field), `missing ${field}`)
  }
})

test('B13 valid fixture covers all eight frozen factor codes', () => {
  const fixture = readJson('contracts/fixtures/valid/long-form-health-projection-r1.json')
  assert.equal(fixture.payloadSchema, 'long-form-health-projection.r1')
  assert.equal(fixture.coverage.totalFactors, 8)
  assert.equal(fixture.factors.length, 8)
  assert.deepEqual(fixture.factors.map(item => item.factorCode), [
    'OBLIGATION_OVERDUE', 'FORESHADOW_AGING', 'ROUTE_DRIFT', 'TASK_BLOCKAGE',
    'CONTEXT_GROWTH', 'MODEL_COST_DRIFT', 'READER_DESIRE', 'QUALITY_DRIFT',
  ])
  assert.ok(fixture.issues.items.length <= 50)
})
