import type {
  ArtifactSummary,
  AssistantRepairPlan,
  BookHealthProjection,
  ChapterListItem,
  ChapterProjection,
  CommandReceipt,
  CommandRequest,
  CommandStatus,
  DecisionItem,
  FrontendPort,
  HealthIssue,
  HistoryEntry,
  HistoryProjection,
  MarketStrategyProjection,
  OpeningReadyProjection,
  PageRequest,
  PageResult,
  ProductionCenterProjection,
  ProjectBundle,
  ProjectLibraryItem,
  ProjectLibraryProjection,
  ProjectOverviewProjection,
  ProjectQueryRequest,
  SearchHit,
  SettingsProjection,
  StoryDirectionProjection,
  StorySetupProjection,
  SupportSessionProjection,
  VersionProjection,
  VolumeDetailProjection,
  VolumeSummary,
  VolumeLifecycleProjection
} from '../../application/contracts'
import { demoBundle, demoProjectRecords, starterMarket, type DemoProjectRecord } from './fixtures'

const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
const clone = <T>(value: T): T => structuredClone(value)
const nowLabel = () => new Date().toLocaleString('zh-CN', { hour12: false })

function page<T>(items: T[], request: PageRequest, defaultSize = 30): PageResult<T> {
  const start = Math.max(0, Number(request.cursor ?? 0) || 0)
  const pageSize = Math.max(1, Math.min(request.pageSize ?? defaultSize, 100))
  const slice = items.slice(start, start + pageSize)
  const next = start + slice.length < items.length ? String(start + slice.length) : null
  return { items: clone(slice), nextCursor: next, total: items.length }
}

function chapterVolumeId(projectId: string, chapterNumber: number) {
  if (projectId === 'proj-river-archive') return chapterNumber <= 30 ? 'vol-1' : chapterNumber <= 60 ? 'vol-2' : 'vol-3'
  return 'vol-1'
}

function chapterListItem(projectId: string, chapter: ProjectBundle['chapters'][number]): ChapterListItem {
  return {
    id: chapter.id,
    volumeId: chapterVolumeId(projectId, chapter.number),
    number: chapter.number,
    title: chapter.title,
    status: chapter.status,
    version: chapter.version,
    wordCount: chapter.wordCount,
    updatedAt: chapter.updatedAt
  }
}

function libraryItem(record: DemoProjectRecord): ProjectLibraryItem {
  const bundle = record.bundle
  return {
    id: bundle.project.id,
    title: bundle.project.title,
    subtitle: bundle.project.subtitle,
    platform: bundle.project.platform,
    genre: bundle.project.genre,
    mode: bundle.project.mode,
    phase: bundle.project.phase,
    runState: bundle.project.runState,
    blockReason: bundle.project.blockReason,
    signingStatus: bundle.project.signingStatus,
    lifecycle: record.lifecycle,
    progress: bundle.project.progress,
    totalCost: bundle.project.totalCost,
    totalWords: bundle.chapters.filter(chapter => chapter.status === 'official').reduce((sum, chapter) => sum + chapter.wordCount, 0),
    committedChapters: bundle.project.committedChapters,
    candidateChapters: bundle.project.candidateChapters,
    pendingDecisions: bundle.decisions.filter(item => item.status === 'pending').length,
    revision: bundle.project.revision,
    generatedRevision: bundle.project.revision,
    createdAt: record.createdAt,
    updatedAt: bundle.project.updatedAt,
    lastOpenedAt: record.lastOpenedAt,
    operational: true
  }
}

function emptySigning(): ProjectBundle['signing'] {
  return {
    status: 'NOT_STARTED',
    score: 0,
    verdict: '尚未进入开篇评估。',
    dimensions: demoBundle.signing.dimensions.map(item => ({ ...item, score: 0, note: '等待评估' }))
  }
}

function newProjectBundle(request: CommandRequest, id: string): ProjectBundle {
  const payload = request.payload ?? {}
  const bundle = clone(demoBundle)
  const idea = String(payload.idea ?? '新的小说计划')
  const title = String(payload.title ?? '').trim() || '未命名作品'
  const selectedMarket = starterMarket.find(item => item.id === payload.marketOpportunityId)
  const platformInput = String(payload.platform ?? '')
  const platform = selectedMarket?.platform ?? (platformInput && platformInput !== 'AI 推荐' ? platformInput : '待 AI 推荐')
  const tags = Array.isArray(payload.tags) ? payload.tags.map(String) : []
  const genre = selectedMarket?.direction ?? (tags.length ? tags.join(' · ') : '待确定题材')

  bundle.project = {
    id,
    title,
    subtitle: idea.length > 52 ? `${idea.slice(0, 52)}…` : idea,
    platform,
    genre,
    mode: payload.mode === 'AUTO' ? 'AUTO' : 'DIRECTOR',
    phase: 'PITCHING',
    runState: 'IDLE',
    blockReason: null,
    signingStatus: 'NOT_STARTED',
    progress: 10,
    totalCost: 0.24,
    totalElapsed: '1 分钟',
    committedChapters: 0,
    candidateChapters: 0,
    latestReceiptId: `RCP-${Date.now()}`,
    revision: 1,
    updatedAt: nowLabel()
  }
  bundle.market = clone(starterMarket)
  bundle.timeline = bundle.timeline.map((item, index) => ({
    ...item,
    status: index === 0 ? 'active' : 'locked',
    description: index === 0 ? '等待你从三份故事方向中选择或组合。' : item.description,
    receipt: undefined,
    elapsed: undefined
  }))
  bundle.tasks = []
  bundle.artifacts = [{
    id: 'art-brief',
    title: '创作要求',
    kind: 'Creative Brief',
    version: 'v1',
    status: 'official',
    summary: `目标平台：${platform}；偏好：${tags.join('、') || '由 AI 推断'}；禁区：${String(payload.forbidden ?? '未设置')}`,
    updatedAt: nowLabel()
  }]
  bundle.decisions = []
  bundle.chapters = []
  bundle.versions = []
  bundle.signing = emptySigning()
  bundle.canon = { characters: [], relations: [], timeline: [], foreshadows: [], knowledge: [] }
  return bundle
}

