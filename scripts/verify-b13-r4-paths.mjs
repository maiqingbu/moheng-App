import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SKILL_RELATIVE_ROOT = 'creative-skills/v1.2.0'
const ARTICLE_RELATIVE_ROOT = SKILL_RELATIVE_ROOT + '/sources/articles'
const PATH_LAYER_A_PATTERN = /[\uFFFDÃÂΣσΦΘ╕┐åèòû]/

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex')
}

function git(args, encoding = 'utf8') {
  const result = spawnSync('git', args, { cwd: root, encoding })
  if (result.status !== 0) throw new Error('B13_R4_GIT_FAILED:' + args.join(' ') + ':' + String(result.stderr || ''))
  return result.stdout
}

function gitText(args) {
  return git(args, 'utf8').trim()
}

function frozenSkillFiles(baselineCommit) {
  return gitText(['-c', 'core.quotePath=false', 'ls-tree', '-r', '--name-only', baselineCommit, '--', SKILL_RELATIVE_ROOT])
    .split('\n')
    .filter(Boolean)
}

function frozenSourceIndex(baselineCommit) {
  const bytes = git(['show', baselineCommit + ':' + SKILL_RELATIVE_ROOT + '/sources/source-index.jsonl'], null)
  return bytes.toString('utf8')
}

function frozenArticleMapping(baselineCommit) {
  const paths = frozenSkillFiles(baselineCommit)
  const articlePaths = paths.filter(file => file.startsWith(ARTICLE_RELATIVE_ROOT + '/') && file.endsWith('.md')).sort((left, right) => left.localeCompare(right))
  const documents = []
  for (const line of frozenSourceIndex(baselineCommit).split(/\r?\n/)) {
    if (!line.trim()) continue
    const value = JSON.parse(line)
    if (!documents.includes(value.document)) documents.push(value.document)
  }
  if (articlePaths.length !== 20 || documents.length !== 20) throw new Error('B13_R4_PATH_COUNT_ESCALATION_REQUIRED')
  const oldToNew = new Map()
  const newToOld = new Map()
  for (const oldPath of articlePaths) {
    const number = Number(oldPath.match(/\/(\d+)-/)?.[1])
    const document = documents[number - 1]
    const restored = ARTICLE_RELATIVE_ROOT + '/' + String(number).padStart(2, '0') + '-' + document + '.md'
    oldToNew.set(oldPath, restored)
    newToOld.set(restored, oldPath)
  }
  return { paths, articlePaths, documents, oldToNew, newToOld }
}

function frozenContentHashes(baselineCommit, mapping) {
  const hashes = new Map()
  for (const file of mapping.paths) {
    const sourcePath = mapping.oldToNew.get(file) || file
    const bytes = git(['show', baselineCommit + ':' + file], null)
    hashes.set(sourcePath, sha256Bytes(bytes))
  }
  return hashes
}

