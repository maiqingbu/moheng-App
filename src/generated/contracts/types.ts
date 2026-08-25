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
  "headUpdates"?: Array<{ "artifactType": ArtifactType; "scope": ArtifactScope; "artifactId": OpaqueId; "version": number; "payloadHash": Sha256 }>
  "canonDelta"?: unknown
  "obligationDelta"?: unknown
  "foreshadowDelta"?: unknown
  "knowledgeDelta"?: unknown
  "stateDelta"?: unknown
  "dependencyEdges"?: Array<unknown>
  "jobUpdates"?: Array<unknown>
  "decisionUpdates"?: Array<unknown>
  "gateCertificates"?: Array<unknown>
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
  "headChanges": Array<unknown>
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
  "createdAt": Rfc3339Utc
  "receiptDigest": Sha256
}

export interface GateCertificate {
  "certificateId": OpaqueId
  "projectId": OpaqueId
  "gatePolicyRef": ArtifactRef
  "evidenceRefs": Array<ArtifactRef>
  "sourceDigest": Sha256
  "passedAt": Rfc3339Utc
  "certificateDigest": Sha256
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
