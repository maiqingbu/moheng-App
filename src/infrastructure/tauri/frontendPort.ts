import type {
  BookHealthProjection,
  HealthSourcePage,
  CanonProjection,
  ChapterListItem,
  ChapterProjection,
  ExportCapabilityProjection,
  ExportQueryProjection,
  ExportReceiptProjection,
  CommandReceipt,
  DecisionItem,
  CommandRequest,
  CommandStatus,
  FrontendPort,
  HistoryProjection,
  MarketOpportunity,
  MarketDiscoveryProjection,
  MarketStrategyProjection,
  PageResult,
  ProductionCenterProjection,
  ProjectLibraryProjection,
  ProjectOverviewProjection,
  ProjectQueryRequest,
  DependencySummaryBootstrapProjection,
  RetconContextProjection,
  NarrativeContextProjection,
  NarrativeRuntimeQueryRequest,
  NarrativeStateProjection,
  ContinuityProjection,
  PlanStaleProjection,
  PlanningRouteProjection,
  SupportSessionProjection,
  SupportCheckpointProjection,
  SearchHit,
  SettingsProjection,
  ProviderSettingsInput,
  AssistantRepairPlan,
  OpeningReadyProjection,
  StoryDirectionProjection,
  StorySetupProjection,
  VersionProjection,
  VolumeDetailProjection,
  VolumeProgressProjection,
  VolumeSummary,
  VolumeLifecycleProjection,
  CompletionStatusProjection,
  EndgameMappingWorkflowProjection,
  SoftCreativeAdviceProjection,
} from '../../application/contracts'

export interface TauriInvoke {
  invoke<T>(command: string, payload?: unknown): Promise<T>
}

const invokeQuery = <T>(ipc: TauriInvoke, command: string, args?: unknown): Promise<T> =>
  ipc.invoke<T>(command, args).catch(error => {
    const message = error instanceof Error ? error.message : String(error)
    if (/^command\s+[^\r\n]+\s+not found$/i.test(message.trim())) {
      throw new Error(`IPC_UNSUPPORTED:${command}`)
    }
    throw error
  })

