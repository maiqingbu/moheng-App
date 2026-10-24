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
  | 'UNKNOWN_REMOTE_OUTCOME'

export type SigningStatus = 'NOT_STARTED' | 'IN_REVIEW' | 'REVISE' | 'REJECTED' | 'PASS' | 'FAIL' | 'NOT_VERIFIED' | 'STALE'
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
export interface ContinuityEvidenceProjection {
  artifactId: string
  version: number
  rangeStart?: number | null
  rangeEnd?: number | null
  quote?: string | null
}
export interface ContinuitySourceProjection { kind: string; ref: string }
export interface ContinuityThreadViewProjection {
  threadId: string
  threadClass: string
  description: string
  fulfillmentObject: string | null
  logicalStatus: string
  sourceEventRef: string | null
  evidenceRefs: ContinuityEvidenceProjection[]
  participantEntities: string[]
  resumeCondition: Record<string, unknown>
  authorityLevel: 'A' | 'B' | 'C'
  currentDisposition: string
  sourceRefs: ContinuitySourceProjection[]
}
export interface ContinuityProjection {
  projectId: string
  projectRevision: number
  threads: ContinuityThreadViewProjection[]
  truncated: boolean
}
/** FX-12: a withdraw-responsibility request awaiting the user's decision. */
export interface PendingWithdrawRequest {
  changeRequestId: string
  responsibilityRef: string
  summary: string
  origin: 'USER_DELETE' | 'CHAPTER_AUDIT'
  createdAt: string
}
export interface PendingWithdrawRequestsProjection {
  projectId: string
  projectRevision: number
  items: PendingWithdrawRequest[]
  truncated: boolean
}
export interface NarrativeContextProjection { payloadSchema: string; compiledFromProjectRevision: number; compiledFromStateDigest: string; [key: string]: unknown }
/** Optional backend projection for the 2026-09-19 PRD. Absence is not PASS. */
export interface NarrativeValidationProjection {
  canExecute: boolean
  context?: {
    status: 'COMPLETE' | 'OVER_BUDGET' | 'INCOMPLETE' | 'UNAVAILABLE'
    reason: string
    hardSourceCount?: number
    hardTokens?: number
    hardTokenLimit?: number
    sourceRefs?: string[]
  }
  audit?: {
    status: 'COMPLETE' | 'INCOMPLETE' | 'UNRESOLVED' | 'STALE' | 'UNAVAILABLE'
    reason: string
    candidateRef?: { artifactId: string; version: number; payloadHash: string }
    evidenceRefs?: string[]
  }
}
export interface PlanStaleProjection { projectId: string; projectRevision: number; status: 'VALID' | 'PLAN_STALE' }
export interface SupportSessionProjection {
  plan: Record<string, unknown>
  calls: Record<string, unknown>[]
  finding: Record<string, unknown>
  receipt: Record<string, unknown>
  evidenceLedger?: Record<string, unknown>[]
  /** FX-21: how the question was understood. */
  intent?: AssistantIntentResolution
}
/** FX-22: one backup of a work; revision is the one it was taken at. */
export interface BackupEntry {
  backupName: string
  checksumSha256: string
  createdAt: string
  revision: number | null
  sizeBytes: number | null
  available: boolean
}
/** FX-21: how the project assistant understood a question. */
export interface AssistantIntentResolution {
  intent: string
  source: 'MODEL' | 'KEYWORD'
  subjects?: string[]
  attribute?: string | null
  modelRunId?: string | null
  fallbackReason?: string | null
}
export interface SupportCheckpointProjection { checkpointId: string; projectId: string; projectRevision: number; status: 'SAFE' | 'CHECKPOINT_STALE' }

export interface StorySetupProjection {
  projectId: string
  status: 'NOT_SELECTED' | 'SELECTED' | 'SETUP_SAVED'
  selectedPitchId: string | null
  selectedPitchIds?: string[]
  storyCore: Record<string, unknown> | null
  openingReady: boolean
  provenance?: Record<string, unknown>
}

/** CRAFT v3 opening craft setup: story bible and style card candidates. */
export interface CraftStageProjection {
  candidates: Array<Record<string, any> & { id: string }>
  confirmed: (Record<string, any> & { id: string }) | null
}
export interface CraftSetupProjection {
  storyBible: CraftStageProjection
  styleCard: CraftStageProjection
  sample: { text: string; digest: string; savedAt: string; metrics: Record<string, number>; minChars: number } | null
  archetypes: Array<{ id: string; name: string; genres: string[] }>
}

