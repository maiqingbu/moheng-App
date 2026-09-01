import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const baseline = '2f6ebc02ad999c43c322ea23b4e9d48148baee01'
const frozenMigrations = {
  'src-tauri/migrations/0001_catalog.sql': 'b56e29641a2edeb1cae02508c8c38363c9abdd97ef56c86d926668c121ab22e1',
  'src-tauri/migrations/0002_project_authority.sql': '7396fe1ec7769a898d423c97c3d063d7a64c3dc217de3967332bd25b84864924',
  'src-tauri/migrations/0003_project_command.sql': '2734404c53a80872b29d5fc4b455830e9fa8bda9d52760d0b812d9370e1630c7',
  'src-tauri/migrations/0004_project_longterm.sql': 'd06fbae9138bc15a72012b1e2b7ce063a5d25b3d54fa5f9deafe8bdcb6c5ae11',
  'src-tauri/migrations/0005_m0c_commit_delta.sql': 'e13b964f497c6398d912dc7f0ded92f39199de6f79f6d40043116e5d51a31d70',
  'src-tauri/migrations/0006_m0e_ops.sql': '13f11ea79c897a56580251fdf6b88b8babba4c67e623710246f56f694c516305',
  'src-tauri/migrations/0007_b05_narrative_foundation.sql': '90f148e5c8d3d9d72bfb2b17c00b06b14f6d201afe1fe5ebc1ee26f7b118d52f',
  'src-tauri/migrations/0008_h01_support_checkpoint.sql': '6cba8e0e1be0e24cebd8e66d2a0850552a27b56895a32db3577e434f25bb1d61',
  'src-tauri/migrations/0009_h03_repair_workflow.sql': 'b2ac4457e982b1b4e93d50b95e4c07d3e6a77f6b36ed2b796e2bc6ed576353ba',
  'src-tauri/migrations/0010_b09_h04_volume_lifecycle.sql': 'c1257d5a8c9d3b52cadb5f6973f6ac8764d9bbcb77ec84a4b036929bc313f99d',
}

const read = (projectRoot, relative) => fs.readFileSync(path.join(projectRoot, relative), 'utf8')
const exists = (projectRoot, relative) => fs.existsSync(path.join(projectRoot, relative))
const digest = (projectRoot, relative) => exists(projectRoot, relative)
  ? crypto.createHash('sha256').update(read(projectRoot, relative)).digest('hex')
  : null

const visibleFiles = [
  'src/ui/components/AppShell.vue',
  'src/ui/pages/CanonGraph.vue',
  'src/ui/pages/CreationLaunch.vue',
  'src/ui/pages/DecisionInbox.vue',
  'src/ui/pages/ManuscriptReader.vue',
  'src/ui/pages/MarketStrategy.vue',
  'src/ui/pages/OpeningReady.vue',
  'src/ui/pages/PitchReview.vue',
  'src/ui/pages/ProductionCenter.vue',
  'src/ui/pages/ProjectAssistant.vue',
  'src/ui/pages/ProjectOverview.vue',
  'src/ui/pages/ProjectLibrary.vue',
  'src/ui/pages/SettingsPage.vue',
  'src/ui/pages/StorySetup.vue',
  'src/ui/pages/VolumeManager.vue',
  'src/ui/pages/VolumeWorkspace.vue',
]

function templateVisibleText(source) {
  return [...source.matchAll(/<template>([\s\S]*?)<\/template>/g)]
    .map(match => match[1]
      .replace(/\{\{[\s\S]*?\}\}/g, ' ')
      .replace(/<[^>]*>/g, ' ')
      .replace(/&[^;]+;/g, ' ')
      .replace(/\s+/g, ' ')
      .trim())
    .join(' ')
}

