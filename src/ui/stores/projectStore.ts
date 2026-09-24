import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type {
  BookHealthProjection,
  CanonProjection,
  ChapterListItem,
  ChapterProjection,
  CommandReceipt,
  CommandRequest,
  CommandStatus,
  FrontendPort,
  DecisionItem,
  HistoryProjection,
  AssistantRepairPlan,
  MarketStrategyProjection,
  OpeningReadyProjection,
  PageResult,
  PlanningRouteProjection,
  ProductionCenterProjection,
  ProjectOverviewProjection,
  SearchHit,
  SettingsProjection,
  ProviderSettingsInput,
  DependencySummaryBootstrapProjection,
  RetconContextProjection,
  StorySetupProjection,
  SupportSessionProjection,
  StoryDirectionProjection,
  VersionProjection,
  VolumeDetailProjection,
  VolumeSummary,
  VolumeLifecycleProjection
} from '../../application/contracts'
import { getFrontendPort } from '../../app/providers/runtimePort'
import { receiptSummary } from '../presentation/productLanguage'
import { useProjectLibraryStore } from './projectLibraryStore'

const frontendPort = getFrontendPort()

export interface CommandUiState {
  status: CommandStatus
  message: string
  commandId?: string
  confirmation?: 'pending' | 'confirmed'
  receiptId?: string
}

export interface CommandBinding {
  commandId: string
  sourceProjectId: string | null
  generation: number
}

const emptyPage = <T>(): PageResult<T> => ({ items: [], nextCursor: null, total: 0 })
type HistoryKind = 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON'