export interface OpeningReadyProjection {
  projectId: string
  ready: boolean
  firstChapterPlan: { artifactId: string; version: number; payloadHash: string; owner: string } | null
  firstChapter: { bookId: string; volumeId: string; chapterId: string } | null
  firstChapterGoal: string
  firstThreeChapterPromise: string
  forbiddenReveals: string[]
  readerDesire: string
  openingProgress?: OpeningProgressProjection
}

export interface OpeningProgressProjection {
  completed: number
  total: number
  currentChapter: number | null
  previousCommitted: boolean
  nextPrepared: boolean
}

export interface AssistantRepairPlan {
  action?: 'EXECUTABLE' | 'PROPOSAL_ONLY' | 'NO_ACTION'
  /** FX-21: how the repair request was understood. */
  intent?: AssistantIntentResolution
  repairPlanId: string
  projectId: string
  basedOnProjectRevision: number
  diagnosis: Record<string, unknown>
  findingRefs: string[]
  targetAuthorityOwner: string
  proposedOperations: Array<Record<string, unknown>>
  affectedArtifacts: string[]
  affectedScopes: string[]
  riskLevel: string
  requiresBackup: boolean
  expectedPostconditions: Array<Record<string, unknown>>
  operationDigest: string
  createdAt: string
}

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
  /** Completion ratio in [0, 1]. */
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
  /** Completion ratio in [0, 1]. */
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
  kind: '稳健型' | '潜力型' | '差异化型' | '官方展示'
  title: string
  direction: string
  platform: string
  audience: string
  demand: number | null
  competition: number | null
  homogeneityRisk: number | null
  saturation: number | null
  growth: number | null
  monetization: number | null
  newcomerDifficulty: number | null
  longFormCapacity: number | null
  originalitySpace: number | null
  aiFeasibility: number | null
  complianceRisk: number | null
  dataConfidence: number | null
  recommendationConfidence: number | null
  evidence: string[]
  recommendWhy: string
  notRecommendWhy: string
  source: string
  collectedAt: string
  policyVersion: string | null
  topicId?: string
  metricsBasis?: 'OFFICIAL_PUBLICATION_SAMPLE' | 'USER_PROVIDED_DATA' | 'DEMO_ONLY'
  sampleCount?: number
  categorySampleCount?: number
  topMarkedCount?: number
  collectedAtEpochMs?: number
  sourceDigest?: string | null
  stale?: boolean
}

export type MarketSourceStatus = 'READY' | 'PARTIAL' | 'SOURCE_UNAVAILABLE' | 'STALE' | 'NOT_CONFIGURED' | 'USER_DATA_ONLY' | 'DEMO_ONLY'

export interface MarketPolicyReference {
  title: string
  url: string
  publishedAt?: string | null
  externalTextClassification: 'DATA_ONLY'
}

export interface MarketDiscoveryProjection {
  status: MarketSourceStatus
  source?: {
    sourceId: string
    sourceUrl: string
    digest: string | null
    observedAtEpochMs: number
    sampleCount: number
    metricsBasis: string
  } | null
  policyStatus: MarketSourceStatus | 'INDEXED'
  policySourceUrl?: string | null
  policyDigest?: string | null
  policyObservedAtEpochMs?: number | null
  policyReferences: MarketPolicyReference[]
  errorCode?: string | null
  opportunities: MarketOpportunity[]
  limitations: string[]
}

export interface MarketStrategyProjection {
  projectId: string
  status: MarketSourceStatus
  sourceStatus: MarketSourceStatus
  policyStatus: MarketSourceStatus | 'INDEXED'
  targetPlatform: string | null
  strategyTitle: string
  readerPromise: string
  platformPolicyVersion: string | null
  snapshotCollectedAt: string | null
  snapshotCollectedAtEpochMs: number | null
  confidence: number | null
  lastPulseAt: string | null
  nextReview: string
  opportunities: MarketOpportunity[]
  selectedOpportunityId: string | null
  source: MarketDiscoveryProjection['source']
  sourceDigest: string | null
  policyReferences: MarketPolicyReference[]
  sourceAttempt: { observedAt: string | null; errorCode: string | null }
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
  generationState?: {
    status: 'FAILED'
    jobId: string
    retryAvailable: boolean
  }
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
  status: 'done' | 'active' | 'waiting' | 'locked' | 'blocked' | 'failed' | 'pending'
  receipt?: string
  elapsed?: string
}