export function scanB11UserVisibleStrings(projectRoot = root) {
  const inventory = []
  const leaks = []
  const forbidden = /\b(?:PLAN|REALITY|PROJECTION|TENTATIVE|Current|Context|Render|Audit|Gate|Commit|Reconcile|Candidate|Final|Owner|Stale|Retcon|Receipt|ChapterPlan|VolumeIntent|VolumePlan|SQLite|SQL|P3|B0[5-9]|H0[1-4]|RC1)\b|OPEN\s*\/\s*SUSPENDED|AI\s+NOVEL\s+PRODUCTION|RepairPlan|operationDigest|sourceRefs|IPC_UNSUPPORTED/i
  const boilerplate = /作为\s*AI|根据你提供的上下文|以下是详细分析|希望以上内容对你有所帮助|如有需要我可以继续/i
  const promptLeak = /system\s+prompt|hidden\s+instruction|ignore\s+previous|internal\s+method/i
  for (const relative of visibleFiles) {
    if (!exists(projectRoot, relative)) {
      leaks.push({ file: relative, reason: 'missing' })
      continue
    }
    const source = read(projectRoot, relative)
    const visible = templateVisibleText(source)
    inventory.push({ file: relative, text: visible })
    const technical = visible.match(forbidden)
    if (technical) leaks.push({ file: relative, reason: 'engineering', match: technical[0] })
    const templateBoilerplate = visible.match(boilerplate)
    if (templateBoilerplate) leaks.push({ file: relative, reason: 'ai-trace', match: templateBoilerplate[0] })
    const prompt = visible.match(promptLeak)
    if (prompt) leaks.push({ file: relative, reason: 'prompt-leak', match: prompt[0] })
  }
  const presentation = exists(projectRoot, 'src/ui/presentation/assistantPresentation.ts')
    ? read(projectRoot, 'src/ui/presentation/assistantPresentation.ts')
    : ''
  const presentationPolicyPass = /sanitizeAssistantText/.test(presentation)
    && /internalStatus/.test(presentation)
    && /jsonBlock/.test(presentation)
    && /internalReference/.test(presentation)
  const presentationLeaks = presentationPolicyPass ? [] : ['missing typed presentation sanitizer']
  return {
    schema: 'b11-user-visible-string-inventory-r1',
    files: inventory,
    scannedFileCount: inventory.length,
    leaks,
    presentationPolicy: 'typed-response-validation-and-user-facing-mapping',
    presentationLeaks,
    status: leaks.length === 0 && presentationPolicyPass ? 'PASS' : 'FAIL',
  }
}

function rustSourceFiles(projectRoot) {
  const directory = path.join(projectRoot, 'src-tauri/src')
  const files = []
  const visit = current => {
    if (!fs.existsSync(current)) return
    for (const item of fs.readdirSync(current, { withFileTypes: true })) {
      const full = path.join(current, item.name)
      if (item.isDirectory()) visit(full)
      else if (item.name.endsWith('.rs')) files.push(full)
    }
  }
  visit(directory)
  return files
}

