import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B01 production topology is bounded to the golden three and never a test SQL fixture', () => {
  const repository = read('src-tauri/src/repositories/project_entry.rs')
  const production = read('src-tauri/src/production.rs')
  const productionRepository = read('src-tauri/src/repositories/production.rs')
  const integration = read('src-tauri/tests/b01_opening_lifecycle.rs')
  const realHarness = read('src-tauri/tests/real_provider_three_chapters.rs')
  assert.match(repository, /OPENING_CHAPTER_COUNT\s*:\s*i64\s*=\s*3/)
  assert.match(repository, /for chapter_no in 1\.\.=OPENING_CHAPTER_COUNT/)
  assert.match(production, /B08_OPENING_ORDER_REQUIRED/)
  assert.match(productionRepository, /return "pending"/)
  assert.doesNotMatch(productionRepository, /else if status=="FAILED" \{0\} else \{50\}/)
  assert.doesNotMatch(integration, /INSERT\s+(?:OR\s+IGNORE\s+)?INTO\s+chapters/i)
  assert.doesNotMatch(integration, /INSERT\s+(?:OR\s+IGNORE\s+)?INTO\s+chapter_production_budgets/i)
  assert.doesNotMatch(realHarness, /INSERT\s+(?:OR\s+IGNORE\s+)?INTO\s+chapters/i)
  assert.doesNotMatch(realHarness, /INSERT\s+(?:OR\s+IGNORE\s+)?INTO\s+chapter_production_budgets/i)
})

test('B01 UI exposes authoritative progress and a runtime phase animation without locking navigation', () => {
  const opening = read('src/ui/pages/OpeningReady.vue')
  const production = read('src/ui/pages/ProductionCenter.vue')
  const styles = read('src/ui/styles/pages.css')
  assert.match(opening, /openingProgress/)
  assert.match(production, /openingProgress/)
  assert.match(production, /production\.timeline/)
  assert.doesNotMatch(opening, /:disabled=/)
  assert.match(production, /COMMIT: '正式提交'/)
  assert.match(production, /RECONCILE: '状态回写与后续判断'/)
  assert.match(styles, /opening-phase-pulse/)
  assert.doesNotMatch(`${opening}\n${production}`, /pointer-events\s*:\s*none/i)
  assert.doesNotMatch(`${opening}\n${production}`, /cursor\s*:\s*(?:wait|progress)/i)
  assert.doesNotMatch(`${opening}\n${production}`, /document\.(body|documentElement)\.style/i)
})