function deriveVolumes(record: DemoProjectRecord): VolumeSummary[] {
  const p = record.bundle.project
  if (p.id === 'proj-river-archive') {
    return [
      { id:'vol-1', number:1, title:'水灯照骨', status:'CLOSED', chapterStart:1, chapterEnd:30, committedChapters:30, plannedChapters:30, wordCount:106800, targetWords:105000, progress:100, goal:'建立沿江十二镇与第一批旧案规则。', health:'healthy', updatedAt:'2026-07-05 21:10' },
      { id:'vol-2', number:2, title:'十二镇回声', status:'CLOSED', chapterStart:31, chapterEnd:60, committedChapters:30, plannedChapters:30, wordCount:112420, targetWords:110000, progress:100, goal:'让分散旧案汇聚为同一条历史因果链。', health:'healthy', updatedAt:'2026-07-29 18:40' },
      { id:'vol-3', number:3, title:'河灯归岸', status:'CLOSED', chapterStart:61, chapterEnd:86, committedChapters:26, plannedChapters:26, wordCount:96840, targetWords:95000, progress:100, goal:'回收核心谜团并完成群像终局。', health:'healthy', updatedAt:p.updatedAt }
    ]
  }
  const status: VolumeSummary['status'] = p.signingStatus !== 'PASS' && p.phase !== 'COMPLETED'
    ? 'LOCKED'
    : p.phase === 'VOLUME_PRODUCTION'
      ? (p.runState === 'PAUSED' ? 'PAUSED' : 'PRODUCING')
      : 'PLANNING'
  const planned = Math.max(36, p.committedChapters || 0)
  const committed = Math.min(p.committedChapters, planned)
  return [{
    id:'vol-1', number:1, title:'真名旧案', status, chapterStart:1, chapterEnd:planned,
    committedChapters: committed, plannedChapters: planned,
    wordCount: record.bundle.chapters.filter(ch => ch.status === 'official').reduce((sum, ch) => sum + ch.wordCount, 0),
    targetWords: 125000, progress: planned ? Math.round(committed / planned * 100) : 0,
    goal:'确认玉符与寒江剑派旧案的联系，击穿第一层身份骗局。',
    health: record.bundle.decisions.some(d => d.priority === 'blocking' && d.status === 'pending') ? 'attention' : 'healthy',
    updatedAt:p.updatedAt
  }]
}

function healthIssues(record: DemoProjectRecord): HealthIssue[] {
  if (record.bundle.project.id === 'proj-river-archive') return []
  if (!record.bundle.project.committedChapters) return [
    { id:'health-start', category:'主线', severity:'info', title:'长篇健康将在正式章节产生后开始积累', detail:'当前仍处于战略或开篇阶段，没有足够正文样本判断长期漂移。', location:'全书', suggestedAction:'继续完成开篇准入与首卷规划。' }
  ]
  return [
    { id:'health-f1', category:'伏笔', severity:'attention', title:'半枚玉符需要在首卷中段获得第一次回报', detail:'该线索已经连续承担悬疑功能，如果长期只增加问题而没有局部答案，会降低读者信任。', location:'第 2 章起', suggestedAction:'在第 8–12 章安排一次可验证的小回收。', advanced:{ obligationId:'obl-foreshadow-003', dependencyType:'REVIEW_REQUIRED' } },
    { id:'health-char', category:'角色', severity:'info', title:'柳青堂需要更稳定地参与主线推进', detail:'当前角色功能清楚，但尚未形成独立行动目标。', location:'第一卷前段', suggestedAction:'让其在下一阶段主动提供一次错误但有价值的判断。', advanced:{ obligationId:'obl-char-011' } },
    { id:'health-repeat', category:'重复', severity:'attention', title:'“陌生人抛出身份谜语”模式已连续出现', detail:'悬疑钩子有效，但若继续使用同构揭示，会让惊奇感递减。', location:'第 1–3 章', suggestedAction:'下一次信息升级改为行动后果，而不是再次口头揭密。', advanced:{ dependencyType:'SOFT' } },
    { id:'health-impact', category:'修改影响', severity:'risk', title:'改动“玄机子知道真实姓名”会影响多处后文', detail:'这项设定同时连接师徒关系、断剑线和后续身份骗局，建议先做影响预览。', location:'故事规则 / 第一卷', suggestedAction:'从“版本 / 修改 / Retcon”创建变更请求。', advanced:{ dependencyType:'HARD', affectedEstimate:'12–18 个后续节点' } }
  ]
}

