import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

function frontendCommands(source) {
  return [...source.matchAll(/invokeQuery(?:<[^>]+>)?\(this\.ipc,\s*'([^']+)'/g)]
    .map(match => match[1])
    .filter((command, index, all) => all.indexOf(command) === index)
}

function handlerFunctions(source) {
  const block = source.match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1] ?? ''
  return [...block.matchAll(/\b([A-Za-z_]\w*)\s*,/g)]
    .map(match => match[1])
    .filter((command, index, all) => all.indexOf(command) === index)
}

export function inspectB11CapabilityParity(projectRoot = root) {
  const frontend = fs.readFileSync(path.join(projectRoot, 'src/infrastructure/tauri/frontendPort.ts'), 'utf8')
  const commands = fs.readFileSync(path.join(projectRoot, 'src-tauri/src/commands.rs'), 'utf8')
  const router = fs.readFileSync(path.join(projectRoot, 'src/app/router.ts'), 'utf8')
  const shell = fs.readFileSync(path.join(projectRoot, 'src/ui/components/AppShell.vue'), 'utf8')
  const overview = fs.readFileSync(path.join(projectRoot, 'src/ui/pages/ProjectOverview.vue'), 'utf8')
  const creation = fs.readFileSync(path.join(projectRoot, 'src/ui/pages/CreationLaunch.vue'), 'utf8')
  const market = fs.readFileSync(path.join(projectRoot, 'src/ui/pages/MarketStrategy.vue'), 'utf8')
  const assistant = fs.readFileSync(path.join(projectRoot, 'src/ui/pages/ProjectAssistant.vue'), 'utf8')
  const store = fs.readFileSync(path.join(projectRoot, 'src/ui/stores/projectStore.ts'), 'utf8')
  const invokedCommands = frontendCommands(frontend)
  const registeredFunctions = handlerFunctions(commands)
  const registeredCommands = new Set(registeredFunctions)
  const missingHandlers = invokedCommands
    .filter(command => !registeredCommands.has(command))
    .map(command => ({ command, expectedHandler: command }))
  const healthRoute = router.split('\n').find(line => line.includes("/projects/:id/health")) ?? ''
  const historyRoute = router.split('\n').find(line => line.includes("/projects/:id/history")) ?? ''
  const frozenIa = {
    healthRouteExists: healthRoute.length > 0,
    historyRouteExists: historyRoute.length > 0,
    healthRouteUnavailable: /UnavailableFeature/.test(healthRoute) && !/LongFormHealth|query_book_health/.test(healthRoute),
    healthRouteActive: /LongFormHealth/.test(healthRoute) && !/UnavailableFeature/.test(healthRoute),
    historyRouteUnavailable: /UnavailableFeature/.test(historyRoute) && !/HistoryCenter|change_request_apply|retcon_apply/.test(historyRoute),
    historyRouteActive: /HistoryCenter/.test(historyRoute) && !/UnavailableFeature/.test(historyRoute),
    healthSidebarPreserved: (/长篇健康/.test(shell) && /尚未开放/.test(shell) && /disabled/.test(shell))
      || (/长篇健康/.test(shell) && /\/health/.test(shell) && !/disabled: true/.test(shell)),
    historySidebarPreserved: (/版本与变更记录/.test(shell) && /尚未开放/.test(shell) && /disabled/.test(shell))
      || (/版本与变更记录/.test(shell) && /match: 'history'/.test(shell) && !/match: 'history', disabled: true/.test(shell)),
  }
  const futureRouteLeaks = [
    frozenIa.healthRouteExists && !frozenIa.healthRouteUnavailable && !frozenIa.healthRouteActive ? 'health route is executable without a declared owner' : null,
    frozenIa.historyRouteExists && !frozenIa.historyRouteUnavailable && !frozenIa.historyRouteActive ? 'history route is executable without a declared B12 owner' : null,
    !frozenIa.healthRouteExists ? 'health route missing' : null,
    !frozenIa.historyRouteExists ? 'history route missing' : null,
    !frozenIa.healthSidebarPreserved ? 'health sidebar item missing or enabled' : null,
    !frozenIa.historySidebarPreserved ? 'history sidebar item missing or enabled' : null,
  ].filter(Boolean)
  const unsupportedMarketActions = ['market_refresh', 'market_pulse_refresh']
    .filter(marker => creation.includes(marker) || market.includes(marker))
  const formalUiActions = {
    projectAssistant: ['support_session', 'query_assistant_repair_plan', 'repair_plan_create', 'repair_authorize', 'repair_execute'],
    projectRead: ['query_volume_detail', 'query_decisions', 'query_chapter_versions', 'query_canon'],
    marketEntry: ['project_create_from_market', 'query_market_opportunities'],
  }
  const formalSources = `${frontend}\n${assistant}\n${store}\n${creation}\n${market}`
  const missingFormalActions = Object.fromEntries(Object.entries(formalUiActions).map(([surface, actions]) => [
    surface,
    actions.filter(action => !formalSources.includes(action)
      || (!invokedCommands.includes(action) && !registeredCommands.has(action) && !registeredCommands.has('execute_command'))),
  ]))
  const status = missingHandlers.length === 0
    && futureRouteLeaks.length === 0
    && unsupportedMarketActions.length === 0
    && Object.values(missingFormalActions).every(actions => actions.length === 0)
    ? 'PASS'
    : 'FAIL'
  return {
    schema: 'b11-ui-capability-parity-r1',
    status,
    invokedCommands,
    registeredHandlers: registeredFunctions,
    missingHandlers,
    formalUiActions,
    missingFormalActions,
    frozenIa,
    futureRouteLeaks,
    unsupportedMarketActions,
  }
}

if (process.argv[1]?.endsWith('verify-b11-capability-parity.mjs')) {
  const report = inspectB11CapabilityParity()
  console.log(JSON.stringify(report, null, 2))
  if (report.status !== 'PASS') process.exitCode = 1
}
