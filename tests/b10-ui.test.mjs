import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const page = readFileSync(resolve(root, 'src/ui/pages/ProductionCenter.vue'), 'utf8')

test('B10 ProductionCenter renders backend Skill and Quality truth without a local fallback', () => {
  assert.match(page, /creativeSkills/)
  assert.match(page, /quality/)
  assert.match(page, /ReaderDesire/)
  assert.match(page, /skillsEnabled:\s*true/)
  assert.match(page, /SOFT 质量评估/)
  assert.match(page, /HARD Gate/)
  assert.doesNotMatch(page, /VITE_BROWSER_DEMO|demoPort|mockProduction|localQuality/)
})
