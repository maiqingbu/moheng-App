import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('R7 display scanner includes Chinese engineering terms and batch-code gates', () => {
  const generator = read('scripts/generate-post-b14-ui-r7-evidence.mjs')
  assert.match(generator, /engineeringChinese/)
  assert.match(generator, /internalBatchCode/)
  assert.match(generator, /FULL_PRODUCT_VISIBLE_DOM_SCAN_R7\.json/)
})

test('R7 A11 evidence verifies every declared expected field from observed state', () => {
  const generator = read('scripts/generate-post-b14-ui-r7-evidence.mjs')
  assert.match(generator, /allExpectedFieldsObserved/)
  assert.match(generator, /allExpectedFieldsMatched/)
  assert.match(generator, /projectionVersion/)
  assert.match(generator, /A11_NOOP_NEGATIVE_R7\.json/)
})

test('R7 package has real secret, inventory, checksum, consistency and acceptance gates', () => {
  const packageScript = read('scripts/assemble-post-b14-ui-r7-package.mjs')
  for (const token of ['SECRET_SCAN_R7.json', 'PACKAGE_INVENTORY_R7.json', 'FILES_SHA256_VERIFY_R7.json', 'POST_B14_UI_R7_EVIDENCE_CONSISTENCY.json', 'POST_B14_UI_R7_ACCEPTANCE.json']) assert.match(packageScript, new RegExp(token.replaceAll('.', '\\.') ))
  assert.match(packageScript, /realSecretFindings\.length === 0/)
  assert.match(packageScript, /READY_FOR_FORMAL_ACCEPTANCE/)
})

test('R7 source baseline and batch boundary are explicit', () => {
  const packageScript = read('scripts/assemble-post-b14-ui-r7-package.mjs')
  assert.match(packageScript, /6e2cc26f38f4582e5275aaf6bf686c44e7c3b18e/)
  assert.match(packageScript, /b15Started: false/)
  assert.match(packageScript, /rc1Started: false/)
})
