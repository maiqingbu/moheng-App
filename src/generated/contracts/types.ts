/* DO NOT EDIT - generated from contracts/schemas. */

export interface GoalConstraint {
  "id": string
  "description": string
}

export interface MacroDirection {
  "id": string
  "description": string
}

export interface FutureReservation {
  "reservationId": string
  "description": string
  "reservedForVolumeNo"?: number | null
  "reservedForVolumeId"?: string | null
  "notBeforeVolumeNo"?: number | null
  "prerequisites": Array<string>
  "mayForeshadow": boolean
  "mayReveal": boolean
  "authorityLevel": "A" | "B"
}

export interface VolumeIntentSeed {
  "seedId": string
  "description": string
}

export interface Condition {
  "kind": string
  "value"?: unknown
}

export type TaskStatus = "NOT_STARTED" | "ACTIVE" | "SATISFIED" | "BLOCKED" | "INVALIDATED"

export interface VolumeTask {
  "taskId": string
  "class": "REQUIRED" | "OPTIONAL" | "FLEXIBLE" | "CLIMAX" | "RESERVED"
  "objective": string
  "dependsOnTaskIds": Array<string>
  "continuityThreadRefs": Array<string>
  "reservationRefs": Array<string>
  "completionPredicate": Condition
  "estimatedChapterCost": { "min"?: number | null; "preferred": number; "max"?: number | null }
  "earliestPressure"?: number | null
  "latestPressure"?: number | null
}

export interface SourceRef {
  "kind": string
  "ref": string
}

export interface EvidenceRef {
  "artifactId": string
  "version": number
  "rangeStart"?: number | null
  "rangeEnd"?: number | null
  "quote"?: string | null
}

export interface OmittedSource {
  "sourceRef": SourceRef
  "reason": string
}

export interface HorizonSlot {
  "slotNo": number
  "objective": string
  "tentative": unknown
}

export interface ContinuityThreadProjection {
  "threadId": string
  "status": "OPEN" | "SUSPENDED" | "RESOLVED" | "ABANDONED"
  "threadClass": string
  "sourceEventId"?: string | null
  "evidenceRefs": Array<EvidenceRef>
  "authorityLevel": "A" | "B" | "C"
}

export interface VolumeProgressProjection {
  "volumeId": string
  "activeTaskIds": Array<string>
  "blockedTaskIds": Array<string>
  "remainingChapterCapacity": number
  "planningPressure": {  }
  "generatedRevision": number
}

export interface TaskProgress {
  "taskId": string
  "status": TaskStatus
  "lastAdvancedChapterId"?: string | null
  "evidenceRefs": Array<EvidenceRef>
  "remainingConditions": Array<Condition>
  "metric"?: { "current": number; "target": number } | null
}

export type NarrativeDefinitions = unknown

export interface BookPlanR1 {
  "payloadSchema": unknown
  "planningMaturity": "SEED" | "APPROVED"
  "storyPromise": string
  "protagonistMacroDirection": string
  "endingBoundary": string
  "mustAchieve"?: Array<GoalConstraint>
  "mustNotBreak": Array<GoalConstraint>
  "relationshipMacroDirections"?: Array<MacroDirection>
  "majorMysteryGoals"?: Array<MacroDirection>
  "futureReservations"?: Array<FutureReservation>
  "volumeIntentSeeds"?: Array<VolumeIntentSeed>
  "openingHypotheses": Array<string>
  "globalLengthBudget": { "minWords"?: number | null; "preferredWords": number; "maxWords"?: number | null }
}

export interface VolumeIntentR1 {
  "payloadSchema": unknown
  "volumeFunction": string
  "entryConditions": Array<Condition>
  "mustAchieve": Array<GoalConstraint>
  "mustNotBreak": Array<GoalConstraint>
  "requiredExitConditions": Array<Condition>
  "futureReservationRefs": Array<string>
  "crossVolumeResponsibilityRefs": Array<string>
  "targetChapterRange": { "min": number; "preferred": number; "max": number }
  "targetWordRange": { "min"?: number | null; "preferred": number; "max"?: number | null }
}

export interface VolumePlanR1 {
  "payloadSchema": unknown
  "volumeIntentRef": ArtifactRef
  "routeVersion": number
  "tasks": Array<VolumeTask>
  "routeInvariants": Array<string>
  "requiredExitConditions": Array<Condition>
  "climaxReservation"?: { "taskId": string; "minRemainingCapacity": number } | null
  "capacityBudget": { "minChapters": number; "preferredChapters": number; "maxChapters": number }
}

