import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import test from 'node:test'

const root = resolve(import.meta.dirname, '../..')
const resourceRoot = resolve(root, 'contracts/resources/signing-eval.r1')
const core = JSON.parse(readFileSync(resolve(resourceRoot, 'core.json'), 'utf8'))
const mappings = JSON.parse(readFileSync(resolve(resourceRoot, 'fixtures/rejection-mappings.json'), 'utf8'))
const outputSchema = JSON.parse(readFileSync(resolve(resourceRoot, 'model-output.schema.json'), 'utf8'))

test('signing rejection notices map every reason to a defined evaluation item', () => {
  const coreIds = new Set(core.levels.flatMap(level => level.items?.map(item => item.itemId) ?? []))
  const mapped = mappings.fixtures.flatMap(fixture => fixture.reasons)
  assert.equal(mappings.fixtures.length, 2)
  assert.equal(mapped.length, 7)
  for (const reason of mapped) {
    assert.ok(coreIds.has(reason.itemId), `${reason.itemId} must exist in signing-eval.r1`)
  }
  assert.ok(mapped.some(reason => reason.itemId === 'R2-5'))
  assert.ok(mapped.some(reason => reason.itemId === 'E3-5'))
})

test('signing rules keep AI declaration advisory and forbid detector evasion', () => {
  assert.equal(core.decisions.declarationPromptOnly, 'C0-4')
  assert.equal(core.decisions.aiDetectionEvasion, false)
  assert.equal(core.decisions.automaticQuoteCorrection, false)
})

test('S35 signing output schema requires exact quote spans per evaluation item', () => {
  const item = outputSchema.properties.payload.properties.signingEvaluationItems.items
  assert.deepEqual(item.required, ['itemId', 'result', 'reason', 'evidence'])
  assert.deepEqual(item.properties.result.enum, ['PASS', 'FAIL', 'NOT_VERIFIED'])
  assert.deepEqual(item.properties.evidence.items.required, ['chapterNo', 'rangeStart', 'rangeEnd', 'exactQuote'])
})
