import type {
  BookHealthProjection,
  CanonProjection,
  ChapterListItem,
  ChapterProjection,
  CommandReceipt,
  DecisionItem,
  CommandRequest,
  CommandStatus,
  FrontendPort,
  HistoryProjection,
  MarketOpportunity,
  MarketStrategyProjection,
  PageResult,
  ProductionCenterProjection,
  ProjectLibraryProjection,
  ProjectOverviewProjection,
  ProjectQueryRequest,
  SearchHit,
  SettingsProjection,
  StoryDirectionProjection,
  VersionProjection,
  VolumeDetailProjection,
  VolumeSummary,
} from '../../application/contracts'

export interface TauriInvoke {
  invoke<T>(command: string, payload?: unknown): Promise<T>
}

const invokeQuery = <T>(ipc: TauriInvoke, command: string, args?: unknown): Promise<T> =>
  ipc.invoke<T>(command, args).catch(error => {
    const message = error instanceof Error ? error.message : String(error)
    if (/unknown command|not found|unsupported/i.test(message)) {
      throw new Error(`IPC_UNSUPPORTED:${command}`)
    }
    throw error
  })

export class TauriFrontendPort implements FrontendPort {
  constructor(private readonly ipc: TauriInvoke) {}
  queryProjectLibrary(): Promise<ProjectLibraryProjection> { return invokeQuery(this.ipc, 'query_project_library') }
  queryProjectOverview(request: ProjectQueryRequest): Promise<ProjectOverviewProjection> { return invokeQuery(this.ipc, 'query_project_overview', { request }) }
  executeCommand(request: CommandRequest, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    onStatus?.('submitting', '正在提交正式事务')
    return invokeQuery<CommandReceipt>(this.ipc, 'execute_command', { request }).then(receipt => {
      onStatus?.(receipt.finalStatus, receipt.message)
      return receipt
    })
  }
  queryGlobalSettings(): Promise<SettingsProjection> { return invokeQuery(this.ipc, 'query_global_settings') }
  testProvider(settings: SettingsProjection, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    onStatus?.('submitting', '正在测试 Provider')
    return invokeQuery<CommandReceipt>(this.ipc, 'test_provider', { settings }).then(receipt => {
      onStatus?.(receipt.finalStatus, receipt.message)
      return receipt
    })
  }
  queryMarketStrategy(request: ProjectQueryRequest) { return invokeQuery<MarketStrategyProjection>(this.ipc, 'query_market_strategy', { request }) }
  queryStoryDirections(request: ProjectQueryRequest) { return invokeQuery<StoryDirectionProjection>(this.ipc, 'query_story_directions', { request }) }
  queryProductionCenter(request: ProjectQueryRequest) { return invokeQuery<ProductionCenterProjection>(this.ipc, 'query_production_center', { request }) }
  queryVolumeList(request: ProjectQueryRequest) { return invokeQuery<PageResult<VolumeSummary>>(this.ipc, 'query_volume_list', { request }) }
  queryVolumeDetail(request: Parameters<FrontendPort['queryVolumeDetail']>[0]) { return invokeQuery<VolumeDetailProjection>(this.ipc, 'query_volume_detail', { request }) }
  queryDecisions(request: Parameters<FrontendPort['queryDecisions']>[0]) { return invokeQuery<PageResult<DecisionItem>>(this.ipc, 'query_decisions', { request }) }
  queryReaderVolumes(request: ProjectQueryRequest) { return invokeQuery<PageResult<VolumeSummary>>(this.ipc, 'query_reader_volumes', { request }) }
  queryChapterList(request: Parameters<FrontendPort['queryChapterList']>[0]) { return invokeQuery<PageResult<ChapterListItem>>(this.ipc, 'query_chapter_list', { request }) }
  queryChapterFinal(request: Parameters<FrontendPort['queryChapterFinal']>[0]) { return invokeQuery<ChapterProjection>(this.ipc, 'query_chapter_final', { request }) }
  queryChapterVersions(request: Parameters<FrontendPort['queryChapterVersions']>[0]) { return invokeQuery<PageResult<VersionProjection>>(this.ipc, 'query_chapter_versions', { request }) }
  queryCanon(request: ProjectQueryRequest) { return invokeQuery<CanonProjection>(this.ipc, 'query_canon', { request }) }
  queryBookHealth(request: ProjectQueryRequest) { return invokeQuery<BookHealthProjection>(this.ipc, 'query_book_health', { request }) }
  queryHistory(request: Parameters<FrontendPort['queryHistory']>[0]) { return invokeQuery<HistoryProjection>(this.ipc, 'query_history', { request }) }
  queryProjectSettings(request: ProjectQueryRequest) { return invokeQuery<SettingsProjection>(this.ipc, 'query_project_settings', { request }) }
  searchFulltext(request: Parameters<FrontendPort['searchFulltext']>[0]) { return invokeQuery<PageResult<SearchHit>>(this.ipc, 'search_fulltext', { request }) }
  queryMarketOpportunities() { return invokeQuery<MarketOpportunity[]>(this.ipc, 'query_market_opportunities') }
}

export function getTauriInvoke(): TauriInvoke | null {
  const candidate = (globalThis as { __TAURI__?: { core?: TauriInvoke } }).__TAURI__?.core
  return candidate ?? null
}