export class TauriFrontendPort implements FrontendPort {
  constructor(private readonly ipc: TauriInvoke) {}
  queryProjectLibrary(): Promise<ProjectLibraryProjection> { return invokeQuery(this.ipc, 'query_project_library') }
  queryProjectOverview(request: ProjectQueryRequest): Promise<ProjectOverviewProjection> { return invokeQuery(this.ipc, 'query_project_overview', { request }) }
  queryCommandReceipt(request: { projectId: string; idempotencyKey: string }) { return invokeQuery<CommandReceipt | null>(this.ipc, 'query_command_receipt', { request }) }
  executeCommand(request: CommandRequest, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    onStatus?.('submitting', '正在提交正式事务')
    return invokeQuery<CommandReceipt>(this.ipc, 'execute_command', { request }).then(receipt => {
      onStatus?.(receipt.finalStatus, receipt.message)
      return receipt
    })
  }
  queryGlobalSettings(): Promise<SettingsProjection> { return invokeQuery(this.ipc, 'query_global_settings') }
  queryNarrativeState(request: NarrativeRuntimeQueryRequest): Promise<NarrativeStateProjection> { return invokeQuery(this.ipc, 'query_narrative_state', { request }) }
  queryContinuity(request: NarrativeRuntimeQueryRequest): Promise<ContinuityProjection> { return invokeQuery(this.ipc, 'query_continuity', { request }) }
  queryVolumeProgress(request: NarrativeRuntimeQueryRequest & { volumeId: string; taskIds?: string[] }): Promise<VolumeProgressProjection> { return invokeQuery(this.ipc, 'query_volume_progress', { request }) }
  queryContextManifest(request: NarrativeRuntimeQueryRequest & { executionMode?: 'OPENING_BOOTSTRAP' | 'LONG_FORM' }): Promise<NarrativeContextProjection> { return invokeQuery(this.ipc, 'query_context_manifest', { request }) }
  queryPlanStale(request: NarrativeRuntimeQueryRequest & { compiledFromProjectRevision: number; compiledFromStateDigest: string; currentStateDigest?: string }): Promise<PlanStaleProjection> { return invokeQuery(this.ipc, 'query_plan_stale', { request }) }
  queryPlanRoute(request: NarrativeRuntimeQueryRequest & { volumeId: string; currentChapter?: number }): Promise<PlanningRouteProjection> { return invokeQuery(this.ipc, 'query_plan_route', { request }) }
  queryVolumeLifecycle(request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeLifecycleProjection> { return invokeQuery(this.ipc, 'query_volume_lifecycle', { request }) }
  queryEndgameStatus(request: ProjectQueryRequest): Promise<CompletionStatusProjection> { return invokeQuery(this.ipc, 'query_endgame_status', { request }) }
  queryCompletionStatus(request: ProjectQueryRequest): Promise<CompletionStatusProjection> { return invokeQuery(this.ipc, 'query_completion_status', { request }) }
  queryEndgameMappingOptions(request: ProjectQueryRequest): Promise<EndgameMappingWorkflowProjection> { return invokeQuery(this.ipc, 'query_endgame_mapping_options', { request }) }
  queryEndgameAdvice(request: ProjectQueryRequest): Promise<SoftCreativeAdviceProjection> { return invokeQuery(this.ipc, 'query_endgame_advice', { request }) }
  queryCompletionQuality(request: ProjectQueryRequest): Promise<SoftCreativeAdviceProjection> { return invokeQuery(this.ipc, 'query_completion_quality', { request }) }
  supportSession(request: { projectId: string; question: string; mode?: 'INSPECT' | 'DIAGNOSE'; scope?: string } & NarrativeRuntimeQueryRequest): Promise<SupportSessionProjection> { return invokeQuery(this.ipc, 'support_session', { request }) }
  supportCheckpointResume(request: { projectId: string; checkpointId: string }): Promise<SupportCheckpointProjection> { return invokeQuery(this.ipc, 'support_checkpoint_resume', { request }) }
  testProvider(settings: ProviderSettingsInput, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    onStatus?.('submitting', settings.apiKey ? '正在测试 Provider…' : '正在等待系统钥匙串授权')
    return invokeQuery<CommandReceipt>(this.ipc, 'test_provider', { settings }).then(receipt => {
      onStatus?.(receipt.finalStatus, receipt.message)
      return receipt
    })
  }
  queryMarketStrategy(request: ProjectQueryRequest) { return invokeQuery<MarketStrategyProjection>(this.ipc, 'query_market_strategy', { request }) }
  queryStoryDirections(request: ProjectQueryRequest) { return invokeQuery<StoryDirectionProjection>(this.ipc, 'query_story_directions', { request }) }
  queryStorySetup(request: ProjectQueryRequest) { return invokeQuery<StorySetupProjection>(this.ipc, 'query_story_setup', { request }) }
  queryOpeningReady(request: ProjectQueryRequest) { return invokeQuery<OpeningReadyProjection>(this.ipc, 'query_opening_ready', { request }) }
  queryAssistantRepairPlan(request: ProjectQueryRequest & { question: string }) { return invokeQuery<AssistantRepairPlan>(this.ipc, 'query_assistant_repair_plan', { request }) }
  queryProductionCenter(request: ProjectQueryRequest) { return invokeQuery<ProductionCenterProjection>(this.ipc, 'query_production_center', { request }) }
  queryVolumeList(request: ProjectQueryRequest) { return invokeQuery<PageResult<VolumeSummary>>(this.ipc, 'query_volume_list', { request }) }
  queryVolumeDetail(request: Parameters<FrontendPort['queryVolumeDetail']>[0]) { return invokeQuery<VolumeDetailProjection>(this.ipc, 'query_volume_detail', { request }) }
  queryDecisions(request: Parameters<FrontendPort['queryDecisions']>[0]) { return invokeQuery<PageResult<DecisionItem>>(this.ipc, 'query_decisions', { request }) }
  queryReaderVolumes(request: ProjectQueryRequest) { return invokeQuery<PageResult<VolumeSummary>>(this.ipc, 'query_reader_volumes', { request }) }
  queryChapterList(request: Parameters<FrontendPort['queryChapterList']>[0]) { return invokeQuery<PageResult<ChapterListItem>>(this.ipc, 'query_chapter_list', { request }) }
  queryChapterFinal(request: Parameters<FrontendPort['queryChapterFinal']>[0]) { return invokeQuery<ChapterProjection>(this.ipc, 'query_chapter_final', { request }) }
  queryChapterVersions(request: Parameters<FrontendPort['queryChapterVersions']>[0]) { return invokeQuery<PageResult<VersionProjection>>(this.ipc, 'query_chapter_versions', { request }) }
  queryExportCapability(request: ProjectQueryRequest) { return invokeQuery<ExportCapabilityProjection>(this.ipc, 'query_export_capability', { request }) }
  createExport(request: { projectId: string; requestId: string; format: 'TXT' }) { return invokeQuery<ExportReceiptProjection>(this.ipc, 'create_export', { request }) }
  queryExportReceipt(request: { projectId: string; requestId: string }) { return invokeQuery<ExportQueryProjection>(this.ipc, 'query_export_receipt', { request }) }
  queryLatestExportReceipt(request: ProjectQueryRequest) { return invokeQuery<ExportQueryProjection>(this.ipc, 'query_latest_export_receipt', { request }) }
  queryCanon(request: ProjectQueryRequest) { return invokeQuery<CanonProjection>(this.ipc, 'query_canon', { request }) }
  queryRetconContext(request: ProjectQueryRequest & { query?: string }) { return invokeQuery<RetconContextProjection>(this.ipc, 'query_retcon_context', { request }) }
  queryDependencySummaryBootstrap(request: ProjectQueryRequest) { return invokeQuery<DependencySummaryBootstrapProjection>(this.ipc, 'query_dependency_summary_bootstrap', { request }) }
  queryBookHealth(request: ProjectQueryRequest) { return invokeQuery<BookHealthProjection>(this.ipc, 'query_book_health', { request }) }
  queryBookHealthFactorSources(request: ProjectQueryRequest & { factorCode: string }) { return invokeQuery<HealthSourcePage>(this.ipc, 'query_book_health_factor_sources', { request }) }
  queryHistory(request: Parameters<FrontendPort['queryHistory']>[0]) { return invokeQuery<HistoryProjection>(this.ipc, 'query_history', { request }) }
  queryChangeRequest(request: Parameters<FrontendPort['queryChangeRequest']>[0]) { return invokeQuery<Record<string, unknown>>(this.ipc, 'query_change_request', { request }) }
  queryProjectSettings(request: ProjectQueryRequest) { return invokeQuery<SettingsProjection>(this.ipc, 'query_project_settings', { request }) }
  searchFulltext(request: Parameters<FrontendPort['searchFulltext']>[0]) { return invokeQuery<PageResult<SearchHit>>(this.ipc, 'search_fulltext', { request }) }
  queryMarketOpportunities() { return invokeQuery<MarketDiscoveryProjection>(this.ipc, 'query_market_opportunities') }
}

export function getTauriInvoke(): TauriInvoke | null {
  const candidate = (globalThis as { __TAURI__?: { core?: TauriInvoke } }).__TAURI__?.core
  return candidate ?? null
}
