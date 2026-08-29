export type ProjectPhase =
  | 'SETUP'
  | 'PITCHING'
  | 'STORY_BLUEPRINT'
  | 'NAMING'
  | 'CONSTITUTION'
  | 'BOOK_PLANNING'
  | 'CANON_INITIALIZATION'
  | 'OPENING_CONTRACT'
  | 'OPENING_PRODUCTION'
  | 'SIGNING'
  | 'POST_SIGNING_PLANNING'
  | 'VOLUME_PRODUCTION'
  | 'ENDGAME'
  | 'COMPLETED'
  | 'EXPORTED'

export type RunState = 'IDLE' | 'ACTIVE' | 'PAUSED' | 'WAITING_DECISION' | 'BLOCKED' | 'FAILED' | 'COMPLETED'
export type BlockReason =
  | null
  | 'USER_PAUSE'
  | 'BUDGET_LIMIT'
  | 'DECISION_REQUIRED'
  | 'POLICY_EXPIRED'
  | 'SIGNING_REQUIRED'
  | 'SIGNING_INVALIDATED'
  | 'CONTRACT_FAILURE'
  | 'CANON_CONFLICT'
  | 'RETRY_EXHAUSTED'
  | 'PERSISTENCE_FAILURE'

export type SigningStatus = 'NOT_STARTED' | 'IN_REVIEW' | 'REVISE' | 'REJECTED' | 'PASS' | 'STALE'
export type CommandStatus = 'idle' | 'submitting' | 'accepted' | 'processing' | 'committed' | 'blocked' | 'failed'
export type ProjectLifecycle = 'ACTIVE' | 'ARCHIVED'

export interface PageRequest {
  cursor?: string | null
  pageSize?: number
}

export interface PageResult<T> {
  items: T[]
  nextCursor: string | null
  total: number
}

/** Every project query carries projectId + explicit scope + page window. */
export interface ProjectQueryRequest extends PageRequest {
  projectId: string
  scope: string
}

export interface NarrativeRuntimeQueryRequest extends ProjectQueryRequest {
  entityIds?: string[]
  references?: string[]
  chapterId?: string
  volumeId?: string
  currentChapter?: number
  currentVolumeId?: string
  presentEntityIds?: string[]
  acquiredFactIds?: string[]
  satisfiedTaskIds?: string[]
  meetings?: string[][]
}

export interface NarrativeStateProjection { projectId: string; projectRevision: number; view: Record<string, unknown> }
export interface ContinuityProjection { projectId: string; projectRevision: number; threads: import('../generated/contracts').ContinuityThreadProjection[] }
export interface NarrativeContextProjection { payloadSchema: string; compiledFromProjectRevision: number; compiledFromStateDigest: string; [key: string]: unknown }
export interface PlanStaleProjection { projectId: string; projectRevision: number; status: 'VALID' | 'PLAN_STALE' }
export interface SupportSessionProjection { plan: Record<string, unknown>; calls: Record<string, unknown>[]; finding: Record<string, unknown>; receipt: Record<string, unknown> }
export interface SupportCheckpointProjection { checkpointId: string; projectId: string; projectRevision: number; status: 'SAFE' | 'CHECKPOINT_STALE' }

export interface ProjectSnapshot {
  id: string
  title: string
  subtitle: string
  platform: string
  genre: string
  mode: 'AUTO' | 'DIRECTOR'
  phase: ProjectPhase
  runState: RunState
  blockReason: BlockReason
  signingStatus: SigningStatus
  progress: number
  totalCost: number
  totalElapsed: string
  committedChapters: number
  candidateChapters: number
  latestReceiptId: string
  revision: number
  updatedAt: string
}

interface ProjectLibraryItemBase {
  id: string
  title: string
  subtitle: string
  platform: string
  genre: string
  mode: 'AUTO' | 'DIRECTOR'
  phase: ProjectPhase
  runState: RunState
  blockReason: BlockReason
  signingStatus: SigningStatus
  lifecycle: ProjectLifecycle
  progress: number
  totalCost: number
  totalWords: number
  committedChapters: number
  candidateChapters: number
  pendingDecisions: number
  createdAt: string
  updatedAt: string
  lastOpenedAt: string
}

