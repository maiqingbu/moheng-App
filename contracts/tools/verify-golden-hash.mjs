import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { sha256Canonical } from './canonical-hash.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const vectors = JSON.parse(
  fs.readFileSync(path.join(root, 'contracts/fixtures/golden/jcs-vectors.json'), 'utf8'),
)
if (vectors.length < 20) throw new Error(`Expected at least 20 golden vectors, received ${vectors.length}`)
for (const vector of vectors) {
  const actual = sha256Canonical(vector.value, {
    normalizeNewlines: vector.normalizeNewlines === true,
  })
  if (actual !== vector.canonicalSha256) {
    throw new Error(`Golden fixture mismatch for ${vector.name}: expected ${vector.canonicalSha256}, received ${actual}`)
  }
}
console.log(`Golden hash verified: ${vectors.length} RFC 8785/newline vectors`)