export const useProjectStore = defineStore('project-projection', () => {
  const currentProjectId = ref<string | null>(null)
  const overview = ref<ProjectOverviewProjection | null>(null)
  const marketStrategy = ref<MarketStrategyProjection | null>(null)
  const directions = ref<StoryDirectionProjection | null>(null)
  const storySetup = ref<StorySetupProjection | null>(null)
  const openingReady = ref<OpeningReadyProjection | null>(null)
  const assistantSession = ref<SupportSessionProjection | null>(null)
  const production = ref<ProductionCenterProjection | null>(null)
  const volumes = ref<PageResult<VolumeSummary> | null>(null)
  const volumeDetail = ref<VolumeDetailProjection | null>(null)
  const planningRoute = ref<PlanningRouteProjection | null>(null)
  const volumeLifecycle = ref<VolumeLifecycleProjection | null>(null)
  const decisions = ref<PageResult<DecisionItem> | null>(null)
  const readerVolumes = ref<PageResult<VolumeSummary> | null>(null)
  const chapterLists = ref<Record<string, PageResult<ChapterListItem>>>({})
  const activeChapter = ref<ChapterProjection | null>(null)
  const chapterVersions = ref<PageResult<VersionProjection>>(emptyPage())
  const canon = ref<CanonProjection | null>(null)
  const health = ref<BookHealthProjection | null>(null)
  const healthError = ref('')
  const history = ref<HistoryProjection | null>(null)
  const historyKind = ref<HistoryKind | null>(null)
  const settings = ref<SettingsProjection | null>(null)
  const searchResults = ref<PageResult<SearchHit>>(emptyPage())
  const loading = ref(false)
  const workspaceLoading = ref<Record<string, boolean>>({})
  const loadError = ref('')
  const commandStates = ref<Record<string, CommandUiState>>({})
  const advancedMode = ref(false)
  let loadSequence = 0
  const pendingConfirmations = new Map<string, { projectId: string | null; receipt: CommandReceipt }>()
  const workspaceSequences = new Map<string, number>()
  const inFlightCommands = new Map<string, Promise<CommandReceipt>>()

  const project = computed(() => overview.value?.project ?? null)
  const pendingDecisionCount = computed(() => overview.value?.pendingDecisionCount ?? decisions.value?.items.filter(item => item.status === 'pending').length ?? 0)
  const pendingDecisions = computed(() => decisions.value?.items.filter(item => item.status === 'pending') ?? [])
  const recentOfficialChapters = computed(() => overview.value?.recentChapters ?? [])

  function queryBase(scope: string, pageSize = 50) {
    if (!currentProjectId.value) throw new Error('当前没有打开的作品。')
    return { projectId: currentProjectId.value, scope, cursor: null, pageSize }
  }

  function resetWorkspaces() {
    marketStrategy.value = null
    directions.value = null
    storySetup.value = null
    openingReady.value = null
    assistantSession.value = null
    production.value = null
    volumes.value = null
    volumeDetail.value = null
    planningRoute.value = null
    volumeLifecycle.value = null
    decisions.value = null
    readerVolumes.value = null
    chapterLists.value = {}
    activeChapter.value = null
    chapterVersions.value = emptyPage()
    canon.value = null
    health.value = null
    healthError.value = ''
    history.value = null
    historyKind.value = null
    settings.value = null
    searchResults.value = emptyPage()
  }

  async function bootstrap(projectId?: string) {
    if (!projectId) return
    if (currentProjectId.value === projectId && overview.value) return
    await openProject(projectId)
  }

  async function openProject(projectId: string) {
    if (!projectId) return
    const requestSequence = ++loadSequence
    const switching = currentProjectId.value !== projectId
    if (switching) {
      overview.value = null
      resetWorkspaces()
      commandStates.value = {}
    }
    currentProjectId.value = projectId
    loading.value = true
    loadError.value = ''
    try {
      const next = await frontendPort.queryProjectOverview({ projectId, scope:'overview', cursor:null, pageSize:8 })
      if (requestSequence !== loadSequence || currentProjectId.value !== projectId) return
      overview.value = next
      void useProjectLibraryStore().refresh()
    } catch (error) {
      if (requestSequence !== loadSequence || currentProjectId.value !== projectId) return
      overview.value = null
      loadError.value = error instanceof Error ? error.message : '无法读取项目概览。'
    } finally {
      if (requestSequence === loadSequence) loading.value = false
    }
  }

  async function refresh() {
    if (currentProjectId.value) await openProject(currentProjectId.value)
  }

  function clearProject() {
    loadSequence += 1
    currentProjectId.value = null
    overview.value = null
    resetWorkspaces()
    loadError.value = ''
  }

  async function withWorkspace<T>(key: string, loader: () => Promise<T>, assign: (value: T) => void) {
    // Only the latest request for this workspace may replace its projection.
    const sequence = (workspaceSequences.get(key) ?? 0) + 1
    workspaceSequences.set(key, sequence)
    const projectId = currentProjectId.value
    workspaceLoading.value[key] = true
    try {
      const value = await loader()
      // A route transition can start the workspace query while AppShell is
      // still completing a duplicate openProject() for the same project. The
      // project-id binding is the authority for clobber protection; a newer
      // generation for that same project must not discard the already valid
      // projection and leave the page blank.
      if (projectId !== currentProjectId.value || workspaceSequences.get(key) !== sequence) return value
      assign(value)
      return value
    } finally {
      if (workspaceSequences.get(key) === sequence) workspaceLoading.value[key] = false
    }
  }

  async function loadMarket(force=false) {
    if (marketStrategy.value && !force) return marketStrategy.value
    return withWorkspace('market', () => frontendPort.queryMarketStrategy(queryBase('market', 20)), v => { marketStrategy.value=v })
  }
  async function loadDirections(force=false) {
    if (directions.value && !force) return directions.value
    return withWorkspace('directions', () => frontendPort.queryStoryDirections(queryBase('directions', 10)), v => { directions.value=v })
  }
  async function loadStorySetup(force=false) {
    if (storySetup.value && !force) return storySetup.value
    return withWorkspace('story-setup', () => frontendPort.queryStorySetup(queryBase('story-setup', 1)), v => { storySetup.value = v })
  }
  async function loadOpeningReady(force=false) {
    if (openingReady.value && !force) return openingReady.value
    return withWorkspace('opening-ready', () => frontendPort.queryOpeningReady(queryBase('opening-ready', 1)), v => { openingReady.value = v })
  }
  async function loadProduction(force=false) {
    if (production.value && !force) return production.value
    return withWorkspace('production', () => frontendPort.queryProductionCenter(queryBase('production', 20)), v => { production.value=v })
  }
  async function loadVolumes(force=false) {
    if (volumes.value && !force) return volumes.value
    return withWorkspace('volumes', () => frontendPort.queryVolumeList(queryBase('volumes', 20)), v => { volumes.value=v })
  }
  async function loadVolumeDetail(volumeId:string, force=false) {
    if (volumeDetail.value?.volume.id === volumeId && !force) return volumeDetail.value
    return withWorkspace('volume-detail', () => frontendPort.queryVolumeDetail({ ...queryBase(`volume:${volumeId}`, 30), volumeId }), v => { volumeDetail.value=v })
  }
  async function loadPlanningRoute(volumeId:string, force=false, currentChapter?: number) {
    if (planningRoute.value?.volumeId === volumeId && !force) return planningRoute.value
    return withWorkspace('planning-route', () => frontendPort.queryPlanRoute({ ...queryBase(`plan-route:${volumeId}`, 30), volumeId, ...(currentChapter == null ? {} : { currentChapter }) }), route => { planningRoute.value = route })
  }
  async function loadVolumeLifecycle(volumeId:string, force=false) {
    if (volumeLifecycle.value?.volumeId === volumeId && !force) return volumeLifecycle.value
    return withWorkspace('volume-lifecycle', () => frontendPort.queryVolumeLifecycle({ ...queryBase(`volume-lifecycle:${volumeId}`, 30), volumeId }), value => { volumeLifecycle.value = value })
  }
  async function loadDecisions(status:'pending'|'resolved'|'all'='all', force=false) {
    if (decisions.value && !force && status==='all') return decisions.value
    return withWorkspace('decisions', () => frontendPort.queryDecisions({ ...queryBase('decisions', 50), status }), v => { decisions.value=v })
  }
  async function loadReaderVolumes(force=false) {
    if (readerVolumes.value && !force) return readerVolumes.value
    return withWorkspace('reader-volumes', () => frontendPort.queryReaderVolumes(queryBase('reader:volumes', 30)), v => { readerVolumes.value=v })
  }
  async function loadChapterList(volumeId:string, force=false) {
    if (chapterLists.value[volumeId] && !force) return chapterLists.value[volumeId]
    return withWorkspace(`chapters:${volumeId}`, () => frontendPort.queryChapterList({ ...queryBase(`reader:volume:${volumeId}`, 40), volumeId }), v => { chapterLists.value={...chapterLists.value,[volumeId]:v} })
  }
  async function loadChapter(chapterId:string) {
    return withWorkspace('chapter', () => frontendPort.queryChapterFinal({ ...queryBase(`reader:chapter:${chapterId}`, 1), chapterId }), v => { activeChapter.value=v })
  }
  async function loadChapterVersions(chapterId:string) {
    return withWorkspace('chapter-versions', () => frontendPort.queryChapterVersions({ ...queryBase(`history:chapter:${chapterId}`, 30), chapterId }), v => { chapterVersions.value=v })
  }
  async function loadCanon(force=false) {
    if (canon.value && !force) return canon.value
    return withWorkspace('graph', () => frontendPort.queryCanon(queryBase('graph:overview', 60)), v => { canon.value=v })
  }
  async function loadHealth(force=false, cursor: string | null = null) {
    if (health.value && !force && cursor === null) return health.value
    healthError.value = ''
    try {
      return await withWorkspace('health', () => frontendPort.queryBookHealth({ ...queryBase('health', 30), cursor }), v => { health.value=v })
    } catch (error) {
      healthError.value = error instanceof Error ? error.message : '暂时无法完成这一项检查。'
      throw error
    }
  }
  async function loadHistory(kind:HistoryKind='all', force=false, cursor: string | null = null) {
    if (history.value && historyKind.value === kind && !force && cursor === null) return history.value
    const pageCursor = cursor && historyKind.value === kind ? cursor : null
    return withWorkspace('history', async () => {
      const page = await frontendPort.queryHistory({ ...queryBase('history', 40), cursor: pageCursor, kind })
      if (!pageCursor || !history.value || historyKind.value !== kind) return page
      const existing = new Set(history.value.entries.items.map(entry => `${entry.kind}:${entry.id}`))
      const appended = page.entries.items.filter(entry => !existing.has(`${entry.kind}:${entry.id}`))
      return {
        ...page,
        entries: { ...page.entries, items: [...history.value.entries.items, ...appended] },
      }
    }, v => { history.value=v; historyKind.value=kind })
  }
  async function queryChangeRequest(changeRequestId: string) {
    return frontendPort.queryChangeRequest({ ...queryBase(`history:change:${changeRequestId}`, 40), changeRequestId })
  }
  async function queryRetconContext(query?: string): Promise<RetconContextProjection> {
    return frontendPort.queryRetconContext({ ...queryBase('history:retcon-context', 30), query })
  }
  async function queryDependencySummaryBootstrap(): Promise<DependencySummaryBootstrapProjection> {
    return frontendPort.queryDependencySummaryBootstrap(queryBase('history:dependency-summary', 1))
  }
  async function loadSettings(force=false) {
    if (settings.value && !force) return settings.value
    return withWorkspace('settings', () => frontendPort.queryProjectSettings(queryBase('settings', 1)), v => { settings.value=v })
  }
  async function search(text:string) {
    return withWorkspace('search', () => frontendPort.searchFulltext({ ...queryBase('search', 30), text }), v => { searchResults.value=v })
  }

  async function supportSession(question: string, mode: 'INSPECT'|'DIAGNOSE' = 'DIAGNOSE') {
    const request = { ...queryBase('assistant', 12), question, mode, requestedAt: new Date().toISOString() } as Parameters<FrontendPort['supportSession']>[0]
    return withWorkspace('assistant', () => frontendPort.supportSession(request), value => { assistantSession.value = value })
  }

  async function queryAssistantRepairPlan(question: string): Promise<AssistantRepairPlan> {
    const request = { ...queryBase('assistant-repair', 1), question, requestedAt: new Date().toISOString() }
    return frontendPort.queryAssistantRepairPlan(request)
  }

  async function prepareAssistantRepair(question: string): Promise<AssistantRepairPlan> {
    commandStates.value['assistant-plan'] = { status: 'submitting', message: '正在核对修复范围…' }
    try {
      const plan = await queryAssistantRepairPlan(question)
      if (plan.action !== 'EXECUTABLE') {
        commandStates.value['assistant-plan'] = {
          status: 'committed',
          message: plan.action === 'PROPOSAL_ONLY' ? '这项请求只能先建立修改提案。' : '当前记录不需要执行修复。',
        }
        return plan
      }
      const receipt = await runCommand('assistant-plan', {
        type: 'repair_plan_create',
        projectId: currentProjectId.value ?? undefined,
        expectedRevision: plan.basedOnProjectRevision,
        payload: { dryRun: true, repairPlan: plan },
      })
      if (receipt.finalStatus !== 'committed') throw new Error(receipt.message || '修复方案未保存。')
      return plan
    } catch (error) {
      commandStates.value['assistant-plan'] = {
        status: 'failed',
        message: error instanceof Error ? error.message : '无法核对修复范围。',
      }
      throw error
    }
  }

  async function authorizeAssistantRepair(plan: AssistantRepairPlan): Promise<CommandReceipt> {
    if (plan.action !== 'EXECUTABLE') throw new Error('当前方案不可执行。')
    const projectId = currentProjectId.value
    if (!projectId) throw new Error('当前没有打开的作品。')
    const authorizationId = `assistant-authorization:${plan.repairPlanId}`
    const authorization = await runCommand('assistant-authorize', {
      type: 'repair_authorize',
      projectId,
      expectedRevision: plan.basedOnProjectRevision,
      payload: {
        authorization: {
          authorizationId,
          projectId,
          repairPlanId: plan.repairPlanId,
          basedOnProjectRevision: plan.basedOnProjectRevision,
          operationDigest: plan.operationDigest,
          approvedByUser: true,
          issuedAt: new Date().toISOString(),
          consumedAt: null,
        },
      },
    })
    if (authorization.finalStatus !== 'committed') throw new Error(authorization.message || '授权未保存。')
    return runCommand('assistant-execute', {
      type: 'repair_execute',
      projectId,
      payload: { repairPlanId: plan.repairPlanId, authorizationId },
    })
  }

  async function refreshLoadedWorkspaces() {
    const jobs: Promise<unknown>[] = []
    if (marketStrategy.value) jobs.push(loadMarket(true))
    if (directions.value) jobs.push(loadDirections(true))
    if (storySetup.value) jobs.push(loadStorySetup(true))
    if (openingReady.value) jobs.push(loadOpeningReady(true))
    if (production.value) jobs.push(loadProduction(true))
    if (volumes.value) jobs.push(loadVolumes(true))
    if (volumeDetail.value) jobs.push(loadVolumeDetail(volumeDetail.value.volume.id, true))
    if (planningRoute.value) jobs.push(loadPlanningRoute(planningRoute.value.volumeId, true))
    if (volumeLifecycle.value) jobs.push(loadVolumeLifecycle(volumeLifecycle.value.volumeId, true))
    if (decisions.value) jobs.push(loadDecisions('all', true))
    if (readerVolumes.value) jobs.push(loadReaderVolumes(true))
    if (canon.value) jobs.push(loadCanon(true))
    if (health.value) jobs.push(loadHealth(true))
    if (history.value) jobs.push(loadHistory('all', true))
    if (settings.value) jobs.push(loadSettings(true))
    const results = await Promise.allSettled(jobs)
    const failure = results.find((result): result is PromiseRejectedResult => result.status === 'rejected')
    if (failure) throw failure.reason
  }

  async function confirmCommandResult(key: string) {
    const pending = pendingConfirmations.get(key)
    if (!pending || pending.projectId !== currentProjectId.value) return
    const isCurrent = () => pendingConfirmations.get(key) === pending && pending.projectId === currentProjectId.value
    try {
      await refresh()
      if (!isCurrent()) return
      if (loadError.value) throw new Error(loadError.value)
      await refreshLoadedWorkspaces()
      if (!isCurrent()) return
      commandStates.value[key] = {
        status: pending.receipt.finalStatus, commandId: pending.receipt.commandId,
        receiptId: pending.receipt.receiptId, confirmation: 'confirmed',
        message: '正式回执已返回，当前作品展示已重新读取。',
      }
      pendingConfirmations.delete(key)
    } catch {
      if (!isCurrent()) return
      commandStates.value[key] = {
        status: pending.receipt.finalStatus, commandId: pending.receipt.commandId,
        receiptId: pending.receipt.receiptId, confirmation: 'pending',
        message: '正式回执已返回，但重新查询未完成，结果待确认。请只重新读取，不要重复提交。',
      }
    }
  }

  async function executeCommand(key: string, request: CommandRequest) {
    const commandId = request.commandId ?? `CMD-${Date.now()}-${Math.floor(Math.random() * 1000)}`
    const binding: CommandBinding = {
      commandId,
      sourceProjectId: request.projectId ?? currentProjectId.value,
      generation: loadSequence,
    }
    const bindingRevision = project.value?.revision
    const projectScoped = binding.sourceProjectId !== null && !['project_create_from_idea', 'project_create_from_market', 'global_settings_save'].includes(request.type)
    const expectedRevision = request.expectedRevision ?? bindingRevision
    const isCurrentBinding = () => binding.generation === loadSequence && binding.sourceProjectId === currentProjectId.value
    const setStateIfCurrent = (state: CommandUiState) => {
      if (!isCurrentBinding()) return
      const previous = commandStates.value[key]
      if (previous?.commandId && previous.commandId !== binding.commandId) return
      commandStates.value[key] = state
    }

    commandStates.value[key] = { status: 'submitting', message: '正在提交…', commandId }
    if (projectScoped && expectedRevision == null) {
      const error = new Error('PROTOCOL / PROJECT_REVISION_UNAVAILABLE')
      commandStates.value[key] = { status: 'failed', message: error.message, commandId }
      throw error
    }
    const envelope: CommandRequest = {
      ...request,
      commandId,
      idempotencyKey: request.idempotencyKey ?? commandId,
      projectId: request.projectId ?? binding.sourceProjectId ?? project.value?.id,
      expectedRevision,
      requestedAt: request.requestedAt ?? new Date().toISOString()
    }
    let receipt: CommandReceipt
    try {
      receipt = await frontendPort.executeCommand(envelope, (status, message) => {
        setStateIfCurrent({ ...commandStates.value[key], commandId, status: status === 'committed' ? 'processing' : status, message: status === 'committed' ? '回执已返回，正在读取当前作品状态。' : message })
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      if (request.type === 'retcon_apply' || request.type === 'change_request_apply') {
        const message = request.type === 'retcon_apply'
          ? '事实修订提交结果暂时无法确认。请先重新读取；若重试，将使用同一操作编号。'
          : '普通修改结果待确认，提交状态未知。请先重新读取；若重试，将使用同一操作编号。'
        setStateIfCurrent({ status: 'processing', confirmation: 'pending', message, commandId })
      } else {
        setStateIfCurrent({ status: 'failed', message, commandId })
      }
      if (request.type.startsWith('production_')) {
        try { await loadProduction(true) } catch { /* preserve the original command error */ }
      }
      throw error
    }

    if (receipt.commandId !== binding.commandId || (projectScoped && receipt.projectId !== binding.sourceProjectId)) {
      const error = new Error('PROTOCOL / PROJECT_BINDING_ERROR')
      setStateIfCurrent({ status: 'failed', message: error.message, commandId })
      throw error
    }
    if (!isCurrentBinding()) return receipt

    commandStates.value[key] = { status: receipt.finalStatus, message: receiptSummary(receipt).detail, commandId: receipt.commandId }
    if (receipt.finalStatus === 'committed') {
      pendingConfirmations.set(key, { projectId: binding.sourceProjectId, receipt })
      commandStates.value[key] = { status: 'committed', message: '回执已返回，正在读取当前作品状态。', commandId: receipt.commandId, receiptId: receipt.receiptId, confirmation: 'pending' }
      await confirmCommandResult(key)
    }
    void useProjectLibraryStore().refresh()
    return receipt
  }

  function inFlightCommandKey(request: CommandRequest): string | null {
    if (!['opening_ready', 'opening_start_creating', 'production_start', 'opening_evaluate', 'book_plan_revise', 'planning_create_volume_intent', 'planning_create_volume_plan', 'planning_prepare_next_chapter', 'planning_compile_chapter_plan'].includes(request.type)) return null
    const projectId = request.projectId ?? currentProjectId.value
    if (!projectId) return null
    const chapterId = request.type === 'production_start' ? request.payload?.chapterId : undefined
    const volumeId = ['planning_create_volume_intent', 'planning_create_volume_plan', 'planning_prepare_next_chapter'].includes(request.type) ? request.payload?.volumeId : undefined
    const chapterIdForPlan = request.type === 'planning_compile_chapter_plan' ? request.payload?.chapterId : undefined
    if (request.type === 'production_start' && typeof chapterId !== 'string') return null
    if (['planning_create_volume_intent', 'planning_create_volume_plan', 'planning_prepare_next_chapter'].includes(request.type) && typeof volumeId !== 'string') return null
    if (request.type === 'planning_compile_chapter_plan' && typeof chapterIdForPlan !== 'string') return null
    return [projectId, request.type, chapterId ?? chapterIdForPlan ?? volumeId ?? ''].join('|')
  }

  async function runCommand(key: string, request: CommandRequest) {
    if (pendingConfirmations.get(key)?.projectId === (request.projectId ?? currentProjectId.value)) {
      throw new Error('COMMAND_RESULT_UNCONFIRMED')
    }
    const dedupeKey = inFlightCommandKey(request)
    if (!dedupeKey) return executeCommand(key, request)
    const active = inFlightCommands.get(dedupeKey)
    if (active) {
      const current = commandStates.value[key]
      commandStates.value[key] = {
        status: 'processing',
        message: '当前操作正在进行，请查看实时进度。',
        commandId: current?.commandId,
      }
      return active
    }
    const promise = executeCommand(key, request)
    inFlightCommands.set(dedupeKey, promise)
    try {
      return await promise
    } finally {
      if (inFlightCommands.get(dedupeKey) === promise) inFlightCommands.delete(dedupeKey)
    }
  }

  async function testProvider(key:string, nextSettings:ProviderSettingsInput) {
    commandStates.value[key]={status:'submitting',message:'正在提交诊断…'}
    try {
      const receipt=await frontendPort.testProvider(nextSettings,(status,message)=>{commandStates.value[key]={...commandStates.value[key],status,message}})
      commandStates.value[key]={status:receipt.finalStatus,message:receipt.message,commandId:receipt.commandId}
      return receipt
    } catch (error) {
      const message=error instanceof Error ? error.message : typeof error === 'string' ? error : error && typeof error === 'object' && 'message' in error ? String(error.message) : JSON.stringify(error) || String(error)
      commandStates.value[key]={status:'failed',message:message || 'Provider 测试未完成。'}
      throw error
    }
  }

  function commandState(key:string):CommandUiState { return commandStates.value[key] ?? { status:'idle',message:'' } }
  function clearCommandState(key:string){ if (pendingConfirmations.has(key)) return; commandStates.value[key]={status:'idle',message:''} }
  function isWorkspaceLoading(key:string){ return !!workspaceLoading.value[key] }

  return {
    currentProjectId, overview, marketStrategy, directions, storySetup, openingReady, assistantSession, production, volumes, volumeDetail, planningRoute, decisions,
    readerVolumes, chapterLists, activeChapter, chapterVersions, canon, health, healthError, history, historyKind, settings, searchResults, volumeLifecycle,
    loading, loadError, project, pendingDecisionCount, pendingDecisions, recentOfficialChapters, advancedMode,
    bootstrap, openProject, refresh, clearProject, loadMarket, loadDirections, loadStorySetup, loadOpeningReady, loadProduction, loadVolumes,
    loadVolumeDetail, loadPlanningRoute, loadVolumeLifecycle, loadDecisions, loadReaderVolumes, loadChapterList, loadChapter, loadChapterVersions,
    loadCanon, loadHealth, loadHistory, queryChangeRequest, queryRetconContext, queryDependencySummaryBootstrap, loadSettings, search, supportSession, queryAssistantRepairPlan, prepareAssistantRepair, authorizeAssistantRepair, runCommand, testProvider, commandState,
    clearCommandState, confirmCommandResult, isWorkspaceLoading
  }
})