export interface OperationalProjectLibraryItem extends ProjectLibraryItemBase {
  operational: true
  revision: number
  generatedRevision: number
  readError?: null
}

export interface DegradedProjectLibraryItem extends ProjectLibraryItemBase {
  /** The project DB could not be read; no revision may be fabricated. */
  operational: false
  revision: null
  generatedRevision: null
  readError: string
}

export type ProjectLibraryItem = OperationalProjectLibraryItem | DegradedProjectLibraryItem

export interface ProjectLibraryProjection {
  projects: ProjectLibraryItem[]
  totals: {
    all: number
    active: number
    archived: number
    producing: number
    waitingDecision: number
    completed: number
    totalCost: number
  }
}

export interface MarketOpportunity {
  id: string
  kind: '稳健型' | '潜力型' | '差异化型'
  title: string
  direction: string
  platform: string
  audience: string
  demand: number
  competition: number
  homogeneityRisk: number
  saturation: number
  growth: number
  monetization: number
  newcomerDifficulty: number
  longFormCapacity: number
  originalitySpace: number
  aiFeasibility: number
  complianceRisk: number
  dataConfidence: number
  recommendationConfidence: number
  evidence: string[]
  recommendWhy: string
  notRecommendWhy: string
  source: string
  collectedAt: string
  policyVersion: string
}

export interface MarketStrategyProjection {
  projectId: string
  targetPlatform: string
  strategyTitle: string
  readerPromise: string
  platformPolicyVersion: string
  snapshotCollectedAt: string
  confidence: number
  lastPulseAt: string
  nextReview: string
  opportunities: MarketOpportunity[]
  signals: { label: string; value: string; trend: 'up' | 'flat' | 'down'; note: string }[]
  guardrails: string[]
}

export interface PitchCandidate {
  id: string
  variant: '原始想法' | '商业增强' | '差异化'
  name: string
  recommended: boolean
  platform: string
  genre: string
  audience: string
  promise: string
  emotion: string
  reward: string
  protagonistDesire: string
  mechanism: string
  worldConflict: string
  longArc: string
  openingHook: string
  volumeGoal: string
  expectedLength: string
  commercialEdge: string
  homogeneityRisk: string
  originality: string
  difficulty: string
  failureRisk: string
  aiReason: string
}

export interface StoryDirectionProjection {
  pitches: PitchCandidate[]
  selectedPitchId?: string
  strategy: {
    storyPromise: string
    protagonistDirection: string
    endingBoundary: string
    lengthBudget: string
    currentBookPlanVersion: string
    currentMainContractVersion: string
  }
}

export interface TimelineItem {
  id: string
  order: number
  title: string
  description: string
  status: 'done' | 'active' | 'waiting' | 'locked' | 'blocked'
  receipt?: string
  elapsed?: string
}

export interface ProductionTask {
  id: string
  title: string
  description: string
  status: 'done' | 'active' | 'waiting' | 'blocked'
  progress?: number
  cost?: number
  elapsed?: string
}

export interface ArtifactSummary {
  id: string
  title: string
  kind: string
  version: string
  status: 'official' | 'candidate' | 'superseded'
  summary: string
  updatedAt: string
  technical?: Record<string, string>
}

export type DecisionCategory =
  | '立项'
  | '样章反馈'
  | 'AI 推断确认'
  | 'BookPlan'
  | '卷方向'
  | '预算'
  | '设定冲突'
  | '模拟签约'
  | '全局修改'
  | 'Retcon'
  | '大规模修复'
  | '完结冲突'

export interface DecisionItem {
  id: string
  title: string
  summary: string
  category: DecisionCategory
  priority: 'normal' | 'high' | 'blocking'
  status: 'pending' | 'resolved'
  createdAt: string
  recommendation?: string
  evidence?: string[]
}

export interface ChapterListItem {
  id: string
  volumeId: string
  number: number
  title: string
  status: 'official' | 'producing'
  version: string
  wordCount: number
  updatedAt: string
}

