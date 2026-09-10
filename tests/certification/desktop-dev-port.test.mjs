import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '../..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('desktop dev server and Tauri devUrl share the clean port 1450', () => {
  assert.match(read('vite.config.ts'), /port:\s*1450/)
  assert.match(read('src-tauri/tauri.conf.json'), /"devUrl"\s*:\s*"http:\/\/localhost:1450"/)
})
