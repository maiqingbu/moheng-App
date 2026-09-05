import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const MOJIBAKE_PATTERN = /[\uFFFDÃÂ]|â(?:€|\u0080)|ï¿½/

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
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

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    ...options,
  })
}

function outputOf(result) {
  return (result.stdout || '') + (result.stderr || '')
}

function writeLog(directory, name, value) {
  fs.writeFileSync(path.join(directory, name), value.endsWith('\n') ? value : value + '\n')
}

function resolveReaderB() {
  for (const candidate of ['7z', 'bsdtar', 'jar', 'unzip']) {
    const result = run('sh', ['-lc', 'command -v ' + candidate])
    if (result.status === 0 && result.stdout.trim()) return { name: candidate, path: result.stdout.trim() }
  }
  return null
}

function readerA(zipPath, extractDir, outputDir) {
  const version = run('python3', ['--version'])
  const python = String.raw`import json, sys, zipfile
zip_path=sys.argv[1]
out=sys.argv[2]
with zipfile.ZipFile(zip_path) as archive:
    infos=[info for info in archive.infolist() if not info.is_dir()]
    names=[info.filename for info in infos]
    roots={name.split('/',1)[0] for name in names}
    utf8_failures=sum(1 for info in infos if any(ord(ch)>127 for ch in info.filename) and not (info.flag_bits & 0x800))
    bad_crc=archive.testzip()
    archive.extractall(out)
    print(json.dumps({'names':names,'roots':sorted(roots),'utf8FlagFailures':utf8_failures,'badCrc':bad_crc}, ensure_ascii=False))
`
  const result = run('python3', ['-c', python, zipPath, extractDir])
  writeLog(outputDir, 'reader-a-version.log', 'command=python3 --version\n' + outputOf(version))
  writeLog(outputDir, 'reader-a-extract.log', 'command=python3 zipfile extract/test\n' + outputOf(result))
  if (result.status !== 0) return { ok: false, version: outputOf(version).trim(), names: [], utf8FlagFailures: null, error: outputOf(result) }
  let parsed
  try { parsed = JSON.parse(result.stdout.trim()) } catch (error) {
    return { ok: false, version: outputOf(version).trim(), names: [], utf8FlagFailures: null, error: String(error) }
  }
  writeLog(outputDir, 'reader-a-list.log', 'command=python3 zipfile entry enumeration\n' + parsed.names.join('\n'))
  return {
    ok: parsed.badCrc === null,
    version: outputOf(version).trim(),
    names: parsed.names,
    roots: parsed.roots,
    utf8FlagFailures: parsed.utf8FlagFailures,
    error: parsed.badCrc === null ? null : 'bad CRC: ' + parsed.badCrc,
  }
}

function readerB(zipPath, extractDir, outputDir, reader) {
  const versionArgs = reader.name === 'unzip' ? ['-v'] : reader.name === '7z' ? ['i'] : reader.name === 'bsdtar' ? ['--version'] : ['--version']
  const version = run(reader.path, versionArgs)
  writeLog(outputDir, 'reader-b-version.log', 'command=' + reader.name + ' ' + versionArgs.join(' ') + '\n' + outputOf(version))
  let list
  let extract
  let test
  if (reader.name === 'unzip') {
    list = run(reader.path, ['-Z1', zipPath])
    test = run(reader.path, ['-t', zipPath])
    extract = run(reader.path, ['-q', zipPath, '-d', extractDir])
  } else if (reader.name === '7z') {
    list = run(reader.path, ['l', '-slt', zipPath])
    test = run(reader.path, ['t', zipPath])
    extract = run(reader.path, ['x', '-y', '-o' + extractDir, zipPath])
  } else if (reader.name === 'bsdtar') {
    list = run(reader.path, ['-tf', zipPath])
    test = { status: 0, stdout: 'bsdtar list/read serves as archive test\n', stderr: '' }
    extract = run(reader.path, ['-xf', zipPath, '-C', extractDir])
  } else {
    list = run(reader.path, ['-tf', zipPath])
    test = { status: 0, stdout: 'jar list/read serves as archive test\n', stderr: '' }
    extract = run(reader.path, ['-xf', zipPath], { cwd: extractDir })
  }
  const listOutput = outputOf(list)
  writeLog(outputDir, 'reader-b-list.log', 'command=' + reader.name + ' list\n' + listOutput)
  writeLog(outputDir, 'reader-b-extract.log', 'command=' + reader.name + ' test\n' + outputOf(test) + 'command=' + reader.name + ' extract\n' + outputOf(extract))
  const names = list.status === 0
    ? (reader.name === '7z'
      ? listOutput.split(/\r?\n/).filter(line => line.startsWith('Path = ')).map(line => line.slice(7)).filter(line => line !== zipPath && !line.endsWith('/'))
      : listOutput.split(/\r?\n/).map(line => line.trim()).filter(Boolean).filter(line => !line.endsWith('/')))
    : []
  return {
    ok: list.status === 0 && test.status === 0 && extract.status === 0,
    version: outputOf(version).trim(),
    names,
    error: list.status === 0 && test.status === 0 && extract.status === 0 ? null : outputOf(test) + outputOf(extract),
  }
}

