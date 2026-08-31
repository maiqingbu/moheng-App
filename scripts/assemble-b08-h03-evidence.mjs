import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidence = process.env.B08_H03_EVIDENCE_DIR ?? path.join(root,'evidence/B08_H03')
const reportPath = path.join(evidence,'gate-results.json')
if (!fs.existsSync(reportPath)) throw new Error('B08_H03 gate-results.json missing')
const report = JSON.parse(fs.readFileSync(reportPath,'utf8'))
if (report.sourceBinding !== 'PASS' || report.workingTreeDirtyBefore || report.workingTreeDirtyAfter) {
  throw new Error('B08_H03 source binding is not clean')
}
const files = fs.readdirSync(evidence).filter(file => !['evidence-manifest.json'].includes(file)).sort()
const manifest = {
  schema:'b08-h03-evidence-manifest-r2',sourceCommit:report.sourceCommit,
  sourceCommitAfter:report.sourceCommitAfter,workingTreeDirtyBefore:false,workingTreeDirtyAfter:false,
  sourceBinding:'PASS',gateCount:Object.keys(report.gates).length,
  files:files.map(file => ({file,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(evidence,file))).digest('hex')})),
}
fs.writeFileSync(path.join(evidence,'evidence-manifest.json'),`${JSON.stringify(manifest,null,2)}\n`)
console.log(`B08_H03 evidence assembled: ${manifest.files.length} files; source=${manifest.sourceCommit}`)