function historyEntries(record: DemoProjectRecord): HistoryEntry[] {
  const fromVersions: HistoryEntry[] = record.bundle.versions.map((v: VersionProjection) => ({
    id:`hist-${v.id}`, kind:'VERSION', title:`章节版本 ${v.label}`, status:v.current ? 'applied' : 'approved', createdAt:v.createdAt,
    summary:v.reason, impact:'仅影响该章节的正文实现；历史版本保持不可变。', scope:`章节 ${v.chapterId}`, receipt:v.current ? record.bundle.project.latestReceiptId : undefined
  }))
  if (record.bundle.project.id !== 'proj-moheng-demo') return fromVersions
  return [
    { id:'chg-001', kind:'CHANGE_REQUEST', title:'增强第三章主角主动性', status:'impact_ready', createdAt:'2026-08-23 23:20', summary:'保持身份信息不变，调整主角在雨夜来客场景中的主动试探与反击。', impact:'预计只影响第 3 章与开篇评估，不改变已提交 Canon。', scope:'第 3 章 / Opening', receipt:undefined },
    { id:'retcon-001', kind:'RETCON', title:'示例：调整玄机子掌握真名的时间点', status:'draft', createdAt:'2026-08-23 22:52', summary:'这是尚未应用的 RetconPlan 示例，重大正式事实不能普通保存覆盖。', impact:'预计影响师徒关系、断剑证据与第一卷后段伏笔。', scope:'正式事实 / 第一卷' },
    ...fromVersions
  ]
}

/**
 * In-memory UI demonstration adapter only. It deliberately exposes the same scoped query boundary
 * as the production adapter, but never acts as durable authority.
 */
export class BrowserDemoPort implements FrontendPort {
  private globalSettings: SettingsProjection = clone(demoBundle.settings)
  private records = new Map<string, DemoProjectRecord>(demoProjectRecords.map(record => [record.bundle.project.id, clone(record)]))
  private receipts = new Map<string, CommandReceipt>()

  private record(projectId: string) {
    const record = this.records.get(projectId)
    if (!record) throw new Error('项目不存在，可能已经被删除。')
    record.lastOpenedAt = nowLabel()
    return record
  }

  async queryProjectLibrary(): Promise<ProjectLibraryProjection> {
    await wait(45)
    const projects = [...this.records.values()].map(libraryItem).sort((a, b) => b.lastOpenedAt.localeCompare(a.lastOpenedAt))
    return clone({
      projects,
      totals: {
        all: projects.length,
        active: projects.filter(item => item.lifecycle === 'ACTIVE').length,
        archived: projects.filter(item => item.lifecycle === 'ARCHIVED').length,
        producing: projects.filter(item => item.lifecycle === 'ACTIVE' && item.runState === 'ACTIVE').length,
        waitingDecision: projects.filter(item => item.lifecycle === 'ACTIVE' && item.pendingDecisions > 0).length,
        completed: projects.filter(item => item.lifecycle === 'ACTIVE' && item.phase === 'COMPLETED').length,
        totalCost: projects.reduce((sum, item) => sum + item.totalCost, 0)
      }
    })
  }

