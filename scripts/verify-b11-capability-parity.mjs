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
  const futureRouteLeaks = ['/health', '/history', 'LongFormHealth', 'HistoryCenter']
    .filter(marker => router.includes(marker) || shell.includes(marker) || overview.includes(marker))
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
    futureRouteLeaks,
    unsupportedMarketActions,
  }
}

if (process.argv[1]?.endsWith('verify-b11-capability-parity.mjs')) {
  const report = inspectB11CapabilityParity()
  console.log(JSON.stringify(report, null, 2))
  if (report.status !== 'PASS') process.exitCode = 1
}