export function inspectB11Architecture(projectRoot = root) {
  const entry = exists(projectRoot, 'src-tauri/src/project_entry.rs') ? read(projectRoot, 'src-tauri/src/project_entry.rs') : ''
  const entryRepository = exists(projectRoot, 'src-tauri/src/repositories/project_entry.rs') ? read(projectRoot, 'src-tauri/src/repositories/project_entry.rs') : ''
  const commands = exists(projectRoot, 'src-tauri/src/commands.rs') ? read(projectRoot, 'src-tauri/src/commands.rs') : ''
  const production = exists(projectRoot, 'src-tauri/src/production.rs') ? read(projectRoot, 'src-tauri/src/production.rs') : ''
  const narrativeRuntime = exists(projectRoot, 'src-tauri/src/narrative/runtime.rs') ? read(projectRoot, 'src-tauri/src/narrative/runtime.rs') : ''
  const planning = exists(projectRoot, 'src-tauri/src/planning.rs') ? read(projectRoot, 'src-tauri/src/planning.rs') : ''
  const router = exists(projectRoot, 'src/app/router.ts') ? read(projectRoot, 'src/app/router.ts') : ''
  const frontendPort = exists(projectRoot, 'src/infrastructure/tauri/frontendPort.ts') ? read(projectRoot, 'src/infrastructure/tauri/frontendPort.ts') : ''
  const runtimePort = exists(projectRoot, 'src/app/providers/runtimePort.ts') ? read(projectRoot, 'src/app/providers/runtimePort.ts') : ''
  const contracts = exists(projectRoot, 'src/application/contracts.ts') ? read(projectRoot, 'src/application/contracts.ts') : ''
  const assistant = exists(projectRoot, 'src/ui/pages/ProjectAssistant.vue') ? read(projectRoot, 'src/ui/pages/ProjectAssistant.vue') : ''
  const freeze = exists(projectRoot, 'evidence/B11/B11_PREBATCH_FREEZE.json')
    ? JSON.parse(read(projectRoot, 'evidence/B11/B11_PREBATCH_FREEZE.json'))
    : null
  const language = scanB11UserVisibleStrings(projectRoot)
  const allRust = rustSourceFiles(projectRoot).map(file => fs.readFileSync(file, 'utf8')).join('\n')
  const rustSqlOutsideOwners = rustSourceFiles(projectRoot)
    .filter(file => {
      const relative = path.relative(path.join(projectRoot, 'src-tauri/src'), file).split(path.sep)
      if (['catalog', 'commit', 'project_db', 'repositories'].includes(relative[0])) return false
      return /\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM)\b/i.test(fs.readFileSync(file, 'utf8'))
    })
    .map(file => path.relative(projectRoot, file))
  const frozenMigrationStatus = Object.entries(frozenMigrations).map(([file, expected]) => ({
    file,
    expected,
    actual: digest(projectRoot, file),
    unchanged: digest(projectRoot, file) === expected,
  }))
  const checks = {
    freezeManifest: freeze?.status === 'PASS'
      && freeze.freezeThrough === 'B10'
      && freeze.acceptedBaseline === baseline
      && freeze.nextAuthorizedBatch === 'B11'
      && JSON.stringify(freeze.doNotStart) === JSON.stringify(['B12', 'B13', 'B14', 'B15', 'RC1']),
    frozenMigrations: frozenMigrationStatus.every(item => item.unchanged),
    projectEntryOwner: /pub fn commit_initial/.test(entry)
      && /CommitService::commit_without_revision/.test(entry)
      && /pub fn commit_selection/.test(entry)
      && /pub fn commit_story_setup/.test(entry)
      && /pub fn commit_opening_ready/.test(entry),
    repositoryBoundary: /latest_artifact_by_type|canonical_head|ensure_opening_topology/.test(entryRepository)
      && !/\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM)\b/i.test(entry),
    explicitSelection: /explicitSelectionRequired/.test(entry)
      && /B11_SELECTION_DUPLICATE/.test(entry)
      && /explicitUserDecision/.test(entry),
    openingUsesExistingOwners: /B06::ContextCompilerOwner/.test(`${entry}\n${narrativeRuntime}`)
      && /B07::PlanningOwner/.test(`${entry}\n${planning}`)
      && /B08::ChapterProductionRuntime/.test(entry)
      && !/OpeningEngine|SecondOpeningEngine|B11::PlanningOwner/.test(`${entry}\n${commands}`),
    assistantQueryBoundary: /query_assistant_repair_plan/.test(commands)
      && /support_session/.test(commands)
      && /queryAssistantRepairPlan/.test(frontendPort)
      && /prepareAssistantRepair|authorizeAssistantRepair/.test(exists(projectRoot, 'src/ui/stores/projectStore.ts') ? read(projectRoot, 'src/ui/stores/projectStore.ts') : ''),
    b11Routes: /StorySetup|story-setup|\/setup/.test(router)
      && /OpeningReady|opening-ready|\/opening/.test(router)
      && /ProjectAssistant|assistant|\/assistant/.test(router),
    noSecondProductionPath: !/production_start_legacy|LegacyProduction|SecondProductionEngine|BrowserDemo/.test(`${production}\n${commands}`)
      && /run_chapter_production/.test(production),
    noFutureBatchOwner: !/B12|B13|B14|B15|RC1|EndgameOwner|CompletionGate|ExportReleaseOwner/i.test(allRust + contracts + router),
    noCommandsSql: !/\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM)\b/i.test(commands),
    noProductionSql: !/\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM)\b/i.test(production),
    frontendDemoFailClosed: /if \(invoke\) return new TauriFrontendPort\(invoke\)/.test(runtimePort)
      && /已拒绝回退到浏览器 Demo 状态/.test(runtimePort),
    languageAndLeakage: language.status === 'PASS',
    noUnboundedEntryInput: /MAX_IDEA_CHARS|MAX_MARKET_MATERIAL_CHARS/.test(entry),
  }
  const globalGates = {
    second_authority: !/SecondAuthority|secondAuthorityOwner|AuthorityV2/i.test(allRust),
    second_production_path: checks.noSecondProductionPath,
    commit_bundle_v3: !/CommitBundleV3/i.test(allRust + contracts),
    narrative_runtime_delta: !/NarrativeRuntimeDelta/i.test(allRust + contracts),
    sql_outside_approved_owner: rustSqlOutsideOwners.length === 0,
    skill_direct_db_query: !/use\s+libsql_rusqlite::Connection|query_row\(|execute_batch\(/.test(exists(projectRoot, 'src-tauri/src/creative_skills.rs') ? read(projectRoot, 'src-tauri/src/creative_skills.rs') : ''),
    harness_arbitrary_write: !/Command::new|std::process::Command|git\s+(?:reset|checkout|commit)/.test(exists(projectRoot, 'src-tauri/src/harness/mod.rs') ? read(projectRoot, 'src-tauri/src/harness/mod.rs') : ''),
    p3_permanent_authority: !/Skill[^\n]*(?:P3|AUTHORIZATION|consume)/i.test(exists(projectRoot, 'src-tauri/src/creative_skills.rs') ? read(projectRoot, 'src-tauri/src/creative_skills.rs') : ''),
    future_reservation_bypass: !/Skill[^\n]*futureReservation|bypass[^\n]*reservation/i.test(allRust),
    plan_writes_reality: !/Skill[^\n]*(?:TASK_REALITY_WRITE|write.*reality)/i.test(allRust),
    candidate_visible_as_final: !/Skill[^\n]*(?:FinalChapter|canonical head)/i.test(allRust),
    ui_local_truth: /queryProjectOverview|query_story_setup|query_opening_ready/.test(`${frontendPort}\n${assistant}`),
    unbounded_project_context: /MAX_CONTEXT_BYTES/.test(allRust),
    fake_provider_demo_formal_path: !/browser-demo|demoPort|mockProduction/.test(production),
    future_batch_owner_early: checks.noFutureBatchOwner,
  }
  const missingGates = Object.entries(checks).filter(([, passed]) => !passed).map(([name]) => name)
  const missingGlobalGates = Object.entries(globalGates).filter(([, passed]) => !passed).map(([name]) => name)
  return {
    schema: 'b11-architecture-r1',
    batch: 'B11',
    baseline,
    status: missingGates.length === 0 && missingGlobalGates.length === 0 ? 'PASS' : 'FAIL',
    missingGates: [...missingGates, ...missingGlobalGates],
    checks: Object.fromEntries(Object.entries(checks).map(([key, value]) => [key, value ? 'PASS' : 'FAIL'])),
    globalGates: Object.fromEntries(Object.entries(globalGates).map(([key, value]) => [key, value ? 'PASS' : 'FAIL'])),
    frozenMigrations: frozenMigrationStatus,
    language,
    rustSqlOutsideOwners,
  }
}

if (process.argv[1]?.endsWith('verify-b11-architecture.mjs')) {
  const result = inspectB11Architecture()
  console.log(JSON.stringify(result, null, 2))
  if (result.status !== 'PASS') process.exitCode = 1
}
