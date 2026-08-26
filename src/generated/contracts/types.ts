/* DO NOT EDIT - generated from contracts/schemas. */

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
