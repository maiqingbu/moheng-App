import crypto from 'node:crypto'

function assertUnicodeScalars(text) {
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index)
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = text.charCodeAt(index + 1)
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error('JSON strings must contain valid Unicode scalar values')
      index += 1
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      throw new Error('JSON strings must contain valid Unicode scalar values')
    }
  }
}

function assertJsonValue(value) {
  if (typeof value === 'number' && !Number.isFinite(value)) throw new Error('JSON numbers must be finite')
  if (typeof value === 'string') {
    assertUnicodeScalars(value)
    return
  }
  if (typeof value === 'number' || typeof value === 'boolean' || value === null) return
  if (Array.isArray(value)) {
    value.forEach(assertJsonValue)
    return
  }
  if (typeof value === 'object') {
    for (const [key, child] of Object.entries(value)) {
      assertUnicodeScalars(key)
      assertJsonValue(child)
    }
    return
  }
  throw new Error('Unsupported JSON value')
}

export function normalizeJsonNewlines(value) {
  if (typeof value === 'string') return value.replace(/\r\n?/g, '\n')
  if (Array.isArray(value)) return value.map(normalizeJsonNewlines)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, normalizeJsonNewlines(child)]))
  }
  return value
}

function canonicalizeValue(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalizeValue).join(',')}]`
  if (value !== null && typeof value === 'object') {
    const keys = Object.keys(value).sort()
    return `{${keys.map(key => `${JSON.stringify(key)}:${canonicalizeValue(value[key])}`).join(',')}}`
  }
  return JSON.stringify(value)
}

export function canonicalize(value, { normalizeNewlines = false } = {}) {
  const normalized = normalizeNewlines ? normalizeJsonNewlines(value) : value
  assertJsonValue(normalized)
  return canonicalizeValue(normalized)
}

export function sha256Canonical(value, options) {
  return crypto.createHash('sha256').update(Buffer.from(canonicalize(value, options), 'utf8')).digest('hex')
}
