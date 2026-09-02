import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { packageGeneratorEnvironment } from './b12-package-environment.mjs'

const root = path.resolve(process.env.B12_PACKAGE_ROOT ?? process.cwd())
const forbidden = /(^|\/)(?:\.git|node_modules|target|dist)(?:\/|$)|\.db(?:$|[-.])|(^|\/)(?:\.env|keys?|secrets?)(?:$|\/)/i
const required = [
  'scripts/generate-b12-acceptance.mjs',
  'scripts/verify-b12-architecture.mjs',
  'scripts/verify-architecture.mjs',
  'scripts/run-b12-evidence-gates.mjs',
  'src-tauri/src/change_retcon.rs',
  'src-tauri/src/repositories/change_retcon.rs',
  'src-tauri/migrations/0011_b12_change_retcon_runtime.sql',
  'evidence/B12/B12_PREBATCH_FREEZE.json',
  'evidence/B12/gate-results.json',
]
const entries = []
const walk = directory => {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name)
    const relative = path.relative(root, full)
    if (entry.isSymbolicLink()) entries.push({ relative, symlink: true })
    else if (entry.isDirectory()) walk(full)
    else entries.push({ relative, symlink: false })
  }
}
if (fs.existsSync(root)) walk(root)
const unsafeEntries = entries.filter(entry => entry.symlink || forbidden.test(entry.relative))
const missingFiles = required.filter(relative => !fs.existsSync(path.join(root, relative)))
const run = (args, cwd = root, extraEnv = {}) => spawnSync('node', args, { cwd, encoding: 'utf8', env: { ...process.env, ...extraEnv }, maxBuffer: 32 * 1024 * 1024 })
const architecture = missingFiles.length === 0 ? run(['scripts/verify-architecture.mjs']) : { status: 1, stdout: '', stderr: 'required file missing' }
const b12Architecture = missingFiles.length === 0 ? run(['scripts/verify-b12-architecture.mjs']) : { status: 1, stdout: '', stderr: 'required file missing' }
const moduleBoundary = missingFiles.length === 0 ? run(['scripts/verify-module-boundaries.mjs']) : { status: 1, stdout: '', stderr: 'required file missing' }
const generator = missingFiles.length === 0
  ? run(['scripts/generate-b12-acceptance.mjs'], root, packageGeneratorEnvironment(root))
  : { status: 1, stdout: '', stderr: 'required file missing' }
const node = missingFiles.length === 0
  ? run(['--test', 'tests/b12-acceptance-generator.test.mjs', 'tests/b12-capability-parity.test.mjs', 'tests/b12-negative-matrix.test.mjs', 'tests/b12-revision-gates.test.mjs'])
  : { status: 1, stdout: '', stderr: 'required file missing' }
const status = missingFiles.length === 0
  && unsafeEntries.length === 0
  && architecture.status === 0
  && b12Architecture.status === 0
  && generator.status === 0
  && moduleBoundary.status === 0
  && node.status === 0
const report = {
  schema: 'b12-package-smoke-r1',
  root,
  requiredFiles: required,
  missingFiles,
  unsafeEntries,
  freshReceiver: {
    architecture: architecture.status === 0 ? 'PASS' : 'FAIL',
    b12Architecture: b12Architecture.status === 0 ? 'PASS' : 'FAIL',
    generator: generator.status === 0 && /READY_FOR_ACCEPTANCE/.test(generator.stdout) ? 'PASS' : 'FAIL',
    moduleBoundary: moduleBoundary.status === 0 ? 'PASS' : 'FAIL',
    node: node.status === 0 ? 'PASS' : 'FAIL',
  },
  status: status ? 'PASS' : 'FAIL',
}
console.log(JSON.stringify(report, null, 2))
if (!status) process.exitCode = 1