export interface ContextManifestR1 {
  "payloadSchema": unknown
  "executionMode": "OPENING_BOOTSTRAP" | "LONG_FORM"
  "compiledFromProjectRevision": number
  "compiledFromStateDigest": string
  "goalContractRefs": Array<ArtifactRef>
  "activeVolumeTaskRefs": Array<string>
  "continuityThreadRefs": Array<string>
  "futureReservationRefs": Array<string>
  "narrativeStateSources": Array<SourceRef>
  "selectedEvidence": Array<EvidenceRef>
  "hardSourceCount": number
  "omittedSoftSources": Array<OmittedSource>
  "budget": { "hardTokens": number; "softTokens": number; "totalTokens": number }
}

export interface ChapterPlanR1 {
  "payloadSchema": unknown
  "chapterId": string
  "compiledFromProjectRevision": number
  "compiledFromStateDigest": string
  "contextManifestRef": ArtifactRef
  "chapterGoal": string
  "activeTaskRefs": Array<string>
  "continuityActions": Array<{ "threadRef": string; "disposition": "ADVANCE" | "RESOLVE" | "DEFER_WITH_BRIDGE" | "NO_LONGER_APPLICABLE_CHANGE_REQUEST"; "bridge"?: string | null }>
  "requiredStateOutcomes": Array<Condition>
  "forbiddenTransitions": Array<Condition>
  "knowledgeBoundaries": Array<string>
  "reservationRefs": Array<string>
  "sceneExecution": Array<unknown>
  "horizonPreview": Array<HorizonSlot>
}

export interface ChapterExitStateR1 {
  "payloadSchema": unknown
  "chapterId": string
  "closedAt": string
  "stateDigest": string
  "activeThreadRefs": Array<string>
  "taskStates": Array<TaskProgress>
  "canonEventRefs": Array<string>
  "evidenceRefs": Array<EvidenceRef>
}

export interface VolumeExitStateR1 {
  "payloadSchema": unknown
  "volumeId": string
  "closedAt": string
  "stateDigest": string
  "remainingTaskIds": Array<string>
  "unresolvedThreadRefs": Array<string>
  "nextVolumeEligibility": "ELIGIBLE" | "BLOCKED"
  "handoffEvidenceRefs": Array<EvidenceRef>
}

export type OpaqueId = string

export type Rfc3339Utc = string

export type Sha256 = string

export type ArtifactType = "CreativeBrief" | "MarketSnapshot" | "MarketEvidenceBundle" | "GenreOpportunity" | "MarketStrategyContract" | "ProjectPitch" | "StoryBlueprint" | "NamingSystem" | "EntityNameDesign" | "StoryConstitution" | "ConstitutionSeal" | "BookPlan" | "MainContract" | "EndgameContract" | "BookStateSnapshot" | "BookExitState" | "BookHealthReport" | "BookCompletionReport" | "NarrativeTasteProfile" | "NarrativeSemanticSeal" | "VolumeIntent" | "VolumePlan" | "VolumeContract" | "PlanningStateLedger" | "VolumeExitState" | "VolumeHealthReport" | "ArcPlan" | "ArcContract" | "ArcExitState" | "ChapterPlan" | "SceneContract" | "DramaticAuthoringPlan" | "SceneStrategySet" | "SceneStrategyDecision" | "ContextManifest" | "ChapterCandidate" | "ReviewReport" | "NarrativeQualityReport" | "RevisionPatch" | "GatePolicy" | "GateCertificate" | "FinalChapter" | "ChapterSummary" | "ChapterExitState" | "ProductionReceipt" | "NarrativeObligationSet" | "ForeshadowPlan" | "CharacterArcPlan" | "KnowledgePlan" | "MysteryPlan" | "ReaderDesireSnapshot" | "ChangeRequest" | "ImpactPlan" | "RetconPlan" | "PlatformPolicy" | "OpeningContract" | "SimulatedSigningReceipt" | "PublishedChapterSnapshot" | "PublishedPerformanceSnapshot" | "PromptPackage" | "ModelRunReceipt" | "ExportPackage" | "BackupReceipt" | "IntegrityReport"

export type ArtifactStatus = "CANDIDATE" | "VALIDATED" | "COMMITTED" | "STALE" | "REJECTED" | "SUPERSEDED"

export type SemanticLifecycle = "PROPOSAL" | "APPROVED_PLAN" | "MATERIALIZED" | "CANON"

export type AuthorityLevel = "A" | "B" | "C" | "D"

export type ArtifactScopeLevel = "PROJECT" | "BOOK" | "VOLUME" | "ARC" | "CHAPTER" | "SCENE" | "ENTITY" | "MARKET"

