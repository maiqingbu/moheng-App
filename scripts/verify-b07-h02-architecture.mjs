import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const read = path => readFileSync(resolve(root, path), 'utf8')
const workspace = read('src/ui/pages/VolumeWorkspace.vue')
const manager = read('src/ui/pages/VolumeManager.vue')
const harness = read('src-tauri/src/harness/mod.rs')
const commands = read('src-tauri/src/commands.rs')
const frontendPort = read('src/infrastructure/tauri/frontendPort.ts')
const forbidden = {
  workspacePlannedChapters: /plannedChapters/.test(workspace),
  workspaceArcTimeline: /detail\.arcs/.test(workspace),
  managerNarrativePercent: /volume\.progress|plannedChapters/.test(manager),
  h02WriteCapability: /diagnose_planning_constraints[\s\S]{0,300}WRITE/.test(harness),
  b08Renderer: /B08|FinalChapter production|NarrativeRuntimeDelta/.test(commands),
}
const requiredWorkspace = {
  volumeGoal: /Volume Goal|卷目标/.test(workspace),
  requiredTasks: /REQUIRED/.test(workspace),
  optionalTasks: /OPTIONAL/.test(workspace),
  flexibleTasks: /FLEXIBLE/.test(workspace),
  blockedTasks: /BLOCKED/.test(workspace),
  continuity: /continuityThreads|Continuity/.test(workspace),
  reservations: /futureReservations|Reservations/.test(workspace),
  chapterPlan: /currentChapterPlan|ChapterPlan/.test(workspace),
  horizon: /horizonPreview|Rolling Horizon/.test(workspace),
  tentative: /TENTATIVE|tentative/.test(workspace),
  taskReality: /taskReality|Task Reality/.test(workspace),
}
const forbiddenProductionActions = ['volume_plan_build', 'volume_production_start', 'volume_production_pause', 'volume_close']
const managerProductionActionsRemoved = forbiddenProductionActions.every(action => !manager.includes(action))
const productionPlanningOwner = /planning_create_volume_intent/.test(manager) && /planning_create_volume_plan/.test(manager) && /planning_create_volume_intent/.test(commands)
const failures = [
  ...Object.entries(forbidden).filter(([, value]) => value).map(([name]) => name),
  ...Object.entries(requiredWorkspace).filter(([, value]) => !value).map(([name]) => `workspace_${name}`),
  ...(!managerProductionActionsRemoved ? ['manager_demo_only_action'] : []),
  ...(!productionPlanningOwner ? ['manager_planning_owner_missing'] : []),
  ...(!/queryPlanRoute/.test(frontendPort) ? ['frontend_query_plan_route_missing'] : []),
]
console.log(`B07_H02_ARCHITECTURE mandatory_arc_runtime_count=${Number(forbidden.workspaceArcTimeline)} legacy_volume_percent_as_narrative_truth_count=${Number(forbidden.managerNarrativePercent || forbidden.workspacePlannedChapters)} h02_write_capability_count=${Number(forbidden.h02WriteCapability)} b08_renderer_count=${Number(forbidden.b08Renderer)} workspace_projection_fields=${Object.values(requiredWorkspace).filter(Boolean).length}/${Object.keys(requiredWorkspace).length} production_planning_owner_count=${Number(productionPlanningOwner)} demo_only_action_count=${Number(!managerProductionActionsRemoved)} status=${failures.length ? 'FAIL' : 'PASS'}`)
if (failures.length) process.exitCode = 1
