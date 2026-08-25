import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

import { generateContractSources, loadContractSchemas } from '../../contracts/tools/generate-contracts.mjs'

test('should generate every manifest schema into both languages without handwritten DTO omissions', () => {
  const { manifest, schemas } = loadContractSchemas()
  const generated = generateContractSources(manifest, schemas)

  for (const entry of manifest.schemas) {
    const title = schemas.get(entry.name).title
    assert.match(generated.typescript, new RegExp(`(?:interface|type) ${title}\\b`), entry.name)
    assert.match(generated.rust, new RegExp(`(?:struct|enum|type) ${title}\\b`), entry.name)
  }
  assert.match(generated.rust, /struct CommandEnvelope/)
  assert.match(generated.rust, /struct QueryEnvelope/)
  assert.match(generated.rust, /deny_unknown_fields/)
  assert.doesNotMatch(generated.rust, /head_updates: Option<Vec<serde_json::Value>>/)
})

test('should validate fixtures through Draft 2020-12 schemas rather than filename heuristics', () => {
  const validator = fs.readFileSync(
    new URL('../../contracts/tools/validate-fixtures.mjs', import.meta.url),
    'utf8',
  )
  assert.match(validator, /schema-manifest/)
  assert.doesNotMatch(validator, /name\.startsWith\('artifact'\)/)
})
