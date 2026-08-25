import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type {
  CommandRequest,
  CommandStatus,
  MarketOpportunity,
  ProjectLibraryProjection,
  SettingsProjection
} from '../../application/contracts'
import { getFrontendPort } from '../../app/providers/runtimePort'

const frontendPort = getFrontendPort()

export interface LibraryCommandUiState {
  status: CommandStatus
  message: string
  commandId?: string
}

export const useProjectLibraryStore = defineStore('project-library-projection', () => {
  const projection = ref<ProjectLibraryProjection | null>(null)
  const market = ref<MarketOpportunity[]>([])
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

  async function testProvider(key: string, settings: SettingsProjection) {
    commandStates.value[key] = { status: 'submitting', message: '正在提交诊断…' }
    const receipt = await frontendPort.testProvider(settings, (status, message) => {
      commandStates.value[key] = { ...commandStates.value[key], status, message }
    })
    commandStates.value[key] = { status: receipt.finalStatus, message: receipt.message, commandId: receipt.commandId }
    return receipt
  }

  async function loadMarket() {
    if (market.value.length || marketLoading.value) return
    marketLoading.value = true
    try {
      market.value = await frontendPort.queryMarketOpportunities()
    } finally {
      marketLoading.value = false
    }
  }

  async function runCommand(key: string, request: CommandRequest) {
    commandStates.value[key] = { status: 'submitting', message: '正在提交…' }
    const commandId = request.commandId ?? `CMD-${Date.now()}-${Math.floor(Math.random() * 1000)}`
    const envelope: CommandRequest = {
      ...request,
      commandId,
      idempotencyKey: request.idempotencyKey ?? commandId,
      requestedAt: request.requestedAt ?? new Date().toISOString()
    }
    const receipt = await frontendPort.executeCommand(envelope, (status, message) => {
      commandStates.value[key] = { ...commandStates.value[key], status, message }
    })
    commandStates.value[key] = { status: receipt.finalStatus, message: receipt.message, commandId: receipt.commandId }
    // The project slot this receipt targets: for a duplicate the result is a
    // brand-new project (resultProjectId) added to the library, never an implicit
    // replacement of the active project unless the caller explicitly switches.
    // Refreshing the library projection is safe regardless — it is idempotent and
    // re-reads the catalog — so a late receipt only re-syncs the list, never the
    // active project store.
    if (receipt.finalStatus === 'committed') {
      void refresh()
    }
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
