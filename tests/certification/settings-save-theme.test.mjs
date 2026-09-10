import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const source = fs.readFileSync(new URL('../../src/ui/pages/SettingsPage.vue', import.meta.url), 'utf8')
const saveBody = source.match(/async function save\(\)\{([\s\S]*?)\n\}/)?.[1] ?? ''

test('saving provider settings does not apply an unrelated theme change', () => {
  assert.doesNotMatch(saveBody, /applyTheme\(\)/)
  assert.match(source, /form\.theme==='light'.*applyTheme\(\)/)
  assert.match(source, /form\.theme==='dark'.*applyTheme\(\)/)
})
