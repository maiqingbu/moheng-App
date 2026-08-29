import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const read = path => readFileSync(resolve(root, path), 'utf8')
const workspace = read('src/ui/pages/VolumeWorkspace.vue')
const manager = read('src/ui/pages/VolumeManager.vue')
const harness = read('src-tauri/src/harness/mod.rs')
const commands = read('src-tauri/src/commands.rs')
const forbidden = {
  workspacePlannedChapters: /plannedChapters/.test(workspace),
  workspaceArcTimeline: /detail\.arcs/.test(workspace),
  managerNarrativePercent: /volume\.progress|plannedChapters/.test(manager),
  h02WriteCapability: /diagnose_planning_constraints[\s\S]{0,300}WRITE/.test(harness),
  b08Renderer: /B08|FinalChapter production|NarrativeRuntimeDelta/.test(commands),
}
const failures = Object.entries(forbidden).filter(([, value]) => value)
console.log(`B07_H02_ARCHITECTURE mandatory_arc_runtime_count=${Number(forbidden.workspaceArcTimeline)} legacy_volume_percent_as_narrative_truth_count=${Number(forbidden.managerNarrativePercent || forbidden.workspacePlannedChapters)} h02_write_capability_count=${Number(forbidden.h02WriteCapability)} b08_renderer_count=${Number(forbidden.b08Renderer)} status=${failures.length ? 'FAIL' : 'PASS'}`)
if (failures.length) process.exitCode = 1