export interface ProductionTask {
  id: string
  title: string
  description: string
  status: 'done' | 'active' | 'waiting' | 'blocked' | 'failed'
  /** Completion ratio in [0, 1]. */
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
  decisionType?: string
  title: string
  summary: string
  category: DecisionCategory
  priority: 'normal' | 'high' | 'blocking'
  status: 'pending' | 'resolved'
  createdAt: string
  recommendation?: string
  evidence?: string[]
  allowedActions?: Array<'RETRY_REMOTE' | 'MARK_FAILED' | 'CANCEL_JOB'>
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

export interface ExportCapabilityProjection {
  available: boolean
  reason: string | null
  supportedFormats: string[]
  currentFinalChapterCount: number
}

export interface ExportProgressProjection {
  status: 'RUNNING' | 'FINALIZING'
  requestId: string
  completedChapters: number
  totalChapters: number
  completedCharacters: number
  totalCharacters: number
  currentChapterNumber: number | null
}

export interface ExportChapterRefProjection {
  chapterId: string
  chapterNumber: number
  title: string
  artifactId: string
  version: number
  payloadHash: string
  characterCount: number
}

export interface ExportReceiptProjection {
  schema: string
  status: 'COMPLETED'
  receiptId: string
  requestId: string
  projectId: string
  format: 'TXT'
  fileName: string
  finalPath: string
  createdAtEpochMs: number
  elapsedMs: number
  sourceProjectRevision: number
  chapters: ExportChapterRefProjection[]
  sourceCharacterCount: number
  outputCharacterCount: number
  outputByteCount: number
  sha256: string
  maxChunkCharacters: number
  maxWorkingBufferBytes: number
}

export type ExportQueryProjection = ExportProgressProjection | ExportReceiptProjection | { status: 'NONE' }

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
  score?: number
  verdict: string
  dimensions?: { label: string; score: number; note: string }[]
  summary?: { chapters?: number; canonRefs?: number; evidenceRefs?: number }
  openingChapterRefs?: string[]
  openingCanonRefs?: string[]
  signingEvidenceRefs?: string[]
  coreStatus?: 'PASS' | 'FAIL' | 'NOT_VERIFIED'
  itemResults?: Array<{
    itemId: string
    result: 'PASS' | 'FAIL' | 'NOT_VERIFIED'
    reason: string
    evidence?: Array<{
      chapterNo: number
      chapterId: string
      rangeStart: number
      rangeEnd: number
      exactQuote: string
    }>
    details?: Record<string, unknown>
  }>
  platformMatrix?: Array<{
    platformId: string
    displayName: string
    mainstream: boolean
    status: 'PASS' | 'FAIL' | 'NOT_VERIFIED'
    items: Array<{ itemId: string; result: 'PASS' | 'FAIL' | 'NOT_VERIFIED'; reason: string }>
    submissionWords?: { minimum: number | null; current: number; distance: number | null }
  }>
  confirmation?: { confirmed: boolean; confirmedBy?: string | null; confirmedAt?: string | null; platformId?: string | null }
  declarationPrompt?: string
  receiptRef?: { artifactId: string; version: number; payloadHash: string }
  evaluation?: {
    status: 'NOT_STARTED' | 'IN_REVIEW' | 'COMPLETED' | 'FAILED'
    phase: string
    summary?: string
    steps: Array<{ id: string; label: string; status: 'pending' | 'active' | 'done' | 'failed' | 'waiting' }>
  }
  receipt?: {
    id: string
    signedAt?: string | null
    platformPolicyVersion?: string
    pitchVersion?: string
    constitutionVersion?: string
    namingVersion?: string
    openingContractVersion?: string | number | null
    chapterHashes: string[]
    reviewReportId?: string | null
    receiptHash?: string
    confirmedPlatform?: string | null
    ruleSetVersion?: string
    ruleSetDigest?: string
  }
}

export interface CreativeSkillsProjection {
  packageVersion: string
  packageDigest: string
  selection?: {
    primary?: { id: string; nameZh?: string; role?: string; bundleDigest?: string }
    support?: Array<{ id: string; nameZh?: string; role?: string; bundleDigest?: string }>
  }
  context?: {
    contextDigest?: string
    contextManifestRef?: string
    projectRevision?: number
    boundedRefs?: Array<{ kind: string; reference?: string }>
    budget?: { contextTokens?: number; modelCalls?: number; repairRuns?: number }
  }
  invocations?: Array<{
    invocationId?: string
    skillId?: string
    skillVersion?: string
    role?: string
    resourceRefs?: string[]
    methodRefs?: string[]
    projectContextRefs?: string[]
    provider?: string
    model?: string
    outputDigest?: string
  }>
  methodInjectionProvenance?: Array<{
    skillId?: string
    version?: string
    bundleDigest?: string
    loadedResources?: Array<{ reference?: string; digest?: string }>
    renderModelRunId?: string
  }>
  proposal?: { kind?: string; status?: string; summary?: string; suggestions?: string[] }
}

