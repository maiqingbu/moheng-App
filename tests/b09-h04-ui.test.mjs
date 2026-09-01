import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '..')
const workspace = fs.readFileSync(path.join(root, 'src/ui/pages/VolumeWorkspace.vue'), 'utf8')

test('B09 cross-volume workspace renders backend lifecycle truth', () => {
  for (const marker of [
    'routeReplans', 'validationStatus', 'newVersion', 'latestHandoff',
    'fromVolumeId', 'toVolumeId', 'handoffStatus', 'unresolvedResponsibilities',
    'OPEN', 'SUSPENDED', 'closeReadiness',
  ]) assert.match(workspace, new RegExp(marker), marker)
})
