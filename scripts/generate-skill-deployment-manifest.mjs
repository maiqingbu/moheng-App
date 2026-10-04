import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const DEPLOYMENT_MANIFEST_NAME = 'deployment-manifest.json'
export const DEPLOYMENT_MANIFEST_SCHEMA = 'moheng.creative-skills.deployment.v1'
export const HASH_ALGORITHM = 'SHA-256 over raw file bytes'
export const MANIFEST_DIGEST_ALGORITHM = 'SHA-256 over raw UTF-8 manifest bytes'
export const PATH_ENCODING = 'UTF-8 POSIX relative paths'

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const repositoryRoot = path.resolve(scriptDirectory, '..')
const skillRoot = path.join(repositoryRoot, 'creative-skills/v1.2.0')
const manifestPath = path.join(
  repositoryRoot,
  'src-tauri/resources/creative-skills-v1.2.0.deployment.json',
)

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(left, 'utf8'), Buffer.from(right, 'utf8'))
}

function isSafeRelativePath(relativePath) {
  return typeof relativePath === 'string'
    && relativePath.length > 0
    && !relativePath.startsWith('/')
    && !relativePath.includes('\\')
    && !relativePath.includes(':')
    && relativePath.split('/').every(part => part.length > 0 && part !== '.' && part !== '..')
}

function listFiles(root, current = root, files = []) {
  const entries = fs.readdirSync(current, { withFileTypes: true })
    .sort((left, right) => utf8Compare(left.name, right.name))

  for (const entry of entries) {
    const absolute = path.join(current, entry.name)
    const relative = path.relative(root, absolute).split(path.sep).join('/')
    if (relative === DEPLOYMENT_MANIFEST_NAME) continue
    if (!isSafeRelativePath(relative)) {
      throw new Error('SKILL_DEPLOYMENT_PATH_INVALID:' + relative)
    }
    if (entry.isSymbolicLink()) {
      throw new Error('SKILL_DEPLOYMENT_SYMLINK_NOT_ALLOWED:' + relative)
    }
    if (entry.isDirectory()) {
      listFiles(root, absolute, files)
    } else if (entry.isFile()) {
      const bytes = fs.readFileSync(absolute)
      files.push({
        path: relative,
        sizeBytes: bytes.length,
        sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
      })
    } else {
      throw new Error('SKILL_DEPLOYMENT_ENTRY_TYPE_NOT_ALLOWED:' + relative)
    }
  }
  return files
}

export function createSkillDeploymentManifest(
  packageRoot,
  sourceArchiveSha256,
  provenanceSourceArchiveSha256,
) {
  const files = listFiles(path.resolve(packageRoot)).sort((left, right) => utf8Compare(left.path, right.path))
  return {
    schema: DEPLOYMENT_MANIFEST_SCHEMA,
    packageVersion: '1.2.0',
    sourceArchiveSha256,
    provenanceSourceArchiveSha256,
    hashAlgorithm: HASH_ALGORITHM,
    manifestDigestAlgorithm: MANIFEST_DIGEST_ALGORITHM,
    pathEncoding: PATH_ENCODING,
    fileCount: files.length,
    files,
  }
}

export function serializeSkillDeploymentManifest(manifest) {
  return JSON.stringify(manifest, null, 2) + '\n'
}

export function verifySkillDeploymentManifest(packageRoot, committedBytes, sourceArchiveSha256, provenanceSourceArchiveSha256) {
  const expected = serializeSkillDeploymentManifest(
    createSkillDeploymentManifest(packageRoot, sourceArchiveSha256, provenanceSourceArchiveSha256),
  )
  const actual = Buffer.isBuffer(committedBytes) ? committedBytes.toString('utf8') : committedBytes
  if (actual !== expected) throw new Error('SKILL_DEPLOYMENT_MANIFEST_STALE')
  return crypto.createHash('sha256').update(actual, 'utf8').digest('hex')
}

function approvedPackageArchiveHash() {
  const rust = fs.readFileSync(path.join(repositoryRoot, 'src-tauri/src/skill_deployment.rs'), 'utf8')
  const match = rust.match(/EXPECTED_PACKAGE_ARCHIVE_SHA256:\s*&str\s*=\s*\n?\s*"([0-9a-f]{64})"/)
  if (!match) throw new Error('SKILL_DEPLOYMENT_ARCHIVE_PIN_MISSING')
  return match[1]
}

function provenanceArchiveHash() {
  const provenance = JSON.parse(fs.readFileSync(path.join(skillRoot, 'BUILD_PROVENANCE.json'), 'utf8'))
  if (provenance.package_version !== '1.2.0' || !/^[0-9a-f]{64}$/.test(provenance.source_archive_sha256)) {
    throw new Error('SKILL_BUILD_PROVENANCE_INVALID')
  }
  return provenance.source_archive_sha256
}

function main() {
  const mode = process.argv[2] ?? '--check'
  const archiveHash = approvedPackageArchiveHash()
  const provenanceHash = provenanceArchiveHash()
  const manifest = createSkillDeploymentManifest(skillRoot, archiveHash, provenanceHash)
  const expectedBytes = serializeSkillDeploymentManifest(manifest)

  if (mode === '--write') {
    fs.mkdirSync(path.dirname(manifestPath), { recursive: true })
    fs.writeFileSync(manifestPath, expectedBytes, { flag: 'w' })
  } else if (mode === '--check') {
    const actual = fs.readFileSync(manifestPath)
    verifySkillDeploymentManifest(skillRoot, actual, archiveHash, provenanceHash)
  } else {
    throw new Error('USAGE: node scripts/generate-skill-deployment-manifest.mjs [--check|--write]')
  }

  const digest = crypto.createHash('sha256').update(expectedBytes, 'utf8').digest('hex')
  process.stdout.write(JSON.stringify({
    status: 'PASS',
    packageVersion: manifest.packageVersion,
    sourceArchiveSha256: manifest.sourceArchiveSha256,
    fileCount: manifest.fileCount,
    deployedManifestDigest: digest,
    manifestPath: path.relative(repositoryRoot, manifestPath),
  }) + '\n')
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main()
  } catch (error) {
    process.stderr.write(error.message + '\n')
    process.exitCode = 1
  }
}
