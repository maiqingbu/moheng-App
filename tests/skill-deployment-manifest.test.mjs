import assert from 'node:assert/strict'
import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import {
  createSkillDeploymentManifest,
  serializeSkillDeploymentManifest,
  verifySkillDeploymentManifest,
} from '../scripts/generate-skill-deployment-manifest.mjs'

function temporaryPackage(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'skill-deployment-manifest-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  return root
}

test('creates a UTF-8 sorted manifest from exact file bytes and excludes the manifest itself', t => {
  const root = temporaryPackage(t)
  fs.mkdirSync(path.join(root, 'skills/example/resources'), { recursive: true })
  fs.writeFileSync(path.join(root, 'registry.json'), '{"version":"1.2.0"}')
  fs.writeFileSync(path.join(root, 'skills/example/SKILL.md'), '方法正文\r\n')
  fs.writeFileSync(path.join(root, 'deployment-manifest.json'), 'old manifest')

  const manifest = createSkillDeploymentManifest(root, 'a'.repeat(64), 'b'.repeat(64))
  const paths = manifest.files.map(file => file.path)

  assert.deepEqual(paths, ['registry.json', 'skills/example/SKILL.md'])
  assert.equal(manifest.fileCount, 2)
  assert.equal(manifest.files[1].sha256, crypto.createHash('sha256').update('方法正文\r\n').digest('hex'))
  assert.equal(verifySkillDeploymentManifest(
    root,
    serializeSkillDeploymentManifest(manifest),
    'a'.repeat(64),
    'b'.repeat(64),
  ).length, 64)
})

test('fails manifest verification when a deployed asset byte changes', t => {
  const root = temporaryPackage(t)
  fs.mkdirSync(path.join(root, 'skills/example'), { recursive: true })
  fs.writeFileSync(path.join(root, 'skills/example/SKILL.md'), 'approved')
  const manifest = createSkillDeploymentManifest(root, 'a'.repeat(64), 'b'.repeat(64))
  const bytes = serializeSkillDeploymentManifest(manifest)
  fs.writeFileSync(path.join(root, 'skills/example/SKILL.md'), 'changed')

  assert.throws(
    () => verifySkillDeploymentManifest(root, bytes, 'a'.repeat(64), 'b'.repeat(64)),
    /SKILL_DEPLOYMENT_MANIFEST_STALE/,
  )
})

test('rejects symlinked method resources from the deployment manifest', t => {
  const root = temporaryPackage(t)
  const outside = path.join(path.dirname(root), 'outside-skill-method.md')
  fs.writeFileSync(outside, 'outside')
  t.after(() => fs.rmSync(outside, { force: true }))
  fs.mkdirSync(path.join(root, 'skills/example'), { recursive: true })
  fs.symlinkSync(outside, path.join(root, 'skills/example/SKILL.md'))

  assert.throws(
    () => createSkillDeploymentManifest(root, 'a'.repeat(64), 'b'.repeat(64)),
    /SKILL_DEPLOYMENT_SYMLINK_NOT_ALLOWED/,
  )
})
