import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { delimiter, resolve } from 'node:path'

const root = resolve(decodeURIComponent(new URL('..', import.meta.url).pathname))
const evidenceDir = resolve(process.env.B03_EVIDENCE_DIR ?? resolve(root, 'evidence/B03'))
const targets = [evidenceDir]
if (process.env.B03_SCAN_PATHS) {
  targets.push(...process.env.B03_SCAN_PATHS.split(delimiter).filter(Boolean).map(resolve))
}
const forbidden = [/sk-secret-b03/g, /Authorization:\s*Bearer\s+\S+/gi]
const hits = []

function visit(path) {
  if (!existsSync(path)) return
  if (statSync(path).isDirectory()) {
    for (const name of readdirSync(path)) visit(resolve(path, name))
    return
  }
  const text = readFileSync(path, 'utf8')
  for (const pattern of forbidden) {
    if (pattern.test(text)) hits.push(path)
    pattern.lastIndex = 0
  }
}

for (const target of targets) visit(target)
console.log(`B03 secret scan: ${hits.length === 0 ? 'PASS' : 'FAIL'} paths=${targets.join(',')}`)
if (hits.length > 0) {
  console.error([...new Set(hits)].join('\n'))
  process.exitCode = 1
}