export interface ChapterProjection extends ChapterListItem {
  content: string[]
}

export interface VersionProjection {
  id: string
  chapterId: string
  label: string
  createdAt: string
  reason: string
  excerpt: string
  current: boolean
}

export interface SigningProjection {
  status: SigningStatus
  score: number
  verdict: string
  dimensions: { label: string; score: number; note: string }[]
  receipt?: {
    id: string
    signedAt: string
    platformPolicyVersion: string
    pitchVersion: string
    constitutionVersion: string
    namingVersion: string
    openingContractVersion: string
    chapterHashes: string[]
    reviewReportId: string
    receiptHash: string
  }
}

export interface ProductionCenterProjection {
  timeline: TimelineItem[]
  tasks: ProductionTask[]
  artifacts: PageResult<ArtifactSummary>
  signing: SigningProjection
  activeWork: {
    label: string
    detail: string
    chapterNumber?: number
    progress?: number
  }
  costSummary: {
    currentVolume: number
    projectTotal: number
    budgetRemaining: number
  }
}

export interface CanonCharacter { id: string; name: string; role: string; status: string; location: string; note: string }
export interface CanonRelation { from: string; to: string; label: string; tone: 'positive' | 'neutral' | 'negative' }
export interface CanonTimelineItem { chapter: string; title: string; detail: string; status: 'official' | 'candidate' }
export interface ForeshadowItem { id: string; title: string; planted: string; planned: string; status: 'open' | 'resolved'; detail: string }
export interface KnowledgeItem { character: string; topic: string; state: 'knows' | 'believes' | 'conceals' | 'unknown'; detail: string }

/** Demo backing shape only; production UI never receives this unbounded aggregate. */
export interface CanonData {
  characters: CanonCharacter[]
  relations: CanonRelation[]
  timeline: CanonTimelineItem[]
  foreshadows: ForeshadowItem[]
  knowledge: KnowledgeItem[]
}

export interface CanonProjection {
  characters: PageResult<CanonCharacter>
  relations: PageResult<CanonRelation>
  timeline: PageResult<CanonTimelineItem>
  foreshadows: PageResult<ForeshadowItem>
  knowledge: PageResult<KnowledgeItem>
}

export interface SettingsProjection {
  provider: string
  model: string
  endpoint: string
  monthlyBudget: number
  chapterBudget: number
  pauseBeforeOverrun: boolean
  privacyMode: 'local-first' | 'balanced'
  sendDraftOnly: boolean
  exportFormat: 'DOCX' | 'Markdown' | 'TXT' | 'EPUB'
  platform: string
  theme: 'light' | 'system' | 'dark'
}

export interface ProjectOverviewProjection {
  project: ProjectSnapshot
  pendingDecisionCount: number
  recentChapters: ChapterListItem[]
  recentArtifacts: ArtifactSummary[]
  currentVolume?: { id: string; number: number; title: string; progress: number }
  healthSummary: { level: 'healthy' | 'attention' | 'risk'; label: string; note: string }
}

export interface VolumeSummary {
  id: string
  number: number
  title: string
  status: 'LOCKED' | 'PLANNING' | 'READY' | 'PRODUCING' | 'PAUSED' | 'REVIEW' | 'CLOSED'
  chapterStart: number
  chapterEnd: number
  committedChapters: number
  plannedChapters: number
  wordCount: number
  targetWords: number
  progress: number
  goal: string
  health: 'healthy' | 'attention' | 'risk'
  updatedAt: string
}

export interface VolumeDetailProjection {
  volume: VolumeSummary
  planVersion: string
  contractVersion: string
  stateLedgerVersion: string
  arcs: { id: string; title: string; chapterRange: string; status: 'done' | 'active' | 'waiting'; goal: string }[]
  chapters: PageResult<ChapterListItem>
  openObligations: number
  overdueForeshadows: number
  exitEligibility: { ready: boolean; blockers: string[] }
  nextVolumeEligibility: string
}

