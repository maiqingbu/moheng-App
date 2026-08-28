import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import crypto from 'node:crypto'

const root = path.resolve('src')
const files = []
const walk = dir => {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name)
    const stat = fs.statSync(file)
    if (stat.isDirectory()) walk(file)
    else if (/\.(ts|vue|css)$/.test(name)) files.push(file)
  }
}
walk(root)

const sourceByPath = new Map(files.map(file => [file, fs.readFileSync(file, 'utf8')]))
const read = relative => sourceByPath.get(path.join(root, relative))
const text = [...sourceByPath.values()].join('\n')
const projectRoot = path.dirname(root)
const contractRoot = path.join(projectRoot, 'contracts')
const generatedRoot = path.join(root, 'generated/contracts')
const rustGeneratedRoot = path.join(projectRoot, 'src-tauri/src/generated/contracts')
const manifestPath = path.join(contractRoot, 'manifests/schema-manifest.json')
const errors = []

const frozenDocuments = {
  'docs/architecture/Narrative_Runtime_Architecture_Amendment_R1_FROZEN.md': '1207bc780fd63df5ce4dd8495432c5ec518ee2b3f56acfd8ee37ff1c8adf101b',
  'docs/product/Narrative_Runtime_PRD_R1_FROZEN.md': '5cfc22ac950b799982302989695b026931b8eb2d54e1db86b0897ce37a71ab44',
  'docs/development/Narrative_Runtime_Batch_Master_Plan_R1_FROZEN.md': '71e07de16c609c381c3fe27dd75b574d1a973488792e9d910af9958fff0fa206',
}
const frozenMigrations = {
  'src-tauri/migrations/0001_catalog.sql': 'b56e29641a2edeb1cae02508c8c38363c9abdd97ef56c86d926668c121ab22e1',
  'src-tauri/migrations/0002_project_authority.sql': '7396fe1ec7769a898d423c97c3d063d7a64c3dc217de3967332bd25b84864924',
  'src-tauri/migrations/0003_project_command.sql': '2734404c53a80872b29d5fc4b455830e9fa8bda9d52760d0b812d9370e1630c7',
  'src-tauri/migrations/0004_project_longterm.sql': 'd06fbae9138bc15a72012b1e2b7ce063a5d25b3d54fa5f9deafe8bdcb6c5ae11',
  'src-tauri/migrations/0005_m0c_commit_delta.sql': 'e13b964f497c6398d912dc7f0ded92f39199de6f79f6d40043116e5d51a31d70',
  'src-tauri/migrations/0006_m0e_ops.sql': '13f11ea79c897a56580251fdf6b88b8babba4c67e623710246f56f694c516305',
}
const digestFile = relative => {
  const file = path.join(projectRoot, relative)
  if (!fs.existsSync(file)) return null
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}
for (const [relative, expected] of Object.entries(frozenDocuments)) {
  if (digestFile(relative) !== expected) errors.push(`R1 frozen document digest mismatch: ${relative}`)
}
for (const [relative, expected] of Object.entries(frozenMigrations)) {
  if (digestFile(relative) !== expected) errors.push(`frozen migration digest mismatch: ${relative}`)
}
for (const adr of ['ADR-026-goal-constrained-rolling-planning.md', 'ADR-027-continuity-thread-is-a-projection.md', 'ADR-028-exit-state-is-closure-snapshot.md', 'ADR-029-relevant-state-digest-determines-plan-staleness.md', 'ADR-030-single-production-path-and-deferred-legacy-removal.md']) {
  if (!fs.existsSync(path.join(projectRoot, 'docs/adr', adr))) errors.push(`缺少 R1 ADR: ${adr}`)
}

