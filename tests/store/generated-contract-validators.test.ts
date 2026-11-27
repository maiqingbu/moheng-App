import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import * as generated from '../../src/generated/contracts/validators'
import { validateChapterPlanR2, validateStoryBibleR1, validateStyleCardR1 } from '../../src/generated/contracts/validators'

const fixtures = resolve(dirname(fileURLToPath(import.meta.url)), '../../contracts/fixtures')
const fixture = (kind: string, name: string) => JSON.parse(fs.readFileSync(resolve(fixtures, kind, name), 'utf8'))

describe('generated contract validators', () => {
  it('should load every validator without duplicate schema id errors', () => {
    expect(typeof validateStoryBibleR1).toBe('function')
    expect(typeof validateStyleCardR1).toBe('function')
    expect(typeof validateChapterPlanR2).toBe('function')
  })

  it('should accept the valid story bible fixture', () => {
    expect(() => validateStoryBibleR1(fixture('valid', 'narrative-story-bible-r1.json'))).not.toThrow()
  })

  it('should reject the invalid story bible fixture', () => {
    expect(() => validateStoryBibleR1(fixture('invalid', 'narrative-story-bible-r1-invalid.json'))).toThrow(/SCHEMA_INVALID:StoryBibleR1/)
  })
})

describe('generated contract validators match fixture expectations', () => {
  // Definition-only schemas accept anything; validate-fixtures.mjs skips them the same way.
  const definitionsOnly = new Set(['validateNarrativeDefinitions', 'validateSharedDefinitions'])
  const validators = Object.entries(generated)
    .filter(([name]) => name.startsWith('validate') && !definitionsOnly.has(name)) as Array<[string, (value: unknown) => void]>
  const accepts = (value: unknown) => validators.filter(([, validate]) => {
    try { validate(value); return true } catch { return false }
  })
  const files = (kind: string) => fs.readdirSync(resolve(fixtures, kind)).filter(name => name.endsWith('.json'))

  it.each(files('valid'))('should accept valid fixture %s with at least one validator', name => {
    expect(accepts(fixture('valid', name)).length).toBeGreaterThan(0)
  })

  it.each(files('invalid'))('should reject invalid fixture %s with every validator', name => {
    expect(accepts(fixture('invalid', name)).map(([validatorName]) => validatorName)).toEqual([])
  })
})