export interface NarrativeQualityProjection {
  ref?: { artifactId: string; version: number; payloadHash: string }
  softOnly?: boolean
  tasteProfileVersion?: string
  dimensions?: Record<string, { score: number; severity: string; evidence: string[] }>
  findings?: Array<{ findingId: string; dimension: string; severity: string; evidence: string[]; repairable: boolean }>
  repairableDimensions?: string[]
  evaluatorKind?: string
  modelProvenance?: { evaluator?: string; evaluatorCalls?: number; provider?: string; model?: string }
  softAssessment?: { readerDesire?: string; qualityScore?: number }
  invocationProvenance?: CreativeSkillsProjection['invocations']
  methodInjectionProvenance?: CreativeSkillsProjection['methodInjectionProvenance']
  repair?: { status?: string; run: number; limit: number; scope: string[]; escalation: string | null }
}

export interface NarrativeTasteProjection {
  ref?: { artifactId: string; version: number; payloadHash: string }
  version?: string
  digest?: string
  softOnly?: boolean
}

export interface ProductionCenterProjection {
  validation?: NarrativeValidationProjection
  timeline: TimelineItem[]
  tasks: ProductionTask[]
  artifacts: PageResult<ArtifactSummary>
  signing: SigningProjection
  bookPlan?: {
    status: 'PENDING' | 'APPROVED'
    ref?: { artifactId: string; version: number; payloadHash: string } | null
    seedPreserved?: boolean
    reapprovalRequired?: boolean
    steps?: Array<{ id: string; label: string; status: 'pending' | 'active' | 'done' | 'failed' | 'waiting' }>
  }
  gatePolicy?: {
    version: number
    policyId: string | null
    needsUpgrade: boolean
    resourceRef: { version: string; sha256: string } | null
    rules: Array<Record<string, unknown>>
  } | null
  runtimeOperation?: {
    jobId: string
    kind: 'B02::OPENING_EVALUATION' | 'B02::BOOK_PLAN'
    phase: string
    status: string
    errorCode?: string | null
    active: boolean
  } | null
  activeWork: {
    label: string
    detail: string
    chapterNumber?: number
    /** Completion ratio in [0, 1]. */
    progress?: number
  }
  costSummary: {
    currentVolume: number
    projectTotal: number
    budgetRemaining: number | null
    chapterUsed?: number
    chapterBudget?: number | null
    chapterRemaining?: number | null
    monthUsed?: number
    monthlyBudget?: number | null
    monthRemaining?: number | null
  }
  productionStage?: 'CONTEXT' | 'PLAN' | 'RENDER' | 'CANDIDATE' | 'AUDIT' | 'GATE' | 'COMMIT' | 'RECONCILE' | 'COMPLETED' | 'IDLE'
  jobStatus?: string | null
  jobError?: string | null
  isActive?: boolean
  canRetry?: boolean
  lastStage?: string
  committedChapters?: number
  candidateChapters?: number
  nextChapter?: {
    id: string
    number: number
    title: string
    volumeId: string
    executionMode: 'OPENING_BOOTSTRAP' | 'LONG_FORM'
    quota?: { used: number; limit: number; allowance: number } | null
  } | null
  chapterPlan?: {
    ref: { artifactId: string; version: number; payloadHash: string }
    chapterGoal?: string
    contextManifestRef?: { artifactId: string; version: number; payloadHash: string }
    activeTaskRefs: string[]
    continuityActions: Array<{ threadRef: string; disposition: string; bridge?: string }>
    compiledFromProjectRevision?: number
    compiledFromStateDigest?: string
  } | null
  triggeredContinuity?: Array<{ threadRef: string; disposition: string; bridge?: string }>
  newResponsibilities?: Array<{
    threadRef: string
    authorityLevel: 'A' | 'B' | 'C'
    lifecycle: string
    description: string
    fulfillmentObject?: string | null
    resumeCondition?: Record<string, unknown> | null
    evidence?: { start: number; end: number; quote: string } | null
    ruleVersion?: string | null
  }>
  gate?: { status: 'PENDING' | 'PASS' | 'BLOCKED' | 'FAIL'; reason: string }
  stale?: { status: 'NO_PLAN' | 'PLAN_STALE' | 'VALID' | 'RECHECK_REQUIRED' | 'VALID_OR_REQUIRES_RELEVANT_RECHECK'; reason: string }
  modelRuns?: Array<{ modelRunId: string; capability: string; provider: string; model: string; status: string }>
  candidateRef?: { artifactId: string; version: number; payloadHash: string } | null
  finalChapterRef?: { artifactId: string; version: number; payloadHash: string } | null
  reconciliation?: 'NO_ACTION' | 'RECOMPILE_NEXT_CHAPTER' | 'RECOMPILE_ROLLING_WINDOW' | 'REPLAN_VOLUME_ROUTE' | 'CHANGE_REQUEST' | null
  repair?: RepairWorkflowProjection | null
  creativeSkills?: CreativeSkillsProjection | null
  quality?: NarrativeQualityProjection | null
  tasteProfile?: NarrativeTasteProjection | null
  readerDesire?: { readerDesire?: string; qualityScore?: number } | null
  openingProgress?: OpeningProgressProjection
}