export interface PlanningRouteProjection {
  projectId: string
  projectRevision: number
  volumeId: string
  volumeIntentRef: { artifactId: string; version: number; payloadHash: string }
  volumePlanRef: { artifactId: string; version: number; payloadHash: string }
  routeVersion: number
  tasks: Array<Record<string, unknown>>
  taskReality: Array<Record<string, unknown>>
  planningPressure: Record<string, unknown>
  truncated: boolean
}

export interface HealthIssue {
  id: string
  category: '主线' | '伏笔' | '角色' | '重复' | '卷目标' | '修改影响'
  severity: 'info' | 'attention' | 'risk'
  title: string
  detail: string
  location: string
  suggestedAction: string
  advanced?: Record<string, string>
}

export interface BookHealthProjection {
  score: number
  level: 'healthy' | 'attention' | 'risk'
  checkedAt: string
  progressToEnding: number
  targetLength: string
  currentWords: number
  issues: PageResult<HealthIssue>
  counters: {
    openObligations: number
    overdueObligations: number
    openForeshadows: number
    overdueForeshadows: number
    absentCharacters: number
    repeatedPatterns: number
  }
}

export interface HistoryEntry {
  id: string
  kind: 'VERSION' | 'CHANGE_REQUEST' | 'RETCON'
  title: string
  status: 'draft' | 'impact_ready' | 'approved' | 'applied' | 'rejected'
  createdAt: string
  summary: string
  impact: string
  scope: string
  receipt?: string
}

export interface HistoryProjection {
  entries: PageResult<HistoryEntry>
  stats: { versions: number; changes: number; retcons: number; pending: number }
}

export interface SearchHit {
  id: string
  chapterId: string
  chapterNumber: number
  volumeId: string
  title: string
  excerpt: string
  matchedBy: 'keyword' | 'alias' | 'location' | 'evidence'
}

/**
 * Demo-only aggregate used by the in-memory adapter as its private backing data.
 * UI code must never query or store this aggregate.
 */
export interface ProjectBundle {
  project: ProjectSnapshot
  market: MarketOpportunity[]
  pitches: PitchCandidate[]
  timeline: TimelineItem[]
  tasks: ProductionTask[]
  artifacts: ArtifactSummary[]
  decisions: DecisionItem[]
  chapters: Omit<ChapterProjection, 'volumeId'>[]
  versions: VersionProjection[]
  signing: SigningProjection
  canon: CanonData
  settings: SettingsProjection
}

export interface CommandReceipt {
  commandId: string
  /**
   * Source project the command acted on. `resultProjectId` is the project the
   * command produced — equal for in-place commands, the new copy for a duplicate.
   * A consumer must gate on the *binding* project identity/generation, and for a
   * duplicate must treat resultProjectId as a freshly-created project rather than
   * overwriting the active store (B01-P0 §六).
   */
  projectId?: string
  resultProjectId?: string
  acceptedAt: string
  finalStatus: Exclude<CommandStatus, 'idle' | 'submitting' | 'accepted' | 'processing'>
  message: string
  receiptId?: string
  beforeRevision?: number
  afterRevision?: number
  requestDigest?: string
}

export type CommandType =
  | 'project_create_from_idea'
  | 'project_create_from_market'
  | 'project_duplicate'
  | 'project_archive'
  | 'project_restore'
  | 'project_delete'
  | 'project_rename'
  | 'market_refresh'
  | 'market_pulse_refresh'
  | 'pitch_regenerate'
  | 'pitch_select'
  | 'book_plan_create'
  | 'book_plan_revise'
  | 'production_start'
  | 'production_pause'
  | 'production_resume'
  | 'production_cancel'
  | 'volume_plan_build'
  | 'volume_production_start'
  | 'volume_production_pause'
  | 'volume_close'
  | 'decision_submit'
  | 'revision_request'
  | 'chapter_revision_request'
  | 'change_request_create'
  | 'change_request_apply'
  | 'retcon_plan_create'
  | 'retcon_apply'
  | 'signing_retry'
  | 'export_create'
  | 'backup_create'
  | 'integrity_check_run'
  | 'settings_save'
  | 'global_settings_save'
  | 'signing_demo_pass'
  | 'signing_demo_revise'
  | 'signing_demo_reject'

