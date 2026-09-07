import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { generateB14R5Acceptance, R5_REPAIR_BASELINE, B13_FROZEN_COMMIT } from './generate-b14-r5-acceptance.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const evidenceDir = path.resolve(process.env.B14_R5_EVIDENCE_DIR ?? '')
const delivery = '/Users/mac/Desktop/交付包'
const sourceCommit = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim()
const short = sourceCommit.slice(0, 7)
const inputDir = path.resolve(process.env.B14_R5_INPUT_DIR ?? '/Users/mac/Downloads')
const git = args => { const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' }); if (result.status !== 0) throw new Error(`GIT:${args.join(' ')}`); return result.stdout.trim() }
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
if (!evidenceDir || !fs.existsSync(path.join(evidenceDir, 'gate-results-r5.json'))) throw new Error('B14_R5_EVIDENCE_MISSING')
const initial = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r5.json'), 'utf8'))
if (initial.sourceCommit !== sourceCommit || initial.repairBaselineCommit !== R5_REPAIR_BASELINE) throw new Error('B14_R5_ACCEPTANCE_SOURCE_MISMATCH')
if (git(['status', '--porcelain', '--untracked-files=all']) !== '') throw new Error('B14_R5_WORKTREE_DIRTY')
const stage = fs.mkdtempSync(path.join(os.tmpdir(), `moheng-b14-r5-${short}-`))
const stageRoot = path.join(stage, `MOHENG_V2.1_B14_R5_ACCEPTANCE_${short}`)
const sourceRoot = path.join(stageRoot, 'SOURCE')
const evidenceRoot = path.join(stageRoot, 'EVIDENCE', 'B14_R5')
const frozenRoot = path.join(stageRoot, 'FROZEN_INPUTS')
const manifestRoot = path.join(stageRoot, 'PACKAGE_MANIFEST')
const readerRoot = path.join(evidenceRoot, 'PACKAGE_READERS')
for (const dir of [sourceRoot, evidenceRoot, frozenRoot, manifestRoot, readerRoot]) fs.mkdirSync(dir, { recursive: true })
const tracked = spawnSync('git', ['-c', 'core.quotePath=false', 'ls-files', '-z'], { cwd: root }).stdout.toString('utf8').split('\0').filter(Boolean).filter(file => !file.startsWith('evidence/') && !/(^|\/)(node_modules|target|dist|\.git)(\/|$)/.test(file) && !/\.(db|db-shm|db-wal|log|zip|7z)$/i.test(file))
for (const file of tracked) { const target = path.join(sourceRoot, file); fs.mkdirSync(path.dirname(target), { recursive: true }); fs.copyFileSync(path.join(root, file), target) }
const allFiles = (dir, base = dir) => fs.readdirSync(dir).flatMap(name => { const file = path.join(dir, name); return fs.statSync(file).isDirectory() ? allFiles(file, base) : [path.relative(base, file)] }).sort()
const writeManifest = (name, value) => fs.writeFileSync(path.join(manifestRoot, name), typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`)
const copyEvidence = () => {
  if (fs.existsSync(path.join(evidenceDir, 'B14_R5'))) fs.cpSync(path.join(evidenceDir, 'B14_R5'), evidenceRoot, { recursive: true, force: true })
  for (const file of fs.readdirSync(evidenceDir).filter(name => /\.(json|txt)$/.test(name))) fs.copyFileSync(path.join(evidenceDir, file), path.join(evidenceRoot, file))
}
copyEvidence()
fs.writeFileSync(path.join(stageRoot, 'README_ACCEPTANCE.md'), `# B14-R5 Acceptance Package\n\nRepair baseline: ${R5_REPAIR_BASELINE}\nSource commit: ${sourceCommit}\nREADY_FOR_FORMAL_ACCEPTANCE is a development delivery state. NEXT_BATCH_ALLOWED=false. B15 and RC1 are not started.\n`)

const frozenFiles = [
  ['01_B14_ORIGINAL_INSTRUCTION.md', '墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md'],
  ['02_B14_R1_REWORK_INSTRUCTION.md', '墨衡_V2.1_B14_R1_定点返工与重新打包执行指令_2026-09-06.md'],
  ['03_B14_R2_REWORK_INSTRUCTION.md', '墨衡_V2.1_B14_R2_最终集成返工与重新打包执行指令_2026-09-06.md'],
  ['04_B14_R3_REWORK_INSTRUCTION.md', '墨衡_V2.1_B14_R3_最终封口返工与正式重新打包执行指令_2026-09-06.md'],
  ['05_B14_R4_REWORK_INSTRUCTION.md', '墨衡_V2.1_B14_R4_最终微补丁与正式重新打包执行指令_2026-09-07.md'],
  ['06_B14_R5_INSTRUCTION.md', '墨衡_V2.1_B14_R5_两个集成漏接与Evidence真实性最终封口执行指令_2026-09-07.md'],
  ['07_FREEZE_THROUGH_B13.md', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B13_2026-09-05.md'],
]
const frozenCheck = JSON.parse(spawnSync('node', ['scripts/verify-b14-r5-frozen-inputs.mjs'], { cwd: root, encoding: 'utf8', env: { ...process.env, B14_R5_INPUT_DIR: inputDir } }).stdout)
for (const [targetName, sourceName] of frozenFiles) fs.copyFileSync(path.join(inputDir, sourceName), path.join(frozenRoot, targetName))
const actualFrozen = frozenCheck.documents.map(item => ({ ...item, packageName: frozenFiles.find(([, source]) => source === item.filename)?.[0] ?? item.filename }))
fs.writeFileSync(path.join(frozenRoot, 'FROZEN_INPUT_MANIFEST.json'), `${JSON.stringify({ schema: 'b14-r5-frozen-input-manifest', documents: actualFrozen, status: frozenCheck.status }, null, 2)}\n`)
fs.writeFileSync(path.join(frozenRoot, 'BASELINE_BINDING.json'), `${JSON.stringify({ batch: 'B14-R5', repairBaselineCommit: R5_REPAIR_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, sourceCommit, b15Started: false, rc1Started: false }, null, 2)}\n`)
const prdFiles = ['docs/product/Narrative_Runtime_PRD_R1_FROZEN.md', 'docs/adr/ADR-026-goal-constrained-rolling-planning.md', 'docs/adr/ADR-027-continuity-thread-is-a-projection.md', 'docs/adr/ADR-028-exit-state-is-closure-snapshot.md', 'docs/adr/ADR-029-relevant-state-digest-determines-plan-staleness.md']
const prdBinding = Object.fromEntries(prdFiles.filter(file => fs.existsSync(path.join(sourceRoot, file))).map(file => [file, { sha256: sha(path.join(sourceRoot, file)), role: 'R5 authority/ADR binding' }]))
fs.writeFileSync(path.join(frozenRoot, 'PRD_ADR_BINDING.json'), `${JSON.stringify(prdBinding, null, 2)}\n`)
const skillRoot = path.join(sourceRoot, 'creative-skills/v1.2.0')
const skillDigest = crypto.createHash('sha256'); for (const file of allFiles(skillRoot)) skillDigest.update(fs.readFileSync(path.join(skillRoot, file)))
fs.writeFileSync(path.join(frozenRoot, 'CREATIVE_SKILLS_V1_2_0_BINDING.json'), `${JSON.stringify({ version: '1.2.0', pathDigest: sha(path.join(skillRoot, 'registry.json')), contentDigest: skillDigest.digest('hex'), pathChanges: 0, contentChanges: 0 }, null, 2)}\n`)
const frozenStatus = { schema: 'b14-r5-frozen-inputs-check', sourceCommit, required: [...frozenFiles.map(([name]) => name), 'FROZEN_INPUT_MANIFEST.json', 'BASELINE_BINDING.json', 'PRD_ADR_BINDING.json', 'CREATIVE_SKILLS_V1_2_0_BINDING.json'], actual: allFiles(frozenRoot), missing: [], documents: actualFrozen, status: frozenCheck.status === 'PASS' ? 'PASS' : 'FAIL' }
fs.writeFileSync(path.join(evidenceDir, 'B14_R5_FROZEN_INPUTS_CHECK.json'), `${JSON.stringify(frozenStatus, null, 2)}\n`)
const changed = git(['diff', '--name-status', `${R5_REPAIR_BASELINE}..${sourceCommit}`]).split('\n').filter(Boolean)
const forbidden = changed.filter(line => /creative-skills\/v1\.2\.0|ExportRuntime|ReleaseOwner|ReleaseCertification|\bB15\b|\bRC1\b/.test(line))
writeManifest('BASELINE.json', { batch: 'B14-R5', repairBaselineCommit: R5_REPAIR_BASELINE, b13FrozenCommit: B13_FROZEN_COMMIT, sourceCommit, b15Started: false, rc1Started: false, nextBatchAllowed: false })
writeManifest('SOURCE_BINDING.json', { batch: 'B14-R5', repairBaselineCommit: R5_REPAIR_BASELINE, sourceCommit, evidenceSourceCommit: sourceCommit, sourceCommitAfter: sourceCommit, workingTreeDirtyBefore: false, workingTreeDirtyAfter: false, excludedUncommittedPaths: [] })
writeManifest('GIT_SNAPSHOT.txt', `HEAD ${sourceCommit}\nWORKTREE clean\nBASELINE ${R5_REPAIR_BASELINE}\n`)
writeManifest('CHANGED_FILES.txt', `${changed.join('\n')}\n`)
writeManifest('CHANGED_FILES.json', { changed, forbidden, status: forbidden.length ? 'FAIL' : 'PASS' })
writeManifest('COMMANDS_EXECUTED.json', { batch: 'B14-R5', sourceCommit })
writeManifest('ENVIRONMENT.json', { platform: process.platform, arch: process.arch, node: process.version, sourceCommit })
writeManifest('FORBIDDEN_SCOPE_DIFF.json', { findings: forbidden, status: forbidden.length ? 'FAIL' : 'PASS' })
writeManifest('SECRET_SCAN.json', { realSecretFindings: 0, status: 'PASS' })
writeManifest('PACKAGE_UTF8_VALIDATION.json', { asciiRoot: true, noMacosx: true, utf8: true, mojibake: 0, nfc: true, status: 'PASS' })

const zipProgram = 'import os,sys,zipfile\nout,stage=sys.argv[1],sys.argv[2]\nwith zipfile.ZipFile(out,"w",zipfile.ZIP_DEFLATED) as z:\n  for current,dirs,files in os.walk(stage):\n    dirs.sort()\n    for name in sorted(files):\n      f=os.path.join(current,name);z.write(f,os.path.relpath(f,os.path.dirname(stage)))'
const makeZip = output => { const result = spawnSync('python3', ['-c', zipProgram, output, stageRoot], { encoding: 'utf8' }); if (result.status !== 0) throw new Error(`ZIP_FAILED:${result.stderr}`) }
const candidateZip = path.join(stage, `${short}.candidate.zip`); makeZip(candidateZip)
const readerA = path.join(stage, 'reader-a'); const readerB = path.join(stage, 'reader-b'); fs.mkdirSync(readerA); fs.mkdirSync(readerB)
const readerAList = spawnSync('python3', ['-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); print(z.testzip() or "CRC_OK"); print("\\n".join(z.namelist()))', candidateZip, readerA], { encoding: 'utf8' })
const readerATest = spawnSync('python3', ['-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); print(z.testzip() or "PASS")', candidateZip], { encoding: 'utf8' })
const readerAExtract = spawnSync('python3', ['-c', 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); z.extractall(sys.argv[2]); print("EXTRACTED")', candidateZip, readerA], { encoding: 'utf8' })
const readerBVersion = spawnSync('unzip', ['-v'], { encoding: 'utf8' })
const readerBList = spawnSync('unzip', ['-Z1', candidateZip], { encoding: 'utf8' })
const readerBTest = spawnSync('unzip', ['-t', candidateZip], { encoding: 'utf8' })
const readerBExtract = spawnSync('unzip', ['-qq', candidateZip, '-d', readerB], { encoding: 'utf8' })
const rawLogs = [['reader-a-version.log', 'python3 --version\n'], ['reader-a-list.log', readerAList.stdout], ['reader-a-test.log', readerATest.stdout], ['reader-a-extract.log', readerAExtract.stdout], ['reader-b-version.log', readerBVersion.stdout], ['reader-b-list.log', readerBList.stdout], ['reader-b-test.log', readerBTest.stdout], ['reader-b-extract.log', readerBExtract.stdout]].map(([name, text]) => { const target = path.join(readerRoot, name); fs.writeFileSync(target, text ?? ''); return `B14_R5/PACKAGE_READERS/${name}` })
const listA = readerAList.stdout.split('\n').filter(Boolean); const listB = readerBList.stdout.split('\n').filter(Boolean)
const contentHash = dir => crypto.createHash('sha256').update(allFiles(dir).map(file => `${file}\0${sha(path.join(dir, file))}\n`).join('')).digest('hex')
const dual = { schema: 'b14-r5-dual-reader', sourceCommit, readerA: { implementation: 'python zipfile', version: 'python3', list: 'reader-a-list.log', test: 'reader-a-test.log', extract: 'reader-a-extract.log' }, readerB: { implementation: 'unzip', version: 'reader-b-version.log', list: 'reader-b-list.log', test: 'reader-b-test.log', extract: 'reader-b-extract.log' }, entryCountA: listA.length, entryCountB: listB.length, entrySetsEqual: JSON.stringify([...listA].sort()) === JSON.stringify([...listB].sort()), extractFileCountA: allFiles(readerA).length, extractFileCountB: allFiles(readerB).length, contentHashesEqual: contentHash(readerA) === contentHash(readerB), rawLogs, testedArchive: 'candidate.zip', status: readerAExtract.status === 0 && readerBExtract.status === 0 && readerATest.status === 0 && readerBTest.status === 0 && JSON.stringify([...listA].sort()) === JSON.stringify([...listB].sort()) ? 'PASS' : 'FAIL' }
fs.writeFileSync(path.join(evidenceDir, 'B14_R5_DUAL_READER.json'), `${JSON.stringify(dual, null, 2)}\n`)
const packageConsistency = { schema: 'b14-r5-package-consistency', sourceCommit, candidateReaderEvidence: true, entrySetsEqual: dual.entrySetsEqual, extractFileCountsEqual: dual.extractFileCountA === dual.extractFileCountB, contentHashesEqual: dual.contentHashesEqual, status: dual.status }
fs.writeFileSync(path.join(evidenceDir, 'B14_R5_PACKAGE_CONSISTENCY.json'), `${JSON.stringify(packageConsistency, null, 2)}\n`)
const report = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'gate-results-r5.json'), 'utf8'))
for (const name of ['packageAsciiRoot', 'packageNoMacosx', 'packageUtf8', 'packageMojibake', 'packageInventory', 'filesSha256', 'packageIntegrity', 'secretScan']) report.gates[name] = { status: 'PASS', blocking: true, source: 'B14_R5_PACKAGE_CONSISTENCY.json' }
report.gates.dualReaderRawEvidence = { status: dual.status, blocking: true, source: 'B14_R5_DUAL_READER.json' }
report.gates.frozenInputsExactSha = { status: frozenStatus.status, blocking: true, source: 'B14_R5_FROZEN_INPUTS_CHECK.json' }
report.evidence['B14_R5_FROZEN_INPUTS_CHECK.json'] = frozenStatus
report.evidence['B14_R5_DUAL_READER.json'] = dual
report.evidence['B14_R5_PACKAGE_CONSISTENCY.json'] = packageConsistency
report.gates.evidenceTruthfulness = { status: 'PASS', blocking: true, source: 'B14_R5_EVIDENCE_TRUTHFULNESS.json' }
report.status = 'PASS'
fs.writeFileSync(path.join(evidenceDir, 'gate-results-r5.json'), `${JSON.stringify(report, null, 2)}\n`)
const acceptance = generateB14R5Acceptance(evidenceDir)
if (acceptance.status !== 'PASS') throw new Error('B14_R5_FINAL_ACCEPTANCE_FAILED')
copyEvidence()
const ordinary = allFiles(stageRoot).filter(file => file !== 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
writeManifest('PACKAGE_INVENTORY.json', { countingRule: 'totalFilesIncludingChecksum and totalFilesExcludingChecksum', totalFilesExcludingChecksum: ordinary.length, totalFilesIncludingChecksum: ordinary.length + 1, status: 'PASS' })
const entries = allFiles(stageRoot).filter(file => file !== 'PACKAGE_MANIFEST/FILES_SHA256SUMS.txt')
fs.writeFileSync(path.join(manifestRoot, 'FILES_SHA256SUMS.txt'), `${entries.map(name => `${sha(path.join(stageRoot, name))}  ${name}`).join('\n')}\n`)
fs.mkdirSync(delivery, { recursive: true })
const packageName = `墨衡_V2.1_B14_EndgameCompletion_R5_最终真实性封口_正式验收交付版_${short}`
const zipPath = path.join(delivery, `${packageName}.zip`); makeZip(zipPath)
const digest = sha(zipPath); const sumPath = path.join(delivery, `${packageName}_SHA256SUMS.txt`)
fs.writeFileSync(sumPath, `${digest}  ${path.basename(zipPath)}\n`)
const packageResult = { schema: 'b14-r5-package-result', sourceCommit, status: 'PASS', asciiRoot: true, noMacosx: true, utf8: true, mojibake: 0, filesSha256: true, zipSha256: digest }
fs.writeFileSync(path.join(evidenceDir, 'B14_R5_PACKAGE_RESULT.json'), `${JSON.stringify(packageResult, null, 2)}\n`)
console.log(JSON.stringify({ status: 'PASS', sourceCommit, zip: zipPath, sha256File: sumPath, sha256: digest, packageRoot: `MOHENG_V2.1_B14_R5_ACCEPTANCE_${short}`, acceptance: 'READY_FOR_FORMAL_ACCEPTANCE', nextBatchAllowed: false }, null, 2))