function walkFiles(directory) {
  const files = []
  const visit = current => {
    for (const entry of fs.readdirSync(current, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      const full = path.join(current, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.isFile()) files.push(full)
    }
  }
  visit(directory)
  return files
}

function cp437Candidates(paths) {
  const script = [
    'import json, re, sys',
    'paths=json.loads(sys.stdin.read())',
    'pattern=re.compile(r"[\\u3400-\\u9fff\\u3000-\\u303f]")',
    'candidates=[]',
    'for path in paths:',
    '    for segment in path.split("/"):' ,
    '        try:',
    '            decoded=segment.encode("cp437").decode("utf-8")',
    '        except (UnicodeEncodeError, UnicodeDecodeError):',
    '            continue',
    '        if decoded != segment and pattern.search(decoded):',
    '            candidates.append({"path":path,"segment":segment,"decoded":decoded,"classification":"CP437_MOJIBAKE_CANDIDATE"})',
    'print(json.dumps(candidates, ensure_ascii=False))',
  ].join('\n')
  const result = spawnSync('python3', ['-c', script], {
    cwd: root,
    input: JSON.stringify(paths),
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
  if (result.status !== 0) throw new Error('B13_R4_CP437_DETECTOR_FAILED:' + String(result.stderr || ''))
  return JSON.parse(result.stdout)
}

function scanPathNames(paths) {
  const uniquePaths = [...new Set(paths)].sort((left, right) => left.localeCompare(right))
  const knownMojibakeFindings = uniquePaths
    .filter(file => PATH_LAYER_A_PATTERN.test(file))
    .map(file => ({ path: file, classification: 'KNOWN_MOJIBAKE_MARKER' }))
  const cp437Findings = cp437Candidates(uniquePaths)
  const nonNfcPaths = uniquePaths.filter(file => file.normalize('NFC') !== file)
  const replacementFindings = uniquePaths
    .filter(file => file.includes('\ufffd'))
    .map(file => ({ path: file, classification: 'REPLACEMENT_CHARACTER' }))
  return {
    replacementFindings,
    knownMojibakeFindings,
    cp437MojibakeFindings: cp437Findings,
    nonNfcPaths,
  }
}

function expectedPathMap(baselineCommit) {
  const mapping = frozenArticleMapping(baselineCommit)
  const expectedPaths = mapping.paths.map(file => mapping.oldToNew.get(file) || file).sort((left, right) => left.localeCompare(right))
  const expectedHashes = frozenContentHashes(baselineCommit, mapping)
  return { mapping, expectedPaths, expectedHashes }
}

function compareToFrozen(actualFiles, actualHashes, baselineCommit) {
  const { mapping, expectedPaths, expectedHashes } = expectedPathMap(baselineCommit)
  const actualPaths = [...actualFiles].sort((left, right) => left.localeCompare(right))
  const missingPaths = expectedPaths.filter(file => !actualFiles.has(file))
  const extraPaths = actualPaths.filter(file => !expectedHashes.has(file))
  const contentMismatches = expectedPaths
    .filter(file => actualHashes.get(file) !== expectedHashes.get(file))
    .map(file => ({ path: file, actual: actualHashes.get(file) || null, expected: expectedHashes.get(file) }))
  const pathFindings = scanPathNames(actualPaths)
  const actualContentHashes = [...actualHashes.values()].sort()
  const expectedContentHashes = [...expectedHashes.values()].sort()
  const contentDigest = sha256Bytes(Buffer.from(JSON.stringify(actualContentHashes), 'utf8'))
  const expectedContentDigest = sha256Bytes(Buffer.from(JSON.stringify(expectedContentHashes), 'utf8'))
  const pathDigest = sha256Bytes(Buffer.from(JSON.stringify(actualPaths), 'utf8'))
  const expectedPathDigest = sha256Bytes(Buffer.from(JSON.stringify(expectedPaths), 'utf8'))
  return {
    schema: 'b13-r4-path-scan',
    baselineCommit,
    totalFiles: actualPaths.length,
    expectedTotalFiles: expectedPaths.length,
    pathDigest,
    expectedPathDigest,
    contentDigest,
    expectedContentDigest,
    missingPaths,
    extraPaths,
    contentMismatches,
    frozenPathMismatches: missingPaths.length + extraPaths.length,
    contentChanges: contentMismatches.length,
    ...pathFindings,
    pathMatchesFrozen: missingPaths.length === 0 && extraPaths.length === 0,
    contentMatchesFrozen: contentMismatches.length === 0 && contentDigest === expectedContentDigest,
    status: missingPaths.length === 0
      && extraPaths.length === 0
      && contentMismatches.length === 0
      && contentDigest === expectedContentDigest
      && pathFindings.replacementFindings.length === 0
      && pathFindings.knownMojibakeFindings.length === 0
      && pathFindings.cp437MojibakeFindings.length === 0
      && pathFindings.nonNfcPaths.length === 0
      ? 'PASS'
      : 'FAIL',
  }
}

export function scanSkillDirectory(directory, baselineCommit) {
  const base = path.resolve(directory)
  const files = walkFiles(base)
  const actualFiles = new Set(files.map(file => SKILL_RELATIVE_ROOT + '/' + path.relative(base, file).split(path.sep).join('/')))
  const actualHashes = new Map(files.map(file => [
    SKILL_RELATIVE_ROOT + '/' + path.relative(base, file).split(path.sep).join('/'),
    sha256Bytes(fs.readFileSync(file)),
  ]))
  return compareToFrozen(actualFiles, actualHashes, baselineCommit)
}

export function scanPackageZip(zipPath, baselineCommit) {
  const script = [
    'import hashlib, json, sys, zipfile',
    'zip_path=sys.argv[1]',
    'with zipfile.ZipFile(zip_path) as archive:',
    '    entries=[]',
    '    for info in archive.infolist():',
    '        if info.is_dir():',
    '            continue',
    '        entries.append({"name":info.filename,"sha256":hashlib.sha256(archive.read(info.filename)).hexdigest()})',
    '    print(json.dumps(entries, ensure_ascii=False))',
  ].join('\n')
  const result = spawnSync('python3', ['-c', script, zipPath], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  })
  if (result.status !== 0) throw new Error('B13_R4_ZIP_PATH_SCAN_FAILED:' + String(result.stderr || ''))
  const entries = JSON.parse(result.stdout)
  const roots = [...new Set(entries.map(entry => entry.name.split('/', 1)[0]))]
  if (roots.length !== 1) return { schema: 'b13-r4-path-scan', baselineCommit, status: 'FAIL', error: 'archive root is ambiguous' }
  const prefix = roots[0] + '/SOURCE/'
  const skillEntries = entries.filter(entry => entry.name.startsWith(prefix + SKILL_RELATIVE_ROOT + '/'))
  const actualFiles = new Set(skillEntries.map(entry => entry.name.slice(prefix.length)))
  const actualHashes = new Map(skillEntries.map(entry => [entry.name.slice(prefix.length), entry.sha256]))
  return { ...compareToFrozen(actualFiles, actualHashes, baselineCommit), archiveRoot: roots[0], archiveEntryCount: entries.length }
}

export function pathRestorationReport(baselineCommit = 'a479b28') {
  const { mapping } = expectedPathMap(baselineCommit)
  const items = [...mapping.newToOld.entries()].sort(([left], [right]) => left.localeCompare(right)).map(([restoredPath, mojibakePath]) => {
    const restoredFile = path.join(root, restoredPath)
    const frozenContentSha256 = frozenContentHashes(baselineCommit, mapping).get(restoredPath)
    const afterContentSha256 = fs.existsSync(restoredFile) ? sha256Bytes(fs.readFileSync(restoredFile)) : null
    const beforeBytes = git(['show', baselineCommit + ':' + mojibakePath], null)
    const beforeContentSha256 = sha256Bytes(beforeBytes)
    return {
      mojibakePath,
      restoredPath,
      beforeContentSha256,
      afterContentSha256,
      frozenContentSha256,
      contentUnchanged: beforeContentSha256 === afterContentSha256 && afterContentSha256 === frozenContentSha256,
      pathMatchesFrozen: fs.existsSync(restoredFile),
      nfcNormalized: restoredPath.normalize('NFC') === restoredPath,
    }
  })
  const summary = gitText(['diff', '--summary', baselineCommit + '..HEAD'])
  const actualRenameCount = summary.split('\n').filter(line => /^ rename /.test(line)).length
  return {
    schema: 'b13-r4-path-restoration',
    baselineCommit,
    expectedRenameCount: 20,
    actualRenameCount,
    items,
    contentChanges: items.filter(item => !item.contentUnchanged).length,
    pathMatchesFrozen: items.filter(item => !item.pathMatchesFrozen).length,
    nfcNonNormalized: items.filter(item => !item.nfcNormalized).length,
    status: items.length === 20 && actualRenameCount === 20 && items.every(item => item.contentUnchanged && item.nfcNormalized && item.pathMatchesFrozen) ? 'PASS' : 'FAIL',
    currentBase: path.join(root, SKILL_RELATIVE_ROOT),
    mappingSource: 'Creative Skills v1.2.0 source-index.jsonl document order + baseline content SHA',
  }
}

export function frozenSkillPathInventory(baselineCommit = 'a479b28') {
  const scan = scanSkillDirectory(path.join(root, SKILL_RELATIVE_ROOT), baselineCommit)
  return {
    schema: 'b13-r4-frozen-skill-path-inventory',
    baselineCommit,
    totalFiles: scan.totalFiles,
    pathDigest: scan.pathDigest,
    expectedPathDigest: scan.expectedPathDigest,
    contentDigest: scan.contentDigest,
    expectedContentDigest: scan.expectedContentDigest,
    mojibakeCount: scan.knownMojibakeFindings.length + scan.cp437MojibakeFindings.length,
    nonNfcCount: scan.nonNfcPaths.length,
    status: scan.status,
  }
}

if (process.argv[1]?.endsWith('verify-b13-r4-paths.mjs')) {
  const baselineIndex = process.argv.indexOf('--baseline')
  const baselineCommit = baselineIndex >= 0 ? process.argv[baselineIndex + 1] : 'a479b28'
  const sourceIndex = process.argv.indexOf('--source')
  const zipIndex = process.argv.indexOf('--zip')
  let result
  if (sourceIndex >= 0) result = scanSkillDirectory(process.argv[sourceIndex + 1], baselineCommit)
  else if (zipIndex >= 0) result = scanPackageZip(process.argv[zipIndex + 1], baselineCommit)
  else {
    console.error('B13_R4_PATH_TARGET_REQUIRED')
    process.exitCode = 1
    process.exit()
  }
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}
