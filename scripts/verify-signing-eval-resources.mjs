import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const resourceDir = join(root, 'contracts/resources/signing-eval.r1')
const manifestPath = join(resourceDir, 'resource-manifest.json')
const paths = [
  'core.json',
  'model-output.schema.json',
  'fixtures/rejection-mappings.json',
  'platforms/FANQIE.json',
  'platforms/QIDIAN.json',
  'platforms/QIMAO.json',
  'platforms/ZONGHENG.json',
  'platforms/JJWXC.json',
]
const files = Object.fromEntries(paths.map(path => [
  path,
  (() => {
    const bytes = readFileSync(join(resourceDir, path))
    JSON.parse(bytes.toString('utf8'))
    return createHash('sha256').update(bytes).digest('hex')
  })(),
]))
const manifest = { version: 'signing-eval.r1', files }

if (process.argv.includes('--write')) {
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  process.stdout.write('signing-eval.r1 resource manifest written\n')
} else {
  const actual = JSON.parse(readFileSync(manifestPath, 'utf8'))
  if (JSON.stringify(actual) !== JSON.stringify(manifest)) {
    process.stderr.write('signing-eval.r1 resource digest mismatch\n')
    process.exitCode = 1
  } else {
    process.stdout.write('signing-eval.r1 resource digests verified\n')
  }
}