  async queryProjectOverview(request: ProjectQueryRequest): Promise<ProjectOverviewProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    const volumes = deriveVolumes(record)
    const recentChapters = record.bundle.chapters.filter(ch => ch.status === 'official').slice(-4).reverse().map(ch => chapterListItem(request.projectId, ch))
    const pendingDecisionCount = record.bundle.decisions.filter(item => item.status === 'pending').length
    return clone({
      project: record.bundle.project,
      pendingDecisionCount,
      recentChapters,
      recentArtifacts: record.bundle.artifacts.slice(0, 5),
      currentVolume: volumes.find(v => !['CLOSED'].includes(v.status)) ?? volumes.at(-1),
      healthSummary: healthIssues(record).some(i => i.severity === 'risk')
        ? { level:'risk', label:'有高影响项待处理', note:'建议在继续大规模生产前查看长篇健康。' }
        : healthIssues(record).some(i => i.severity === 'attention')
          ? { level:'attention', label:'总体稳定，有少量关注项', note:'当前没有硬阻断，但存在需要规划层处理的长期风险。' }
          : { level:'healthy', label:'长期状态稳定', note:'当前未发现需要立即处理的长期风险。' }
    })
  }

  async queryMarketStrategy(request: ProjectQueryRequest): Promise<MarketStrategyProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    return clone({
      projectId: request.projectId,
      targetPlatform: record.bundle.project.platform,
      strategyTitle: '悬疑身份线优先 · 保持成长回报',
      readerPromise: record.bundle.pitches.find(p => p.recommended)?.promise ?? '等待确认故事方向后编译市场策略。',
      platformPolicyVersion: 'FQ-2026.08',
      snapshotCollectedAt: '2026-08-22', confidence: 86, lastPulseAt: '2026-08-22 09:10', nextReview: '下一卷规划前',
      opportunities: record.bundle.market,
      signals: [
        { label:'题材需求', value:'较强', trend:'up', note:'悬疑元素在跨题材内容中的互动信号继续上升。' },
        { label:'同质化压力', value:'中等', trend:'flat', note:'宗门成长供给高，身份谜局提供差异空间。' },
        { label:'长篇承载力', value:'高', trend:'flat', note:'卷级谜局与成长线适合多卷推进。' }
      ],
      guardrails:['市场只决定约束与机会，不复制具体作品情节。','低置信度证据不得自动改写作品核心方向。','卷间 MarketPulse 只能在授权范围内调整节奏与包装。']
    })
  }

  async queryStoryDirections(request: ProjectQueryRequest): Promise<StoryDirectionProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    return clone({
      pitches: record.bundle.pitches,
      selectedPitchId: record.bundle.project.phase === 'PITCHING' ? undefined : (record.bundle.pitches.find(p => p.recommended)?.id ?? record.bundle.pitches[0]?.id),
      strategy: {
        storyPromise: record.bundle.pitches.find(p => p.recommended)?.promise ?? '等待选择立项方向。',
        protagonistDirection: '主角必须主动追索真实身份，不能被动接受终局真相。',
        endingBoundary: '终局必须让“名字与真实经历”的权力冲突得到明确解决。',
        lengthBudget: '120–180 万字', currentBookPlanVersion:'全书战略 v1', currentMainContractVersion:'全书约束 v1'
      }
    })
  }

  async queryStorySetup(request: ProjectQueryRequest): Promise<StorySetupProjection> {
    await wait(35)
    const record = this.record(request.projectId)
    const selected = record.bundle.project.phase === 'PITCHING' ? null : (record.bundle.pitches.find(p => p.recommended)?.id ?? record.bundle.pitches[0]?.id ?? null)
    return clone({
      projectId: request.projectId,
      status: selected ? (record.bundle.project.phase === 'OPENING_PRODUCTION' ? 'SETUP_SAVED' : 'SELECTED') : 'NOT_SELECTED',
      selectedPitchId: selected,
      selectedPitchIds: selected ? [selected] : [],
      storyCore: selected ? { protagonist: '等待你确认的主角', desire: '等待你确认的核心欲望', mechanism: '等待你确认的核心机制', conflict: '等待你确认的核心冲突', readerPromise: '等待你确认的读者承诺' } : null,
      openingReady: record.bundle.project.phase === 'OPENING_PRODUCTION',
    })
  }

  async queryOpeningReady(request: ProjectQueryRequest): Promise<OpeningReadyProjection> {
    await wait(35)
    const ready = this.record(request.projectId).bundle.project.phase === 'OPENING_PRODUCTION'
    return clone({
      projectId: request.projectId,
      ready,
      firstChapterPlan: ready ? { artifactId: 'demo-chapter-plan', version: 1, payloadHash: 'd'.repeat(64), owner: 'B07::PlanningOwner' } : null,
      firstChapter: ready ? { bookId: 'demo-book', volumeId: 'demo-volume', chapterId: 'demo-chapter' } : null,
      firstChapterGoal: '让主角面对一个必须主动回应的异常',
      firstThreeChapterPromise: '兑现一次局部回报并留下更深的问题',
      forbiddenReveals: ['前三章不揭开机制来源'],
      readerDesire: '看见主角如何用自己的选择承担代价。',
    })
  }

  async queryAssistantRepairPlan(request: ProjectQueryRequest & { question: string }): Promise<AssistantRepairPlan> {
    await wait(30)
    const revision = this.record(request.projectId).bundle.project.revision
    return {
      repairPlanId: `demo-repair-${revision}`,
      projectId: request.projectId,
      basedOnProjectRevision: revision,
      diagnosis: { summary: '项目展示需要重新核对' },
      findingRefs: [],
      targetAuthorityOwner: 'B04::ProjectionOwner',
      proposedOperations: [{ operation: 'rebuild_projection', target: '项目展示', params: {}, postconditions: {} }],
      affectedArtifacts: ['项目展示'],
      affectedScopes: ['当前作品'],
      riskLevel: 'LOW',
      requiresBackup: false,
      expectedPostconditions: [{ kind: 'projection_current', expected: true }],
      operationDigest: 'e'.repeat(64),
      createdAt: new Date().toISOString(),
    }
  }

  async queryProductionCenter(request: ProjectQueryRequest): Promise<ProductionCenterProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    return clone({
      timeline: record.bundle.timeline,
      tasks: record.bundle.tasks,
      artifacts: page(record.bundle.artifacts, request, 12),
      signing: record.bundle.signing,
      activeWork: { label: record.bundle.tasks.find(t => t.status === 'active')?.title ?? '当前没有运行中的制作步骤', detail: record.bundle.tasks.find(t => t.status === 'active')?.description ?? '可以从当前阶段继续。', chapterNumber: record.bundle.project.committedChapters + 1, progress: record.bundle.tasks.find(t => t.status === 'active')?.progress },
      costSummary: { currentVolume: Math.min(record.bundle.project.totalCost, 48.2), projectTotal: record.bundle.project.totalCost, budgetRemaining: Math.max(0, record.bundle.settings.monthlyBudget - record.bundle.project.totalCost) }
    })
  }

  async queryVolumeList(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>> {
    await wait(45)
    return page(deriveVolumes(this.record(request.projectId)), request, 20)
  }

  async queryVolumeDetail(request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeDetailProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    const volume = deriveVolumes(record).find(v => v.id === request.volumeId)
    if (!volume) throw new Error('卷不存在。')
    const chapters = record.bundle.chapters.map(ch => chapterListItem(request.projectId, ch)).filter(ch => ch.volumeId === request.volumeId)
    return clone({
      volume,
      planVersion:'VolumePlan v1', contractVersion:'VolumeContract v1', stateLedgerVersion:'PlanningStateLedger v1',
      arcs:[
        { id:'arc-1', title:'入局', chapterRange:`${volume.chapterStart}–${Math.min(volume.chapterStart + 7, volume.chapterEnd)} 章`, status: volume.progress > 22 ? 'done' : 'active', goal:'建立本卷主要问题与人物站位。' },
        { id:'arc-2', title:'真假证据', chapterRange:`${Math.min(volume.chapterStart + 8, volume.chapterEnd)}–${Math.min(volume.chapterStart + 22, volume.chapterEnd)} 章`, status: volume.progress > 55 ? 'active' : 'waiting', goal:'让局部线索互相冲突并产生一次中段回报。' },
        { id:'arc-3', title:'卷终回收', chapterRange:`${Math.min(volume.chapterStart + 23, volume.chapterEnd)}–${volume.chapterEnd} 章`, status: volume.status === 'CLOSED' ? 'done' : 'waiting', goal:'完成本卷主问题，并形成下一卷可消费的退出状态。' }
      ],
      chapters: page(chapters, request, 30),
      openObligations: volume.status === 'CLOSED' ? 0 : 7,
      overdueForeshadows: volume.status === 'CLOSED' ? 0 : 1,
      exitEligibility: volume.status === 'CLOSED' ? { ready:true, blockers:[] } : { ready:false, blockers:['本卷主线尚未完成','仍有 1 个关键伏笔未到回收条件','Volume Gate 尚未运行'] },
      nextVolumeEligibility: volume.status === 'CLOSED' ? '已解锁下一卷规划' : '本卷关闭并提交 VolumeExitState 后解锁'
    })
  }

  async queryDecisions(request: ProjectQueryRequest & { status?: 'pending' | 'resolved' | 'all' }): Promise<PageResult<DecisionItem>> {
    await wait(35)
    const items = this.record(request.projectId).bundle.decisions.filter(d => !request.status || request.status === 'all' || d.status === request.status)
    return page(items, request, 40)
  }

  async queryReaderVolumes(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>> {
    return this.queryVolumeList(request)
  }

  async queryChapterList(request: ProjectQueryRequest & { volumeId: string }): Promise<PageResult<ChapterListItem>> {
    await wait(35)
    const record = this.record(request.projectId)
    const items = record.bundle.chapters.map(ch => chapterListItem(request.projectId, ch)).filter(ch => ch.volumeId === request.volumeId)
    return page(items, request, 30)
  }

  async queryChapterFinal(request: ProjectQueryRequest & { chapterId: string }): Promise<ChapterProjection> {
    await wait(35)
    const record = this.record(request.projectId)
    const chapter = record.bundle.chapters.find(ch => ch.id === request.chapterId)
    if (!chapter) throw new Error('章节不存在。')
    return clone({ ...chapter, volumeId: chapterVolumeId(request.projectId, chapter.number) })
  }

  async queryChapterVersions(request: ProjectQueryRequest & { chapterId: string }): Promise<PageResult<VersionProjection>> {
    await wait(30)
    const items = this.record(request.projectId).bundle.versions.filter(v => v.chapterId === request.chapterId)
    return page(items, request, 20)
  }

  async queryCanon(request: ProjectQueryRequest) {
    await wait(40)
    const canon = this.record(request.projectId).bundle.canon
    return clone({
      characters: page(canon.characters, request, 60),
      relations: page(canon.relations, request, 80),
      timeline: page(canon.timeline, request, 80),
      foreshadows: page(canon.foreshadows, request, 80),
      knowledge: page(canon.knowledge, request, 80)
    })
  }

  async queryBookHealth(request: ProjectQueryRequest): Promise<BookHealthProjection> {
    await wait(45)
    const record = this.record(request.projectId)
    const issues = healthIssues(record)
    const hasRisk = issues.some(i => i.severity === 'risk')
    const hasAttention = issues.some(i => i.severity === 'attention')
    return clone({
      score: record.bundle.project.phase === 'COMPLETED' ? 94 : hasRisk ? 73 : hasAttention ? 84 : 91,
      level: record.bundle.project.phase === 'COMPLETED' ? 'healthy' : hasRisk ? 'risk' : hasAttention ? 'attention' : 'healthy',
      checkedAt: record.bundle.project.updatedAt,
      progressToEnding: record.bundle.project.phase === 'COMPLETED' ? 100 : Math.min(92, Math.round(record.bundle.project.progress * .72)),
      targetLength:'120–180 万字',
      currentWords: record.bundle.chapters.filter(ch => ch.status === 'official').reduce((s, ch) => s + ch.wordCount, 0),
      issues: page(issues, request, 50),
      counters:{ openObligations:record.bundle.project.phase === 'COMPLETED' ? 0 : 12, overdueObligations:0, openForeshadows:record.bundle.project.phase === 'COMPLETED' ? 0 : 3, overdueForeshadows:record.bundle.project.phase === 'COMPLETED' ? 0 : 1, absentCharacters:record.bundle.project.phase === 'COMPLETED' ? 0 : 1, repeatedPatterns:record.bundle.project.phase === 'COMPLETED' ? 0 : 1 }
    })
  }

  async queryHistory(request: ProjectQueryRequest & { kind?: 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON' }): Promise<HistoryProjection> {
    await wait(40)
    const all = historyEntries(this.record(request.projectId))
    const filtered = !request.kind || request.kind === 'all' ? all : all.filter(e => e.kind === request.kind)
    return clone({
      entries: page(filtered, request, 30),
      stats:{ versions:all.filter(e=>e.kind==='VERSION').length, changes:all.filter(e=>e.kind==='CHANGE_REQUEST').length, retcons:all.filter(e=>e.kind==='RETCON').length, pending:all.filter(e=>['draft','impact_ready'].includes(e.status)).length }
    })
  }

  async queryChangeRequest(request: ProjectQueryRequest & { changeRequestId: string }): Promise<Record<string, unknown>> {
    await wait(40)
    const entry = historyEntries(this.record(request.projectId)).find(item => item.id === request.changeRequestId)
    if (!entry) throw new Error('变更记录不存在。')
    return clone({ changeRequestId: entry.id, summary: entry.summary, status: entry.status, impact: { scopeClass: entry.scope, items: [], bounded: true, paged: true } })
  }

  async queryProjectSettings(request: ProjectQueryRequest): Promise<SettingsProjection> {
    await wait(30)
    return clone(this.record(request.projectId).bundle.settings)
  }

  async searchFulltext(request: ProjectQueryRequest & { text: string }): Promise<PageResult<SearchHit>> {
    await wait(40)
    const q = request.text.trim().toLowerCase()
    if (!q) return { items:[], nextCursor:null, total:0 }
    const record = this.record(request.projectId)
    const hits: SearchHit[] = []
    for (const chapter of record.bundle.chapters) {
      const hay = `${chapter.title}\n${chapter.content.join('\n')}`.toLowerCase()
      if (!hay.includes(q)) continue
      const paragraph = chapter.content.find(p => p.toLowerCase().includes(q)) ?? chapter.content[0] ?? ''
      hits.push({ id:`hit-${chapter.id}`, chapterId:chapter.id, chapterNumber:chapter.number, volumeId:chapterVolumeId(request.projectId, chapter.number), title:chapter.title, excerpt:paragraph, matchedBy:'keyword' })
    }
    for (const character of record.bundle.canon.characters) {
      if (character.name.toLowerCase().includes(q) && !hits.some(h => h.chapterId === record.bundle.chapters[0]?.id)) {
        const ch = record.bundle.chapters[0]
        if (ch) hits.push({ id:`hit-alias-${character.id}`, chapterId:ch.id, chapterNumber:ch.number, volumeId:chapterVolumeId(request.projectId, ch.number), title:ch.title, excerpt:`人物索引命中：${character.name} · ${character.note}`, matchedBy:'alias' })
      }
    }
    return page(hits, request, 30)
  }

  async queryMarketOpportunities() {
    await wait(40)
    return clone(starterMarket)
  }

  async queryGlobalSettings(): Promise<SettingsProjection> {
    await wait(30)
    return clone(this.globalSettings)
  }

  async queryNarrativeState(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_narrative_state') }
  async queryContinuity(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_continuity') }
  async queryVolumeProgress(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_volume_progress') }
  async queryContextManifest(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_context_manifest') }
  async queryPlanStale(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_plan_stale') }
  async queryPlanRoute(): Promise<never> { throw new Error('IPC_UNSUPPORTED:query_plan_route') }
  async queryVolumeLifecycle(_request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeLifecycleProjection> { throw new Error('IPC_UNSUPPORTED:query_volume_lifecycle') }
  async supportSession(request: { projectId: string; question: string; mode?: 'INSPECT' | 'DIAGNOSE'; scope?: string }): Promise<SupportSessionProjection> {
    await wait(45)
    const supported = !request.question.includes('没有')
    return clone({
      plan: { status: 'READY', steps: [{ capabilityId: 'query_project_meta', status: 'DONE' }] },
      calls: [{ capabilityId: 'query_project_meta', result: supported ? { summary: '项目记录已查询' } : {} }],
      evidenceLedger: supported ? [{ sourceRefs: ['demo:project-record'], truncated: false }] : [],
      finding: { claim: supported ? '当前项目记录已完成查询。' : '现有记录不足以确认这件事。', supportStatus: supported ? 'SUPPORTED' : 'INSUFFICIENT', sourceRefs: supported ? ['demo:project-record'] : [] },
      receipt: { projectId: request.projectId, projectRevision: this.record(request.projectId).bundle.project.revision },
    })
  }
  async supportCheckpointResume(): Promise<never> { throw new Error('IPC_UNSUPPORTED:support_checkpoint_resume') }

  async executeCommand(request: CommandRequest, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    const commandId = request.commandId ?? `CMD-${Date.now()}-${Math.floor(Math.random() * 1000)}`
    const prior = this.receipts.get(commandId)
    if (prior) return clone(prior)
    const emit = (status: CommandStatus, message: string) => onStatus?.(status, message)

    emit('submitting', '正在提交指令…')
    await wait(55)
    emit('accepted', '指令已接收，正在进入处理队列…')
    await wait(65)
    emit('processing', '正在处理，并等待权威投影刷新…')
    await wait(90)

    let finalStatus: CommandReceipt['finalStatus'] = 'committed'
    let message = '操作已完成。'
    const receiptId = `RCP-${Date.now()}`
    let resultProjectId = request.projectId

    if (request.type === 'project_create_from_idea' || request.type === 'project_create_from_market') {
      const id = `proj-${Date.now().toString(36)}-${Math.floor(Math.random() * 999).toString(36)}`
      const bundle = newProjectBundle(request, id)
      this.records.set(id, { lifecycle:'ACTIVE', createdAt:nowLabel(), lastOpenedAt:nowLabel(), bundle })
      resultProjectId = id
      message = '新作品已经建立，三份故事方向已生成。'
    } else if (request.type === 'market_refresh' || request.type === 'market_pulse_refresh') {
      message = request.type === 'market_pulse_refresh' ? '卷间市场复核已完成；策略投影已更新，未越权改写作品核心方向。' : '市场数据刷新任务已完成，推荐结果已重新投影。'
    } else if (request.type === 'global_settings_save') {
      if (request.payload?.settings) this.globalSettings = clone(request.payload.settings as unknown as SettingsProjection)
      message = '应用级设置已保存。各作品仍保留自己的项目预算与目标平台投影。'
    } else if (request.type === 'project_duplicate') {
      const source = request.projectId ? this.records.get(request.projectId) : undefined
      if (!source) { finalStatus='failed'; message='复制失败：原作品不存在。' }
      else {
        const id = `proj-copy-${Date.now().toString(36)}`
        const record = clone(source)
        record.bundle.project.id=id; record.bundle.project.title=`${source.bundle.project.title} · 副本`; record.bundle.project.runState='IDLE'; record.bundle.project.blockReason=null; record.bundle.project.revision=1; record.bundle.project.latestReceiptId=receiptId; record.bundle.project.updatedAt=nowLabel()
        record.lifecycle='ACTIVE'; record.createdAt=nowLabel(); record.lastOpenedAt=nowLabel(); this.records.set(id,record); resultProjectId=id
        message='作品副本已创建。副本默认处于未运行状态，不继承活动任务。'
      }
    } else if (request.type === 'project_archive' || request.type === 'project_restore' || request.type === 'project_delete' || request.type === 'project_rename') {
      const record = request.projectId ? this.records.get(request.projectId) : undefined
      if (!record) { finalStatus='failed'; message='操作失败：作品不存在。' }
      else if (request.expectedRevision != null && request.expectedRevision !== record.bundle.project.revision) { finalStatus='blocked'; message='作品状态已变化，请刷新后重试。' }
      else if (request.type === 'project_archive') {
        if (record.bundle.project.runState === 'ACTIVE') { finalStatus='blocked'; message='作品正在生产，请先暂停生产后再归档。' }
        else { record.lifecycle='ARCHIVED'; record.bundle.project.updatedAt=nowLabel(); message='作品已归档。正文、图谱、版本与生产回执均保留。' }
      } else if (request.type === 'project_restore') { record.lifecycle='ACTIVE'; record.bundle.project.updatedAt=nowLabel(); message='作品已从归档恢复。' }
      else if (request.type === 'project_delete') { this.records.delete(record.bundle.project.id); message='作品已删除。正式实现中必须同时执行项目级安全清理策略。' }
      else {
        const title=String(request.payload?.title ?? '').trim(); if(!title){finalStatus='failed';message='重命名失败。'} else {record.bundle.project.title=title;record.bundle.project.updatedAt=nowLabel();record.bundle.project.revision+=1;message='作品名称已更新。'}
      }
    } else {
      const record = request.projectId ? this.records.get(request.projectId) : undefined
      if (!record) { finalStatus='failed'; message='操作失败：没有找到目标作品。' }
      else if (request.expectedRevision != null && request.expectedRevision !== record.bundle.project.revision) { finalStatus='blocked'; message='作品状态已变化，请刷新当前工作区后重试。' }
      else {
        const bundle = record.bundle
        switch (request.type) {
          case 'production_start': bundle.project.runState='ACTIVE'; bundle.project.blockReason=null; message='制作任务已启动。'; break
          case 'production_pause': bundle.project.runState='PAUSED'; bundle.project.blockReason='USER_PAUSE'; message='制作已暂停，正式进度不受影响。'; break
          case 'production_resume': bundle.project.runState='ACTIVE'; bundle.project.blockReason=null; message='制作已恢复。'; break
          case 'production_cancel': bundle.project.runState='FAILED'; bundle.project.blockReason=null; message='本次生产已取消；正式提交内容保持不变。'; break
          case 'pitch_regenerate': bundle.project.phase='PITCHING'; bundle.project.runState='IDLE'; bundle.project.blockReason=null; message='三份故事方向已根据新约束重新生成。'; break
          case 'pitch_select': bundle.project.phase='STORY_BLUEPRINT'; bundle.project.runState='ACTIVE'; bundle.project.blockReason=null; bundle.project.progress=Math.max(bundle.project.progress,16); message='故事方向已确认，系统将继续建立书级战略。'; break
          case 'story_setup_save': bundle.project.phase='OPENING_CONTRACT'; bundle.project.runState='IDLE'; bundle.project.progress=Math.max(bundle.project.progress,28); message='故事核心已保存，等待你批准开篇准备。'; break
          case 'opening_ready': bundle.project.phase='OPENING_PRODUCTION'; bundle.project.runState='IDLE'; bundle.project.progress=Math.max(bundle.project.progress,36); message='开篇准备已完成，已生成第一章的最小规划。'; break
          case 'opening_start_creating': bundle.project.phase='OPENING_PRODUCTION'; bundle.project.runState='ACTIVE'; bundle.project.progress=Math.max(bundle.project.progress,40); message='已进入正式创作中心。'; break
          case 'book_plan_create': bundle.project.phase='BOOK_PLANNING'; message='BookPlan 与 MainContract 规划任务已创建。'; break
          case 'book_plan_revise': bundle.project.phase='BOOK_PLANNING'; message='书级战略修订已进入影响分析。'; break
          case 'volume_plan_build':
            if (bundle.project.signingStatus !== 'PASS') { finalStatus='blocked'; message='开篇评估尚未通过，当前卷详细规划保持锁定。' }
            else { bundle.project.phase='POST_SIGNING_PLANNING'; message='当前卷详细规划任务已建立。' }
            break
          case 'volume_production_start':
            if (bundle.project.signingStatus !== 'PASS') { finalStatus='blocked'; message='开篇评估尚未通过，不能进入卷级生产。' }
            else { bundle.project.phase='VOLUME_PRODUCTION'; bundle.project.runState='ACTIVE'; bundle.project.blockReason=null; message='当前卷生产已启动。' }
            break
          case 'volume_production_pause': bundle.project.runState='PAUSED'; bundle.project.blockReason='USER_PAUSE'; message='当前卷生产已暂停，可从卷/章节边界恢复。'; break
          case 'volume_close': message='卷关闭检查已提交；只有 Volume Gate 通过后才会生成 VolumeExitState。'; break
          case 'decision_submit': {
            const id=String(request.payload?.decisionId ?? ''); const target=bundle.decisions.find(item=>item.id===id); if(target) target.status='resolved'
            if(id==='dec-opening'){bundle.project.runState='ACTIVE';bundle.project.blockReason=null;bundle.tasks=bundle.tasks.map(task=>task.id==='task-review'?{...task,status:'done' as const,progress:100}:task.id==='task-repair'?{...task,status:'active' as const,progress:24}:task)}
            message='你的选择已记录，系统将按新的正式约束继续。'; break
          }
          case 'revision_request':
          case 'chapter_revision_request': message=request.payload?.kind==='chapter_rollback'?'版本回退已提交并生成新的正式版本记录。':'章节修订请求已记录，系统会先分析影响范围。'; break
          case 'change_request_create': message='变更请求已创建，正在计算类型化影响范围；尚未改写正式状态。'; break
          case 'change_request_apply': message='已批准的变更请求已提交，受影响投影将按依赖规则刷新。'; break
          case 'retcon_plan_create': message='RetconPlan 已建立；正式事实仍保持不变，等待影响确认。'; break
          case 'retcon_apply': message='Retcon 已按批准计划提交，并保留原正式历史与审计回执。'; break
          case 'export_create': message='正式成品导出任务已完成。'; break
          case 'backup_create': message='项目备份已创建并完成快速校验。'; break
          case 'integrity_check_run': message='完整性检查已完成，未发现权威数据断链。'; break
          case 'settings_save': if(request.payload?.settings) bundle.settings=clone(request.payload.settings as unknown as SettingsProjection); message='项目设置已保存。'; break
          case 'signing_retry': bundle.signing.status='IN_REVIEW';bundle.project.signingStatus='IN_REVIEW';bundle.signing.verdict='正在重新执行完整开篇评估。';bundle.project.runState='ACTIVE';bundle.project.blockReason=null;message='开篇评估已重新启动。';break
          case 'signing_demo_pass':
            bundle.signing={status:'PASS',score:87,verdict:'开篇评估通过：平台匹配、连续升级和原创差异均达到当前门槛，后续生产已解锁。',dimensions:demoBundle.signing.dimensions.map((d,i)=>({...d,score:82+(i%7)*2,note:'演示评估已通过。'})),receipt:{id:'SIGN-RCP-0001',signedAt:nowLabel(),platformPolicyVersion:'FQ-2026.08',pitchVersion:'pitch-b@v1',constitutionVersion:'constitution@v3',namingVersion:'naming@v2',openingContractVersion:'opening@v2',chapterHashes:['sha256:ch1…','sha256:ch2…','sha256:ch3…'],reviewReportId:'SIGN-REPORT-001',receiptHash:'sha256:sign…a71'}}
            bundle.project.phase='POST_SIGNING_PLANNING';bundle.project.signingStatus='PASS';bundle.project.runState='ACTIVE';bundle.project.blockReason=null;message='开篇评估已通过，并生成正式签约回执。';break
          case 'signing_demo_revise': bundle.signing.status='REVISE';bundle.signing.score=74;bundle.signing.verdict='需要修订：第三章主角主动性未达到门槛。';bundle.project.phase='OPENING_PRODUCTION';bundle.project.signingStatus='REVISE';bundle.project.runState='BLOCKED';bundle.project.blockReason='SIGNING_REQUIRED';message='评估要求修订，第四章仍保持锁定。';break
          case 'signing_demo_reject': bundle.signing.status='REJECTED';bundle.signing.score=51;bundle.signing.verdict='未通过：核心故事承诺与目标平台不匹配。';bundle.project.signingStatus='REJECTED';bundle.project.runState='BLOCKED';bundle.project.blockReason='SIGNING_REQUIRED';message='评估未通过，需要重新调整故事方向。';break
          default: message='演示命令已处理。'
        }
        if(finalStatus==='committed'){bundle.project.revision+=1;bundle.project.latestReceiptId=receiptId;bundle.project.updatedAt=nowLabel()}
      }
    }

    // Match the Rust contract: projectId is the command source; resultProjectId
    // is the produced/target project. Create has no source, so this demo follows
    // the current Rust shape and uses the created project for both fields.
    const sourceProjectId = request.projectId ?? resultProjectId
    const receipt: CommandReceipt = { commandId, projectId:sourceProjectId, resultProjectId, acceptedAt:new Date().toISOString(), finalStatus, message, receiptId }
    this.receipts.set(commandId, clone(receipt))
    emit(finalStatus,message)
    return receipt
  }

  async testProvider(_settings: SettingsProjection, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt> {
    const commandId=`DIAG-${Date.now()}`
    onStatus?.('submitting','正在提交连接诊断…'); await wait(60); onStatus?.('processing','正在检查服务可达性与模型配置…'); await wait(80)
    const receipt: CommandReceipt={commandId,acceptedAt:new Date().toISOString(),finalStatus:'committed',message:'连接诊断完成：演示适配器返回可用。',receiptId:`RCP-${Date.now()}`}
    onStatus?.('committed',receipt.message); return receipt
  }
}

export const frontendPort = new BrowserDemoPort()
