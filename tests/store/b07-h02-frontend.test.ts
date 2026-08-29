import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { TauriFrontendPort } from '../../src/infrastructure/tauri/frontendPort'
import { BrowserDemoPort } from '../../src/infrastructure/browser-demo/demoPort'

describe('B07/H02 frontend boundary', () => {
  it('uses the real Tauri plan-route command', async () => {
    let command = ''
    const port = new TauriFrontendPort({ invoke: async name => { command = name; return { projectId: 'p', projectRevision: 1, volumeId: 'v', volumeIntentRef: { artifactId: 'intent', version: 1, payloadHash: 'a'.repeat(64) }, volumePlanRef: { artifactId: 'route', version: 1, payloadHash: 'b'.repeat(64) }, routeVersion: 1, tasks: [], taskReality: [], planningPressure: {}, truncated: false } } })
    const result = await port.queryPlanRoute({ projectId: 'p', scope: 'volume', volumeId: 'v' })
    expect(command).toBe('query_plan_route')
    expect(result.volumeId).toBe('v')
  })

  it('keeps browser demo from becoming production planning truth', async () => {
    await expect(new BrowserDemoPort().queryPlanRoute({ projectId: 'p', scope: 'volume', volumeId: 'v' })).rejects.toThrow('IPC_UNSUPPORTED:query_plan_route')
  })

  it('labels Volume Workspace plan and projection semantics', () => {
    const source = readFileSync(resolve(import.meta.dirname, '../../src/ui/pages/VolumeWorkspace.vue'), 'utf8')
    expect(source).toMatch(/PLAN/)
    expect(source).toMatch(/PROJECTION/)
    expect(source).not.toMatch(/detail\.arcs/)
  })
})