function extractedHashes(directory) {
  const files = walkFiles(directory)
  const hashes = Object.fromEntries(files.map(file => [path.relative(directory, file).split(path.sep).join('/').normalize('NFC'), sha256(file)]))
  return { files, hashes }
}

export function verifyDualReaders(zipPath, outputDir = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-readers-'))) {
  fs.mkdirSync(outputDir, { recursive: true })
  const extractRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'moheng-b13-r3-reader-extract-'))
  const extractA = path.join(extractRoot, 'a')
  const extractB = path.join(extractRoot, 'b')
  fs.mkdirSync(extractA)
  fs.mkdirSync(extractB)
  const a = readerA(zipPath, extractA, outputDir)
  const reader = resolveReaderB()
  if (!reader) {
    writeLog(outputDir, 'reader-b-version.log', 'reader B unavailable: unzip, 7z, bsdtar and jar were not found')
    writeLog(outputDir, 'reader-b-list.log', 'reader B unavailable')
    writeLog(outputDir, 'reader-b-extract.log', 'reader B unavailable')
  }
  const b = reader ? readerB(zipPath, extractB, outputDir, reader) : { ok: false, version: '', names: [], error: 'no independent reader available' }
  const namesA = [...a.names].map(name => name.normalize('NFC')).sort((left, right) => left.localeCompare(right))
  const namesB = [...b.names].map(name => name.normalize('NFC')).sort((left, right) => left.localeCompare(right))
  const entrySetsEqual = JSON.stringify(namesA) === JSON.stringify(namesB)
  const hashesA = a.ok ? extractedHashes(extractA) : { files: [], hashes: {} }
  const hashesB = b.ok ? extractedHashes(extractB) : { files: [], hashes: {} }
  const fileSetA = Object.keys(hashesA.hashes).sort((left, right) => left.localeCompare(right))
  const fileSetB = Object.keys(hashesB.hashes).sort((left, right) => left.localeCompare(right))
  const fileSetEqual = JSON.stringify(fileSetA) === JSON.stringify(fileSetB)
  const contentHashesEqual = fileSetEqual && fileSetA.every(file => hashesA.hashes[file] === hashesB.hashes[file])
  const nonAsciiEntries = namesA.filter(name => [...name].some(character => character.charCodeAt(0) > 127))
  const mojibakeFindings = nonAsciiEntries.filter(name => MOJIBAKE_PATTERN.test(name)).length
  const compare = {
    schema: 'b13-r3-dual-reader-content-compare',
    readerA: 'python-zipfile',
    readerB: reader?.name ?? null,
    entrySetA: namesA,
    entrySetB: namesB,
    entrySetsEqual,
    fileSetA,
    fileSetB,
    fileSetEqual,
    hashesA: hashesA.hashes,
    hashesB: hashesB.hashes,
    contentHashesEqual,
    nonAsciiEntries,
    mojibakeFindings,
    status: a.ok && b.ok && entrySetsEqual && contentHashesEqual && mojibakeFindings === 0 ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(outputDir, 'reader-content-compare.json'), JSON.stringify(compare, null, 2) + '\n')
  const summary = {
    schema: 'b13-r3-package-utf8-validation',
    readerA: 'python-zipfile',
    readerB: reader?.name ?? null,
    readerAVersion: a.version,
    readerBVersion: b.version,
    entryCountA: namesA.length,
    entryCountB: namesB.length,
    entrySetsEqual,
    extractFileCountA: fileSetA.length,
    extractFileCountB: fileSetB.length,
    contentHashesEqual,
    utf8FlagFailures: a.utf8FlagFailures,
    mojibakeFindings,
    replacementCharacterEntries: nonAsciiEntries.filter(name => name.includes('\ufffd')),
    readerAOk: a.ok,
    readerBOk: b.ok,
    readerBTool: reader?.path ?? null,
    status: a.ok && b.ok && entrySetsEqual && contentHashesEqual && a.utf8FlagFailures === 0 && mojibakeFindings === 0 ? 'PASS' : 'FAIL',
  }
  fs.writeFileSync(path.join(outputDir, 'B13_R3_DUAL_READER_UTF8.json'), JSON.stringify(summary, null, 2) + '\n')
  return { ...summary, outputDir }
}

if (process.argv[1]?.endsWith('verify-b13-r3-dual-reader.mjs')) {
  const zipPath = process.argv[2]
  const outputDir = process.argv[3]
  if (!zipPath) {
    console.error('B13_R3_DUAL_READER_ZIP_REQUIRED')
    process.exitCode = 1
  } else {
    const result = verifyDualReaders(zipPath, outputDir)
    console.log(JSON.stringify(result, null, 2))
    if (result.status !== 'PASS') process.exitCode = 1
  }
}
