import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('B13 owns health computation in a projection-only service and repository', () => {
  const health = read('src-tauri/src/health/mod.rs')
  const repository = read('src-tauri/src/repositories/health.rs')
  assert.match(health, /LongFormHealthProjection|DETERMINISTIC_RUNTIME|SOFT_ADVISORY/)
  assert.match(repository, /HealthRepository|LIMIT/)
  assert.doesNotMatch(health, /UPDATE\s+(?:canon_|narrative_obligations|foreshadows|reader_desires)/i)
  assert.doesNotMatch(health, /model.*transport|call.*model/i)
})

test('B13 formal health route uses backend capability instead of the unavailable shell', () => {
  const router = read('src/app/router.ts')
  const healthPage = read('src/ui/pages/LongFormHealth.vue')
  assert.match(router, /path:\s*['"]\/projects\/:id\/health['"][^\n]*LongFormHealth/)
  assert.doesNotMatch(router, /health[^\n]*UnavailableFeature/)
  assert.doesNotMatch(healthPage, /终局推进|progressToEnding|targetLength|义务 ID|obligationId|dependencyType/)
})

test('B13 migration and evidence tooling are present', () => {
  assert.ok(fs.existsSync(path.join(root, 'src-tauri/migrations/0012_b13_longform_health_indexes.sql')))
  assert.ok(fs.existsSync(path.join(root, 'scripts/verify-b13-architecture.mjs')))
  assert.ok(fs.existsSync(path.join(root, 'scripts/run-b13-evidence-gates.mjs')))
  assert.ok(fs.existsSync(path.join(root, 'scripts/generate-b13-acceptance.mjs')))
})
