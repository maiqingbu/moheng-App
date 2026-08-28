import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
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

test('B05 frontend narrative projections use the shared DTO fixture boundary', () => {
  const thread = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/valid/narrative-continuity-thread-projection.json`, 'utf8'))
  const progress = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/valid/narrative-volume-progress-projection.json`, 'utf8'))
  assert.equal(thread.authorityLevel, 'B')
  assert.equal(thread.status, 'OPEN')
  assert.deepEqual(progress.activeTaskIds, ['task:1'])
  assert.equal(progress.remainingChapterCapacity, 10)
})

test('B05 BookPlan fixture distinguishes SEED from APPROVED authority', () => {
  const seed = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/valid/narrative-book-plan-r1.json`, 'utf8'))
  const approved = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/valid/narrative-book-plan-r1-approved.json`, 'utf8'))
  const invalidApproved = JSON.parse(fs.readFileSync(`${root}contracts/fixtures/invalid/narrative-book-plan-r1-approved-missing-reservations.json`, 'utf8'))
  assert.equal(seed.planningMaturity, 'SEED')
  assert.deepEqual(seed.openingHypotheses, [])
  assert.equal('futureReservations' in seed, false)
  assert.equal(approved.planningMaturity, 'APPROVED')
  for (const field of ['mustAchieve', 'relationshipMacroDirections', 'majorMysteryGoals', 'futureReservations', 'volumeIntentSeeds']) assert.ok(field in approved, field)
  assert.equal(invalidApproved.planningMaturity, 'APPROVED')
  assert.equal('futureReservations' in invalidApproved, false)
  const schema = fs.readFileSync(`${root}contracts/schemas/narrative/book-plan-r1.schema.json`, 'utf8')
  assert.match(schema, /globalLengthBudget is the canonical serialized representation of targetLengthRange/)
})