export interface CommandRequest {
  type: CommandType
  commandId?: string
  projectId?: string
  expectedRevision?: number
  idempotencyKey?: string
  requestedAt?: string
  payload?: Record<string, unknown>
}

/**
 * UI boundary. Project workspaces use scoped/paged projections instead of a full-project bundle.
 * The production adapter maps these to typed Tauri Query/Command IPC calls.
 */
export interface FrontendPort {
  queryProjectLibrary(): Promise<ProjectLibraryProjection>
  queryProjectOverview(request: ProjectQueryRequest): Promise<ProjectOverviewProjection>
  queryMarketStrategy(request: ProjectQueryRequest): Promise<MarketStrategyProjection>
  queryStoryDirections(request: ProjectQueryRequest): Promise<StoryDirectionProjection>
  queryProductionCenter(request: ProjectQueryRequest): Promise<ProductionCenterProjection>
  queryVolumeList(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>>
  queryVolumeDetail(request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeDetailProjection>
  queryDecisions(request: ProjectQueryRequest & { status?: 'pending' | 'resolved' | 'all' }): Promise<PageResult<DecisionItem>>
  queryReaderVolumes(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>>
  queryChapterList(request: ProjectQueryRequest & { volumeId: string }): Promise<PageResult<ChapterListItem>>
  queryChapterFinal(request: ProjectQueryRequest & { chapterId: string }): Promise<ChapterProjection>
  queryChapterVersions(request: ProjectQueryRequest & { chapterId: string }): Promise<PageResult<VersionProjection>>
  queryCanon(request: ProjectQueryRequest): Promise<CanonProjection>
  queryBookHealth(request: ProjectQueryRequest): Promise<BookHealthProjection>
  queryHistory(request: ProjectQueryRequest & { kind?: 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON' }): Promise<HistoryProjection>
  queryProjectSettings(request: ProjectQueryRequest): Promise<SettingsProjection>
  searchFulltext(request: ProjectQueryRequest & { text: string }): Promise<PageResult<SearchHit>>
  queryMarketOpportunities(): Promise<MarketOpportunity[]>
  queryGlobalSettings(): Promise<SettingsProjection>
  queryNarrativeState(request: NarrativeRuntimeQueryRequest): Promise<NarrativeStateProjection>
  queryContinuity(request: NarrativeRuntimeQueryRequest): Promise<ContinuityProjection>
  queryVolumeProgress(request: NarrativeRuntimeQueryRequest & { volumeId: string; taskIds?: string[] }): Promise<import('../generated/contracts').VolumeProgressProjection>
  queryContextManifest(request: NarrativeRuntimeQueryRequest & { executionMode?: 'OPENING_BOOTSTRAP' | 'LONG_FORM' }): Promise<NarrativeContextProjection>
  queryPlanStale(request: NarrativeRuntimeQueryRequest & { compiledFromProjectRevision: number; compiledFromStateDigest: string; currentStateDigest?: string }): Promise<PlanStaleProjection>
  queryPlanRoute(request: NarrativeRuntimeQueryRequest & { volumeId: string; currentChapter?: number }): Promise<PlanningRouteProjection>
  supportSession(request: { projectId: string; question: string; mode?: 'INSPECT' | 'DIAGNOSE'; scope?: string } & NarrativeRuntimeQueryRequest): Promise<SupportSessionProjection>
  supportCheckpointResume(request: { projectId: string; checkpointId: string }): Promise<SupportCheckpointProjection>
  executeCommand(request: CommandRequest, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt>
  testProvider(settings: SettingsProjection, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt>
}

// R1 Narrative DTOs are generated from the shared contract schemas. They are
// projections/contracts only; authoritative state remains in Rust/SQLite.
export type {
  BookPlanR1,
  ChapterExitStateR1,
  ChapterPlanR1,
  ContextManifestR1,
  ContinuityThreadProjection,
  TaskProgress,
  TaskStatus,
  VolumeIntentR1,
  VolumePlanR1,
  VolumeProgressProjection,
  VolumeTask,
  VolumeExitStateR1,
} from '../generated/contracts'
