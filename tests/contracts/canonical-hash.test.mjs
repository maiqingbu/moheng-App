import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

import {
  canonicalize,
  normalizeJsonNewlines,
  sha256Canonical,
} from '../../contracts/tools/canonical-hash.mjs'

const vectors = JSON.parse(
  fs.readFileSync(new URL('../../contracts/fixtures/golden/jcs-vectors.json', import.meta.url), 'utf8'),
)

test('should normalize CRLF and CR before hashing payload text', () => {
  assert.deepEqual(normalizeJsonNewlines({ text: 'a\r\nb\rc' }), { text: 'a\nb\nc' })
  assert.equal(
    sha256Canonical({ text: 'a\r\nb\rc' }),
    sha256Canonical({ text: 'a\nb\nc' }),
  )
})

test('official sha256Canonical defaults to newline normalization (TS/Rust parity)', () => {
  // The official artifact hash API must normalize CRLF/CR -> LF by default,
  // so a caller who forgets the option never drifts from the Rust kernel.
  assert.equal(
    sha256Canonical({ text: 'a\r\nb\rc' }),
    sha256Canonical({ text: 'a\nb\nc' }),
  )
})

test('should serialize RFC 8785 numeric and UTF-16 ordering edge cases', () => {
  assert.equal(canonicalize({ '\u{1f600}': 1, '\ue000': 2 }), '{"😀":1,"":2}')
  assert.equal(canonicalize({ values: [1e30, 4.5, 0.000002, 1e-7, -0] }), '{"values":[1e+30,4.5,0.000002,1e-7,0]}')
})

test('should reject lone Unicode surrogates', () => {
  assert.throws(() => canonicalize({ text: '\ud800' }), /Unicode scalar/)
})

test('should verify at least twenty frozen golden vectors', () => {
  assert.ok(vectors.length >= 20)
  for (const vector of vectors) {
    assert.equal(
      sha256Canonical(vector.value, { normalizeNewlines: vector.normalizeNewlines === true }),
      vector.canonicalSha256,
      vector.name,
    )
  }
})