export type CommandContractVersion = string

export type ProjectLifecycle = "ACTIVE" | "ARCHIVED" | "DELETING"

export interface HeadChange {
  "artifactType": ArtifactType
  "scope": ArtifactScope
  "artifactId": OpaqueId
  "version": number
  "payloadHash": Sha256
}

export interface PageRequest {
  "cursor"?: string | null
  "pageSize"?: number
}

export type SharedDefinitions = unknown

export interface ArtifactRef {
  "artifactId": OpaqueId
  "version": number
  "payloadHash": Sha256
}

export interface ArtifactScope {
  "level": ArtifactScopeLevel
  "bookId"?: OpaqueId
  "volumeId"?: OpaqueId
  "arcId"?: OpaqueId
  "chapterId"?: OpaqueId
  "sceneId"?: OpaqueId
  "entityId"?: OpaqueId
  "platformId"?: OpaqueId
}

export interface ArtifactEnvelope {
  "artifactId": OpaqueId
  "projectId": OpaqueId
  "artifactType": ArtifactType
  "scope": ArtifactScope
  "version": number
  "schemaVersion": string
  "artifactStatus": ArtifactStatus
  "semanticLifecycle"?: SemanticLifecycle
  "authorityLevel"?: AuthorityLevel
  "payload": unknown
  "payloadHash": Sha256
  "dependencyRefs": Array<ArtifactRef>
  "provenance": { "createdBy": string; "commandId": OpaqueId; "jobId"?: OpaqueId; "stepId"?: OpaqueId; "modelRunId"?: OpaqueId; "promptRef"?: OpaqueId; "contextManifestRef"?: OpaqueId; "decisionRefs"?: Array<OpaqueId>; "gateCertificateRefs"?: Array<OpaqueId> }
  "createdAt": Rfc3339Utc
  "committedAt"?: Rfc3339Utc
}

export interface CommandEnvelope {
  "commandId": OpaqueId
  "commandType": string
  "projectId"?: OpaqueId
  "expectedRevision"?: number
  "requestedAt": Rfc3339Utc
  "idempotencyKey": OpaqueId
  "payload": {  }
  "contractVersion": CommandContractVersion
}

export interface ProjectCommandReceipt {
  "receiptId": OpaqueId
  "projectId": OpaqueId
  "resultProjectId": OpaqueId
  "commandId": OpaqueId
  "idempotencyKey": OpaqueId
  "commandType": string
  "beforeRevision": number
  "afterRevision": number
  "requestDigest": Sha256
  "committedAt": Rfc3339Utc
  "receiptDigest": Sha256
}

export interface CommitBundle {
  "commitId": OpaqueId
  "projectId": OpaqueId
  "commandId": OpaqueId
  "idempotencyKey": OpaqueId
  "expectedProjectRevision"?: number
  "contractVersion": CommandContractVersion
  "createdAt": Rfc3339Utc
  "artifacts": Array<ArtifactEnvelope>
  "headUpdates"?: Array<HeadChange>
  "canonDelta"?: Array<CanonDelta>
  "obligationDelta"?: Array<ObligationDelta>
  "foreshadowDelta"?: Array<ForeshadowDelta>
  "knowledgeDelta"?: Array<KnowledgeDelta>
  "stateDelta"?: Array<StateDelta>
  "dependencyEdges"?: Array<DependencyEdge>
  "jobUpdates"?: Array<JobUpdate>
  "decisionUpdates"?: Array<DecisionUpdate>
  "readerDesireDelta"?: Array<ReaderDesireDelta>
  "gateCertificates"?: Array<GateCertificate>
  "receiptDraft"?: ReceiptDraft
}

export interface CanonDelta {
  "entityId"?: OpaqueId
  "factId": OpaqueId
  "subjectId"?: OpaqueId
  "predicate"?: string
  "objectJson"?: {  }
  "relationId"?: OpaqueId
  "validFromChapterId"?: OpaqueId
  "validToChapterId"?: OpaqueId
  "authorityLevel"?: AuthorityLevel
  "eventType"?: string
  "payloadJson"?: {  }
}

export interface ObligationDelta {
  "obligationId": OpaqueId
  "scope"?: "BOOK" | "VOLUME" | "ARC" | "CHAPTER"
  "obligationType": string
  "authorityLevel"?: "A" | "B" | "C"
  "lifecycle"?: "PROPOSED" | "APPROVED" | "PLANTED" | "PROGRESSING" | "RESOLVED" | "ABANDONED" | "SUPERSEDED"
  "mustResolveBeforeChapter"?: OpaqueId
  "description"?: string
  "prerequisites"?: Array<OpaqueId>
  "decisionRef"?: OpaqueId
  "payloadJson"?: {  }
}

