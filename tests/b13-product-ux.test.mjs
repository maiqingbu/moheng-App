import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B13 health UI presents deterministic factors and soft advice in Chinese', () => {
  const page = read('src/ui/pages/LongFormHealth.vue')
  for (const label of ['长篇健康', '需要关注', '重新检查', '数据积累中', '暂时无法完成这一项检查']) assert.match(page, new RegExp(label))
  for (const token of ['OBLIGATION_OVERDUE', 'FORESHADOW_AGING', 'ROUTE_DRIFT', 'TASK_BLOCKAGE', 'CONTEXT_GROWTH', 'MODEL_COST_DRIFT', 'READER_DESIRE', 'QUALITY_DRIFT', 'LongFormHealthProjection', 'obligationId', 'payloadHash', 'Endgame', 'Completion']) {
    assert.doesNotMatch(page, new RegExp(token))
  }
})

test('B13 product UX gate scans the formal health surface', () => {
  const verifier = read('scripts/verify-product-ux.mjs')
  assert.match(verifier, /LongFormHealth\.vue/)
  assert.match(verifier, /obligationId|OBLIGATION_OVERDUE/)
  assert.match(verifier, /b13|health/i)
})