export interface RepairWorkflowProjection {
  repairPlanId: string
  basedOnProjectRevision: number
  diagnosis: unknown
  findingRefs: string[]
  targetAuthorityOwner: string
  proposedOperations: Array<{ operation: string; target: string; params: unknown; postconditions: unknown }>
  affectedArtifacts: string[]
  affectedScopes: string[]
  riskLevel: string
  requiresBackup: boolean
  expectedPostconditions: unknown[]
  operationDigest: string
  status: 'PROPOSED' | 'AUTHORIZED' | 'REPAIR_VERIFIED' | 'REPAIR_VERIFICATION_FAILED' | 'REPAIR_VERIFICATION_INCOMPLETE'
  authorizationId?: string | null
  consumedAt?: string | null
  executionReceipt?: {
    receiptRef: string
    projectId: string | null
    repairPlanId: string | null
    authorizationId: string | null
    beforeRevision: number | null
    afterRevision: number | null
    operationDigest: string | null
    owner: string | null
    ownerReceiptRef: string | null
    executedAt: string | null
    status: string
    postconditions: unknown[]
  } | null
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

export interface RetconContextProjection {
  facts: Array<{ factId: string; label: string; current: boolean; predicate?: string; source?: string }>
  evidence: Array<{ artifactId: string; version: number; payloadHash: string; label: string; kind?: string; supportsFactId?: string; relationship?: 'FACT_SOURCE' | 'ADDITIONAL_RETCON_EVIDENCE'; sourceArtifactId?: string; sourceVersion?: number; sourceChapterId?: string | null }>
  grounding?: { status: 'UNIQUE' | 'AMBIGUOUS' | 'NO_MATCH' | 'NEEDS_QUERY'; candidateCount: number; needsUserSelection: boolean; evidenceStatus?: 'EXACT' | 'EVIDENCE_GAP' | 'NEEDS_FACT_SELECTION' }
  selectedFact?: { factId: string; label: string; current: boolean } | null
}

export interface DependencySummaryBootstrapProjection {
  projectId: string
  summary: { summaryStatus: 'READY' | 'NOT_READY'; totalEdges: number; mutationEpoch: number; computedEpoch: number }
  job: Record<string, unknown> | null
  owner: string
}

export interface SettingsProjection {
  provider: string
  model: string
  endpoint: string
  monthlyBudget: number | null
  chapterBudget: number | null
  pauseBeforeOverrun?: boolean
  budgetHistory?: Array<{ setting: 'monthlyBudget' | 'chapterBudget'; oldValue: number | null; newValue: number | null; commandId: string; changedAt: string }>
  modelPricing?: {
    version: number
    updatedAt?: string | null
    entries: Array<{ providerId: string; modelId: string; inputPricePerMillionTokens: number | null; outputPricePerMillionTokens: number | null; cachedInputPricePerMillionTokens?: number | null }>
    history: Array<{ version: number; updatedAt: string; commandId: string; entries: Array<{ providerId: string; modelId: string; inputPricePerMillionTokens: number | null; outputPricePerMillionTokens: number | null; cachedInputPricePerMillionTokens?: number | null }> }>
  }
  privacyMode: 'local-first' | 'balanced'
  sendDraftOnly: boolean
  /** FX-18: the work's default chapter length range (project settings only). */
  chapterLengthRange?: { min: number; max: number }
  exportFormat: 'DOCX' | 'Markdown' | 'TXT' | 'EPUB'
  platform: string
  theme: 'light' | 'system' | 'dark'
  providerId?: string
  modelId?: string
  /** Optional model used only for writing chapter prose; blank uses modelId. */
  renderModelId?: string
  /** Optional model used only for the audit extraction; blank uses modelId. */
  auditModelId?: string
  enabled?: boolean
  timeoutMs?: number
  secretStatus?: 'AVAILABLE' | 'NOT_CONFIGURED' | 'UNAVAILABLE'
  capabilityStatus?: 'AVAILABLE' | 'UNAVAILABLE' | 'ERROR'
}

export interface ProviderSettingsInput extends SettingsProjection {
  /** Ephemeral IPC input; never part of SettingsProjection or persisted state. */
  apiKey?: string
}

export interface ProjectOverviewProjection {
  project: ProjectSnapshot
  pendingDecisionCount: number
  recentChapters: ChapterListItem[]
  recentArtifacts: ArtifactSummary[]
  currentVolume?: {
    id: string
    number: number
    title: string
    /** Completion ratio in [0, 1]. */
    progress: number
  }
  healthSummary: { level: 'not_evaluated' | 'insufficient_data' | 'source_unavailable' | 'healthy' | 'attention' | 'risk'; label: string; note: string }
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
  /** Completion ratio in [0, 1]. */
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
  runtimeOperation?: {
    jobId: string
    kind: string
    phase: string
    status: string
    errorCode?: string | null
    active: boolean
    steps?: Array<{ id: string; label: string; status: 'active' | 'done' | 'pending' | 'failed' }>
  } | null
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
  routeInvariants?: string[]
  requiredExitConditions?: Array<Record<string, unknown>>
  capacityBudget?: { minChapters?: number; preferredChapters?: number; maxChapters?: number }
  goalContractRefs?: Array<{ artifactId: string; version: number; payloadHash: string }>
  compiledFromProjectRevision?: number
  sourceEvidenceRefs?: string[]
  semanticDigest?: string
  continuityThreads?: Array<Record<string, unknown>>
  futureReservations?: Array<Record<string, unknown>>
  currentChapterPlan?: { artifactId: string; version: number; payloadHash: string } | null
  horizonPreview?: Array<Record<string, unknown>>
  truncated: boolean
}

export interface VolumeLifecycleProjection {
  projectId: string
  projectRevision: number
  volumeId: string
  planningState: Record<string, unknown>
  route: PlanningRouteProjection
  latestClose: Record<string, unknown> | null
  latestHandoff: Record<string, unknown> | null
  routeReplans: Record<string, unknown>[]
  owner: string
}

/** B13 long-form health is a rebuildable projection, never an authority. */
export type HealthIssue = import('../generated/contracts').HealthIssue
export type HealthSourcePage = import('../generated/contracts').HealthSourcePage
export type LongFormHealthProjectionR1 = import('../generated/contracts').LongFormHealthProjectionR1
export type BookHealthProjection = LongFormHealthProjectionR1

export interface HistoryEntry {
  id: string
  kind: 'VERSION' | 'CHANGE_REQUEST' | 'RETCON'
  source?: string | null
  targetClass?: string | null
  authorityType?: string | null
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
  idempotencyKey?: string
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
  actionStatus?: 'CHANGE_REQUEST_SUBMITTED'
  changeRequestId?: string
  receiptId?: string
  beforeRevision?: number
  afterRevision?: number
  requestDigest?: string
  result?: Record<string, unknown>
  receipt?: Record<string, unknown>
  postverify?: Record<string, unknown>
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
  | 'market_opportunity_select'
  | 'market_pulse_refresh'
  | 'story_directions_generate'
  | 'pitch_regenerate'
  | 'pitch_select'
  | 'story_setup_save'
  | 'story_bible_generate'
  | 'story_bible_confirm'
  | 'style_card_generate'
  | 'style_card_confirm'
  | 'style_sample_save'
  | 'opening_ready'
  | 'opening_start_creating'
  | 'book_plan_create'
  | 'book_plan_revise'
  | 'planning_create_volume_intent'
  | 'planning_create_volume_plan'
  | 'planning_prepare_next_chapter'
  | 'planning_compile_chapter_plan'
  | 'production_start'
  | 'production_pause'
  | 'production_resume'
  | 'production_cancel'
  | 'production_reset_chapter_quota'
  | 'repair_plan_create'
  | 'repair_authorize'
  | 'repair_execute'
  | 'volume_plan_build'
  | 'volume_production_start'
  | 'volume_production_pause'
  | 'volume_close'
  | 'volume_replan_route'
  | 'volume_handoff'
  | 'decision_submit'
  | 'thread_responsibility_adjust'
  | 'thread_withdraw_decide'
  | 'gate_policy_update'
  | 'revision_request'
  | 'change_request_create'
  | 'impact_preflight'
  | 'impact_expand'
  | 'impact_resume'
  | 'impact_cancel'
  | 'change_request_decide'
  | 'change_request_apply'
  | 'retcon_plan_create'
  | 'retcon_apply'
  | 'dependency_summary_bootstrap'
  | 'opening_evaluate'
  | 'signing_confirm'
  | 'backup_create'
  | 'backup_restore'
  | 'integrity_check_run'
  | 'endgame_contract_create'
  | 'complete_book'
  | 'settings_save'
  | 'global_settings_save'
  | 'model_pricing_save'

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
  queryCommandReceipt(request: { projectId: string; idempotencyKey: string }): Promise<CommandReceipt | null>
  queryMarketStrategy(request: ProjectQueryRequest): Promise<MarketStrategyProjection>
  queryStoryDirections(request: ProjectQueryRequest): Promise<StoryDirectionProjection>
  queryStorySetup(request: ProjectQueryRequest): Promise<StorySetupProjection>
  queryCraftSetup(request: ProjectQueryRequest): Promise<CraftSetupProjection>
  queryOpeningReady(request: ProjectQueryRequest): Promise<OpeningReadyProjection>
  queryAssistantRepairPlan(request: ProjectQueryRequest & { question: string }): Promise<AssistantRepairPlan>
  queryBackups(request: ProjectQueryRequest): Promise<{ projectId: string; backups: BackupEntry[] }>
  classifyAssistantIntent(request: ProjectQueryRequest & { question: string; entry: 'DIAGNOSE' | 'CHANGE' | 'REPAIR' }): Promise<AssistantIntentResolution>
  queryProductionCenter(request: ProjectQueryRequest): Promise<ProductionCenterProjection>
  queryVolumeList(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>>
  queryVolumeDetail(request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeDetailProjection>
  queryDecisions(request: ProjectQueryRequest & { status?: 'pending' | 'resolved' | 'all' }): Promise<PageResult<DecisionItem>>
  queryReaderVolumes(request: ProjectQueryRequest): Promise<PageResult<VolumeSummary>>
  queryChapterList(request: ProjectQueryRequest & { volumeId: string }): Promise<PageResult<ChapterListItem>>
  queryChapterFinal(request: ProjectQueryRequest & { chapterId: string }): Promise<ChapterProjection>
  queryChapterVersions(request: ProjectQueryRequest & { chapterId: string }): Promise<PageResult<VersionProjection>>
  queryExportCapability(request: ProjectQueryRequest): Promise<ExportCapabilityProjection>
  createExport(request: { projectId: string; requestId: string; format: 'TXT' }): Promise<ExportReceiptProjection>
  queryExportReceipt(request: { projectId: string; requestId: string }): Promise<ExportQueryProjection>
  queryLatestExportReceipt(request: ProjectQueryRequest): Promise<ExportQueryProjection>
  queryCanon(request: ProjectQueryRequest): Promise<CanonProjection>
  queryRetconContext(request: ProjectQueryRequest & { query?: string }): Promise<RetconContextProjection>
  queryDependencySummaryBootstrap(request: ProjectQueryRequest): Promise<DependencySummaryBootstrapProjection>
  queryBookHealth(request: ProjectQueryRequest): Promise<BookHealthProjection>
  queryBookHealthFactorSources(request: ProjectQueryRequest & { factorCode: string }): Promise<HealthSourcePage>
  queryHistory(request: ProjectQueryRequest & { kind?: 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON' }): Promise<HistoryProjection>
  queryChangeRequest(request: ProjectQueryRequest & { changeRequestId: string }): Promise<Record<string, unknown>>
  queryProjectSettings(request: ProjectQueryRequest): Promise<SettingsProjection>
  searchFulltext(request: ProjectQueryRequest & { text: string }): Promise<PageResult<SearchHit>>
  queryMarketOpportunities(): Promise<MarketDiscoveryProjection>
  queryGlobalSettings(): Promise<SettingsProjection>
  queryNarrativeState(request: NarrativeRuntimeQueryRequest): Promise<NarrativeStateProjection>
  queryContinuity(request: NarrativeRuntimeQueryRequest): Promise<ContinuityProjection>
  queryPendingWithdrawRequests(request: ProjectQueryRequest): Promise<PendingWithdrawRequestsProjection>
  queryVolumeProgress(request: NarrativeRuntimeQueryRequest & { volumeId: string; taskIds?: string[] }): Promise<import('../generated/contracts').VolumeProgressProjection>
  queryContextManifest(request: NarrativeRuntimeQueryRequest & { executionMode?: 'OPENING_BOOTSTRAP' | 'LONG_FORM' }): Promise<NarrativeContextProjection>
  queryPlanStale(request: NarrativeRuntimeQueryRequest & { compiledFromProjectRevision: number; compiledFromStateDigest: string; currentStateDigest?: string }): Promise<PlanStaleProjection>
  queryPlanRoute(request: Omit<NarrativeRuntimeQueryRequest, 'currentChapter'> & { volumeId: string }): Promise<PlanningRouteProjection>
  queryVolumeLifecycle(request: ProjectQueryRequest & { volumeId: string }): Promise<VolumeLifecycleProjection>
  queryEndgameStatus(request: ProjectQueryRequest): Promise<CompletionStatusProjection>
  queryCompletionStatus(request: ProjectQueryRequest): Promise<CompletionStatusProjection>
  queryEndgameMappingOptions(request: ProjectQueryRequest): Promise<EndgameMappingWorkflowProjection>
  queryEndgameAdvice(request: ProjectQueryRequest): Promise<SoftCreativeAdviceProjection>
  queryCompletionQuality(request: ProjectQueryRequest): Promise<SoftCreativeAdviceProjection>
  supportSession(request: { projectId: string; question: string; mode?: 'INSPECT' | 'DIAGNOSE'; scope?: string } & NarrativeRuntimeQueryRequest): Promise<SupportSessionProjection>
  supportCheckpointResume(request: { projectId: string; checkpointId: string }): Promise<SupportCheckpointProjection>
  executeCommand(request: CommandRequest, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt>
  testProvider(settings: ProviderSettingsInput, onStatus?: (status: CommandStatus, message: string) => void): Promise<CommandReceipt>
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
  HealthCoverage,
  HealthCounters,
  HealthFactor,
  HealthIssue as GeneratedHealthIssue,
  HealthIssuesPage,
  HealthProvenance,
  HealthSourceRef,
} from '../generated/contracts'

export type CompletionGateStatus = 'READY' | 'BLOCKED' | 'NEEDS_DECISION' | 'NEEDS_RECHECK' | 'SOURCE_UNAVAILABLE'

export interface CompletionCheckProjection {
  kind: string
  status: CompletionGateStatus
  summary: string
  blockerCount: number
  advisoryCount: number
  sourceRefs: string[]
}

export interface CompletionStatusProjection {
  projectId: string
  projectRevision: number
  status: CompletionGateStatus
  checks: CompletionCheckProjection[]
  blockerCount: number
  advisoryCount: number
  relevantStateDigest: string
  sourceAvailable: boolean
  requestedEndingConfirmed: boolean
  completed: boolean
  hasHistoricalCompletion: boolean
}

export interface SoftCreativeAdviceProjection {
  softOnly: true
  proposal?: { summary?: string; suggestions?: string[] }
  review?: { summary?: string; suggestions?: string[] }
  projectRevision: number
  sourceAvailable: boolean
  runtimeCheckRequests: unknown[]
  userAction: string
}

export interface EndgameMappingWorkflowProjection {
  reservationCandidates: Record<string, EndgameMappingCandidate[]>
  hardPayoffCandidates: Record<string, EndgameMappingCandidate[]>
  mysteries: Array<{ key: string; id: string; label: string }>
  reservations: Array<{ key: string; id: string; label: string }>
  hardPayoffs: Array<{ key: string; id: string; label: string }>
  responsibilityCandidates: Array<{ ref: string; label: string }>
  fulfillmentCandidates: Array<{ ref: string; label: string }>
}

export interface EndgameMappingCandidate {
  ref: string
  label: string
  volume?: string
  chapterNo?: number | null
  evidenceSummary?: string
  relationValidated?: boolean
}