// Rust authoritative write boundary: SQL may live only in migration/project DB,
// catalog, repository, or CommitService ownership modules.
const rustRoot = path.join(projectRoot, 'src-tauri/src')
const rustFiles = []
const walkRust = dir => {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name)
    if (fs.statSync(file).isDirectory()) walkRust(file)
    else if (name.endsWith('.rs')) rustFiles.push(file)
  }
}
walkRust(rustRoot)
const sqlOwnerPrefixes = ['catalog/', 'commit/', 'project_db/', 'repositories/']
for (const file of rustFiles) {
  const relative = path.relative(rustRoot, file).split(path.sep).join('/')
  const source = fs.readFileSync(file, 'utf8')
  const hasSql = /\b(?:INSERT\s+INTO|UPDATE\s+\w+|DELETE\s+FROM|SELECT\s+.+\s+FROM)\b/i.test(source)
  if (hasSql && !sqlOwnerPrefixes.some(prefix => relative.startsWith(prefix))) {
    errors.push(`Rust SQL 越过 authoritative write/query owner: ${relative}`)
  }
}

if (!fs.existsSync(manifestPath)) errors.push('缺少 Schema manifest 唯一真源')
for (const generatedPath of [generatedRoot, rustGeneratedRoot]) {
  if (!fs.existsSync(generatedPath)) errors.push(`缺少生成合同目录 ${generatedPath}`)
}
for (const token of ['localStorage', 'sessionStorage', 'indexedDB']) {
  if (text.includes(token)) errors.push(`禁止使用 ${token} 冒充权威状态`)
}
const narrativeSchemaNames = ['book-plan-r1', 'volume-intent-r1', 'volume-plan-r1', 'context-manifest-r1', 'chapter-plan-r1', 'chapter-exit-state-r1', 'volume-exit-state-r1', 'task-progress', 'continuity-thread-projection', 'volume-progress-projection']
const schemaManifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
for (const name of narrativeSchemaNames) {
  if (!schemaManifest.schemas.some(entry => entry.name === name)) errors.push(`缺少 B05 Narrative schema: ${name}`)
}
const productionRustText = rustFiles.filter(file => !file.includes(`${path.sep}tests${path.sep}`)).map(file => fs.readFileSync(file, 'utf8')).join('\n')
const productionText = `${productionRustText}\n${text}`
const migrationText = fs.readdirSync(path.join(projectRoot, 'src-tauri/migrations'))
  .filter(name => name.endsWith('.sql'))
  .map(name => fs.readFileSync(path.join(projectRoot, 'src-tauri/migrations', name), 'utf8'))
  .join('\n')
const forbiddenAuthorityTables = ['continuity_threads', 'narrative_state', 'memory_store', 'future_reservations', 'volume_task_graph', 'rolling_horizon', 'timeline_events_v2']
const forbiddenAuthorityTableCount = forbiddenAuthorityTables.reduce((count, table) => count +
  [...`${productionRustText}\n${migrationText}`.matchAll(new RegExp(`CREATE\\s+TABLE(?:\\s+IF\\s+NOT\\s+EXISTS)?\\s+${table}\\b`, 'ig'))].length, 0)
const legacyPlannerCount = [...productionText.matchAll(/\bLegacyPlanner\b/g)].length
const narrativeRuntimeDeltaCount = [...productionText.matchAll(/\bNarrativeRuntimeDelta\b/g)].length
const commitBundleV3Count = [...productionText.matchAll(/CommitBundle\s+V3/g)].length
if (forbiddenAuthorityTableCount > 0) errors.push(`B05 forbidden authority table declarations: ${forbiddenAuthorityTableCount}`)
if (legacyPlannerCount > 0 || narrativeRuntimeDeltaCount > 0 || commitBundleV3Count > 0) {
  errors.push(`B05 forbidden production implementation symbols: LegacyPlanner=${legacyPlannerCount}, NarrativeRuntimeDelta=${narrativeRuntimeDeltaCount}, CommitBundle V3=${commitBundleV3Count}`)
}
const frozenRoutes = [
  '/projects', '/projects/new', '/projects/:id', '/projects/:id/market', '/projects/:id/directions',
  '/projects/:id/production', '/projects/:id/volumes', '/projects/:id/volumes/:volumeId',
  '/projects/:id/decisions', '/projects/:id/read', '/projects/:id/graph', '/projects/:id/health',
  '/projects/:id/history', '/projects/:id/settings', '/settings'
]
const router = read('app/router.ts')
for (const route of frozenRoutes) {
  if (!router.includes(`path:'${route}'`) && !router.includes(`path: '${route}'`)) errors.push(`缺少冻结信息架构路由 ${route}`)
}

