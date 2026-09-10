import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '../..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('window controls are delegated to the native macOS titlebar', () => {
  const shell = read('src/ui/components/AppShell.vue')
  assert.doesNotMatch(shell, /handleWindowControl/)
  assert.doesNotMatch(shell, /prototype-dot/)
  assert.doesNotMatch(shell, /prototype-titlebar/)
})

test('Desktop shell uses native window decorations and system corners', () => {
  const config = read('src-tauri/tauri.conf.json')
  const styles = read('src/ui/styles/prototype-shell.css')
  assert.match(config, /"decorations"\s*:\s*true/)
  assert.doesNotMatch(config, /"macOSPrivateApi"\s*:\s*true/)
  assert.doesNotMatch(config, /"transparent"\s*:\s*true/)
  assert.doesNotMatch(styles, /\.prototype-titlebar/)
})
