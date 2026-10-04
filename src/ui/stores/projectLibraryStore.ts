import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type {
  CommandRequest,
  CommandStatus,
  MarketDiscoveryProjection,
  ProjectLibraryProjection,
  SettingsProjection,
  ProviderSettingsInput
} from '../../application/contracts'
import { getFrontendPort } from '../../app/providers/runtimePort'
import { receiptSummary } from '../presentation/productLanguage'

const frontendPort = getFrontendPort()

export interface LibraryCommandUiState {
  status: CommandStatus
  message: string
  commandId?: string
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) return String(error.message)
  return '操作未完成。'
}

export const useProjectLibraryStore = defineStore('project-library-projection', () => {
  const projection = ref<ProjectLibraryProjection | null>(null)
  const market = ref<MarketDiscoveryProjection>({
    status: 'NOT_CONFIGURED',
    policyStatus: 'NOT_CONFIGURED',
    policyReferences: [],
    opportunities: [],
    limitations: []
  })
  const globalSettings = ref<SettingsProjection | null>(null)
  const loading = ref(false)
  const marketLoading = ref(false)
  const loadError = ref('')
  const commandStates = ref<Record<string, LibraryCommandUiState>>({})

  const projects = computed(() => projection.value?.projects ?? [])
  const activeProjects = computed(() => projects.value.filter(item => item.lifecycle === 'ACTIVE'))
  const archivedProjects = computed(() => projects.value.filter(item => item.lifecycle === 'ARCHIVED'))
  const recentProjects = computed(() => activeProjects.value.slice().sort((a, b) => b.lastOpenedAt.localeCompare(a.lastOpenedAt)).slice(0, 6))

  async function bootstrap() {
    if (projection.value || loading.value) return
    await refresh()
  }

  async function refresh() {
    loading.value = true
    loadError.value = ''
    try {
      projection.value = await frontendPort.queryProjectLibrary()
    } catch (error) {
      loadError.value = error instanceof Error ? error.message : '无法读取作品库。'
    } finally {
      loading.value = false
    }
  }

  async function loadGlobalSettings() {
    if (globalSettings.value) return
    globalSettings.value = await frontendPort.queryGlobalSettings()
  }

  async function testProvider(key: string, settings: ProviderSettingsInput) {
    commandStates.value[key] = { status: 'submitting', message: '正在提交诊断…' }
    try {
      const receipt = await frontendPort.testProvider(settings, (status, message) => {
        commandStates.value[key] = { ...commandStates.value[key], status, message }
      })
      commandStates.value[key] = { status: receipt.finalStatus, message: receiptSummary(receipt).detail, commandId: receipt.commandId }
      return receipt
    } catch (error) {
      const message = error instanceof Error ? error.message : typeof error === 'string' ? error : error && typeof error === 'object' && 'message' in error ? String(error.message) : JSON.stringify(error) || String(error)
      commandStates.value[key] = { status: 'failed', message: message || 'Provider 测试未完成。' }
      throw error
    }
  }

  async function loadMarket(force = false) {
    if ((!force && market.value.status !== 'NOT_CONFIGURED') || marketLoading.value) return
    marketLoading.value = true
    try {
      market.value = await frontendPort.queryMarketOpportunities()
    } catch (error) {
      market.value = {
        ...market.value,
        status: 'SOURCE_UNAVAILABLE',
        policyStatus: 'SOURCE_UNAVAILABLE',
        opportunities: [],
        errorCode: error instanceof Error ? error.message : 'MARKET_QUERY_FAILED'
      }
    } finally {
      marketLoading.value = false
    }
  }

  async function runCommand(key: string, request: CommandRequest) {
    const commandId = request.commandId ?? `CMD-${Date.now()}-${Math.floor(Math.random() * 1000)}`
    const envelope: CommandRequest = {
      ...request,
      commandId,
      idempotencyKey: request.idempotencyKey ?? commandId,
      requestedAt: request.requestedAt ?? new Date().toISOString()
    }
    const projectScoped = envelope.projectId != null && !['project_create_from_idea', 'project_create_from_market', 'global_settings_save'].includes(envelope.type)
    commandStates.value[key] = { status: 'submitting', message: '正在提交…', commandId }
    let receipt: Awaited<ReturnType<typeof frontendPort.executeCommand>>
    try {
      receipt = await frontendPort.executeCommand(envelope, (status, message) => {
        if (commandStates.value[key]?.commandId !== commandId) return
        commandStates.value[key] = { ...commandStates.value[key], commandId, status, message }
      })
    } catch (error) {
      if (commandStates.value[key]?.commandId === commandId) {
        commandStates.value[key] = { status: 'failed', message: errorMessage(error), commandId }
      }
      throw error
    }
    if (receipt.commandId !== commandId || (projectScoped && receipt.projectId !== envelope.projectId)) {
      const error = new Error('PROTOCOL / PROJECT_BINDING_ERROR')
      if (commandStates.value[key]?.commandId === commandId) {
        commandStates.value[key] = { status: 'failed', message: error.message, commandId }
      }
      throw error
    }
    if (commandStates.value[key]?.commandId !== commandId) return receipt
    commandStates.value[key] = { status: receipt.finalStatus, message: receipt.message, commandId: receipt.commandId }
    // The project slot this receipt targets: for a duplicate the result is a
    // brand-new project (resultProjectId) added to the library, never an implicit
    // replacement of the active project unless the caller explicitly switches.
    if (receipt.finalStatus === 'committed') void refresh()
    return receipt
  }

  function commandState(key: string): LibraryCommandUiState {
    return commandStates.value[key] ?? { status: 'idle', message: '' }
  }

  function clearCommandState(key: string) {
    commandStates.value[key] = { status: 'idle', message: '' }
  }

  function findProject(projectId: string) {
    return projects.value.find(item => item.id === projectId)
  }

  return {
    projection,
    market,
    globalSettings,
    loading,
    marketLoading,
    loadError,
    projects,
    activeProjects,
    archivedProjects,
    recentProjects,
    bootstrap,
    refresh,
    loadMarket,
    loadGlobalSettings,
    testProvider,
    runCommand,
    commandState,
    clearCommandState,
    findProject
  }
})
