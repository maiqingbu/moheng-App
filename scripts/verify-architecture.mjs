import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

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
console.log(`Architecture verification passed: ${files.length} source files scanned; scoped queries, frozen IA, command boundaries and demo isolation checks passed.`)