export interface ForeshadowDelta {
  "foreshadowId": OpaqueId
  "intent": string
  "status"?: "PLANTED" | "REINFORCED" | "RECOVERED" | "ABANDONED" | "STALE"
  "plantChapterId"?: OpaqueId
  "recoverWindowEndChapter"?: OpaqueId
  "forbiddenRevealBeforeChapter"?: OpaqueId
  "secretPayloadRef"?: OpaqueId
  "payloadJson"?: {  }
}

export interface KnowledgeDelta {
  "knowledgeId": OpaqueId
  "knowerId": OpaqueId
  "factId": OpaqueId
  "epistemicState": "KNOWS" | "BELIEVES_TRUE" | "BELIEVES_FALSE" | "SUSPECTS" | "CONCEALS" | "UNKNOWN"
  "visibilityScope"?: string
  "fromChapterId"?: OpaqueId
  "untilChapterId"?: OpaqueId
  "payloadJson"?: {  }
}

export interface StateDelta {
  "stateId": OpaqueId
  "stateType": string
  "valueJson"?: {  }
  "chapterId"?: OpaqueId
  "payloadJson"?: {  }
}

export interface DependencyEdge {
  "edgeId"?: OpaqueId
  "sourceRef": OpaqueId
  "targetRef": OpaqueId
  "dependencyType": "HARD_CONTRACT" | "PLAN_INPUT" | "CANON_FACT" | "STATE_INPUT" | "KNOWLEDGE_INPUT" | "FORESHADOW_INPUT" | "OBLIGATION_INPUT" | "EVIDENCE" | "PRESENTATION" | "SOFT_CONTEXT"
  "scope": string
  "impactClass": "HARD" | "REVIEW_REQUIRED" | "SOFT" | "EVIDENCE"
  "sourceHash": Sha256
  "selector"?: string
  "entityId"?: OpaqueId
  "payloadJson"?: {  }
}

export interface JobUpdate {
  "jobId": OpaqueId
  "status"?: "QUEUED" | "RUNNING" | "PAUSING" | "PAUSED" | "WAITING_MODEL" | "WAITING_DECISION" | "WAITING_BUDGET" | "BLOCKED" | "COMPLETED" | "FAILED" | "CANCELLED"
  "currentStage"?: string
  "attemptNo"?: number
  "repairRound"?: number
  "errorCode"?: string
  "payloadJson"?: {  }
}

export interface DecisionUpdate {
  "decisionId": OpaqueId
  "decisionType": string
  "outcome": string
  "reason"?: string
  "sourceRef"?: OpaqueId
  "payloadJson"?: {  }
}

export interface ReaderDesireDelta {
  "desireId": OpaqueId
  "desireType": string
  "status"?: "OPEN" | "ESCALATED" | "PARTIAL_PAYOFF" | "PAID" | "ABANDONED" | "STALE"
  "expectedPayoffChapter"?: OpaqueId
  "createdChapterId"?: OpaqueId
  "lastTouchedChapterId"?: OpaqueId
  "payloadJson"?: {  }
}

export interface CommitDeltas {
  "canonDelta"?: Array<CanonDelta>
  "obligationDelta"?: Array<ObligationDelta>
  "foreshadowDelta"?: Array<ForeshadowDelta>
  "knowledgeDelta"?: Array<KnowledgeDelta>
  "stateDelta"?: Array<StateDelta>
  "dependencyEdges"?: Array<DependencyEdge>
  "jobUpdates"?: Array<JobUpdate>
  "decisionUpdates"?: Array<DecisionUpdate>
  "readerDesireDelta"?: Array<ReaderDesireDelta>
}

export interface CommitReceipt {
  "receiptId": OpaqueId
  "commitId": OpaqueId
  "projectId": OpaqueId
  "commandId": OpaqueId
  "idempotencyKey": OpaqueId
  "beforeRevision": number
  "afterRevision": number
  "bundleDigest": Sha256
  "committedArtifactRefs": Array<ArtifactRef>
  "headChanges": Array<HeadChange>
  "appliedDeltaCounts": Record<string, number>
  "gateCertificateRefs": Array<OpaqueId>
  "committedAt": Rfc3339Utc
  "receiptDigest": Sha256
}

export interface DomainReceipt {
  "receiptId": OpaqueId
  "projectId": OpaqueId
  "domain": string
  "commandId": OpaqueId
  "commitReceiptId": OpaqueId
  "artifactRefs": Array<ArtifactRef>
  "payload": {  }
  "createdAt": Rfc3339Utc
  "receiptDigest": Sha256
}