const contracts = read('application/contracts.ts')
for (const contract of ['BookPlanR1', 'VolumeIntentR1', 'VolumePlanR1', 'VolumeTask', 'TaskStatus', 'ContextManifestR1', 'ChapterPlanR1', 'ContinuityThreadProjection', 'VolumeProgressProjection']) {
  if (!contracts.includes(contract)) errors.push(`缺少 Frontend Narrative contract: ${contract}`)
}
for (const contract of [
  'ProjectQueryRequest', 'PageResult', 'queryProjectOverview', 'queryMarketStrategy', 'queryStoryDirections',
  'queryProductionCenter', 'queryVolumeList', 'queryVolumeDetail', 'queryDecisions', 'queryReaderVolumes',
  'queryChapterList', 'queryChapterFinal', 'queryChapterVersions', 'queryCanon', 'queryBookHealth',
  'queryHistory', 'queryProjectSettings', 'searchFulltext'
]) {
  if (!contracts.includes(contract)) errors.push(`缺少长篇 Query 合同：${contract}`)
}
for (const command of [
  'book_plan_create', 'book_plan_revise', 'volume_plan_build', 'volume_production_start',
  'volume_production_pause', 'volume_close', 'chapter_revision_request', 'change_request_create',
  'change_request_apply', 'retcon_plan_create', 'retcon_apply', 'market_pulse_refresh',
  'backup_create', 'integrity_check_run'
]) {
  if (!contracts.includes(`'${command}'`)) errors.push(`缺少冻结 Command：${command}`)
}

const uiFiles = files.filter(file => file.includes(`${path.sep}ui${path.sep}`))
const ui = uiFiles.map(file => sourceByPath.get(file)).join('\n')
if (ui.includes('queryProjectBundle')) errors.push('UI 仍在查询整项目 Bundle')
if (/store\.bundle|projectStore\.bundle/.test(ui)) errors.push('Pinia/UI 仍持有整项目 Bundle')

const store = read('ui/stores/projectStore.ts')
for (const query of ['queryProjectOverview', 'queryChapterList', 'queryChapterFinal', 'queryBookHealth']) {
  if (!store.includes(query)) errors.push(`项目 Store 未通过细粒度查询：${query}`)
}

const appShell = read('ui/components/AppShell.vue')
for (const label of ['总览', '市场与平台策略', '故事方向', 'AI 制片中心', '卷管理', '决策收件箱', '成品阅读', '作品图谱', '长篇健康', '版本 / 修改 / Retcon', '项目设置']) {
  if (!appShell.includes(label)) errors.push(`侧栏缺少工作区：${label}`)
}

const reader = read('ui/pages/ManuscriptReader.vue')
for (const scope of ['chapter', 'range', 'volume', 'book']) {
  if (!reader.includes(`value=\"${scope}\"`)) errors.push(`成品阅读缺少导出范围：${scope}`)
}
if (!reader.includes("type: 'export_create'")) errors.push('成品阅读未通过 Command 创建导出任务')

const settings = read('ui/pages/SettingsPage.vue')
if (!settings.includes("type:'backup_create'")) errors.push('项目设置缺少备份 Command')
if (!settings.includes("type:'integrity_check_run'")) errors.push('项目设置缺少完整性检查 Command')

const runtimePort = read('app/providers/runtimePort.ts')
if (!runtimePort.includes('VITE_BROWSER_DEMO') || !runtimePort.includes('已拒绝回退到浏览器 Demo 状态')) {
  errors.push('正式组合根未显式隔离浏览器 Demo 适配器')
}

if (errors.length) {
  console.error('Architecture verification failed:\n- ' + errors.join('\n- '))
  process.exit(1)
}
console.log(`B05_ARCHITECTURE_RESULT docs=PASS frozen_migrations=PASS no_second_authority=PASS forbidden_authority_tables=${forbiddenAuthorityTableCount} legacy_planner_count=${legacyPlannerCount} narrative_runtime_delta_count=${narrativeRuntimeDeltaCount} commit_bundle_v3_count=${commitBundleV3Count} status=PASS`)
console.log(`Architecture verification passed: ${files.length} source files scanned; scoped queries, frozen IA, command boundaries and demo isolation checks passed.`)