export interface GateCertificate {
  "certificateId": OpaqueId
  "projectId": OpaqueId
  "gatePolicyRef": ArtifactRef
  "subjectRef": ArtifactRef
  "evidenceRefs": Array<ArtifactRef>
  "sourceDigest": Sha256
  "passedAt": Rfc3339Utc
  "certificateDigest": Sha256
}

export interface ReceiptDraft {
  "domain": string
  "receiptId"?: OpaqueId
  "artifactRefs"?: Array<ArtifactRef>
  "payload"?: {  }
}

export interface QueryEnvelope {
  "projectId": OpaqueId
  "scope": string
  "pagination": PageRequest
}

export interface ProjectCatalogRecord {
  "projectId": OpaqueId
  "projectPath": string
  "title": string
  "lifecycle": ProjectLifecycle
  "createdAt": Rfc3339Utc
  "updatedAt": Rfc3339Utc
  "lastOpenedAt"?: Rfc3339Utc
  "projectSchemaVersion": string
  "compatibilityState": "COMPATIBLE" | "MIGRATION_REQUIRED" | "RECOVERY"
}

export interface Budget {
  "budgetId": OpaqueId
  "projectId": OpaqueId
  "chapterId": OpaqueId
  "importanceClass": "NORMAL" | "IMPORTANT" | "CRITICAL"
  "modelPolicyRef"?: OpaqueId
  "targetModelRuns": number
  "softMaxModelRuns": number
  "hardMaxModelRuns": number
  "inputTokenBudget": number
  "outputTokenBudget": number
  "repairRunLimit": number
  "wallClockBudgetMs": number
  "costBudgetCents": number
  "strategySearchBudget": number
  "frozenAt": Rfc3339Utc
}

export interface HealthSourceRef {
  "kind": string
  "ref": string
}

export interface HealthFactor {
  "factorCode": "OBLIGATION_OVERDUE" | "FORESHADOW_AGING" | "ROUTE_DRIFT" | "TASK_BLOCKAGE" | "CONTEXT_GROWTH" | "MODEL_COST_DRIFT" | "READER_DESIRE" | "QUALITY_DRIFT"
  "truthClass": "DETERMINISTIC_RUNTIME" | "SOFT_ADVISORY"
  "status": "EVALUATED" | "INSUFFICIENT_DATA" | "SOURCE_UNAVAILABLE"
  "severity": "healthy" | "attention" | "risk"
  "summary": string
  "detail": string
  "sourceRefs": Array<HealthSourceRef>
  "recommendedDestination": string
}

export interface HealthIssue {
  "issueId": string
  "factorCode": "OBLIGATION_OVERDUE" | "FORESHADOW_AGING" | "ROUTE_DRIFT" | "TASK_BLOCKAGE" | "CONTEXT_GROWTH" | "MODEL_COST_DRIFT" | "READER_DESIRE" | "QUALITY_DRIFT"
  "severity": "healthy" | "attention" | "risk"
  "title": string
  "detail": string
  "location": string
  "dueChapter"?: number | null
  "recommendedDestination": string
  "sourceRefs": Array<HealthSourceRef>
}

export interface HealthIssuesPage {
  "items": Array<HealthIssue>
  "nextCursor": string | null
  "total": number
}

export interface HealthCoverage {
  "totalFactors": number
  "evaluatedFactors": number
  "insufficientFactors": number
  "unavailableFactors": number
}

export interface HealthCounters {
  "openObligations": number
  "overdueObligations": number
  "openForeshadows": number
  "overdueForeshadows": number
  "blockedRequiredTasks": number
  "blockedOptionalTasks": number
  "overdueReaderDesires": number
  "qualitySamples": number
  "comparableContextRuns": number
  "comparableCostRuns": number
}

export interface HealthProvenance {
  "queryMode": "ON_DEMAND"
  "cacheMode": "PROJECTION_ONLY" | "NO_CACHE"
  "sourceRevision": number
  "bounded": boolean
  "fullManuscriptScan": boolean
  "factorSources": Record<string, Array<HealthSourceRef>>
}

export interface LongFormHealthProjectionR1 {
  "payloadSchema": string
  "projectId": string
  "projectRevision": number
  "checkedAt": string
  "score": number
  "level": "healthy" | "attention" | "risk"
  "coverage": { "totalFactors": number; "evaluatedFactors": number; "insufficientFactors": number; "unavailableFactors": number }
  "factors": Array<HealthFactor>
  "issues": HealthIssuesPage
  "counters": HealthCounters
  "provenance": HealthProvenance
}
