// DO NOT EDIT - generated from contracts/schemas.

use serde::{Deserialize, Serialize};

pub type OpaqueId = String;

pub type Rfc3339Utc = String;

pub type Sha256 = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArtifactType {
    #[serde(rename = "CreativeBrief")]
    CreativeBrief,
    #[serde(rename = "MarketSnapshot")]
    MarketSnapshot,
    #[serde(rename = "MarketEvidenceBundle")]
    MarketEvidenceBundle,
    #[serde(rename = "GenreOpportunity")]
    GenreOpportunity,
    #[serde(rename = "MarketStrategyContract")]
    MarketStrategyContract,
    #[serde(rename = "ProjectPitch")]
    ProjectPitch,
    #[serde(rename = "StoryBlueprint")]
    StoryBlueprint,
    #[serde(rename = "NamingSystem")]
    NamingSystem,
    #[serde(rename = "EntityNameDesign")]
    EntityNameDesign,
    #[serde(rename = "StoryConstitution")]
    StoryConstitution,
    #[serde(rename = "ConstitutionSeal")]
    ConstitutionSeal,
    #[serde(rename = "BookPlan")]
    BookPlan,
    #[serde(rename = "MainContract")]
    MainContract,
    #[serde(rename = "EndgameContract")]
    EndgameContract,
    #[serde(rename = "BookStateSnapshot")]
    BookStateSnapshot,
    #[serde(rename = "BookExitState")]
    BookExitState,
    #[serde(rename = "BookHealthReport")]
    BookHealthReport,
    #[serde(rename = "BookCompletionReport")]
    BookCompletionReport,
    #[serde(rename = "NarrativeTasteProfile")]
    NarrativeTasteProfile,
    #[serde(rename = "NarrativeSemanticSeal")]
    NarrativeSemanticSeal,
    #[serde(rename = "VolumeIntent")]
    VolumeIntent,
    #[serde(rename = "VolumePlan")]
    VolumePlan,
    #[serde(rename = "VolumeContract")]
    VolumeContract,
    #[serde(rename = "PlanningStateLedger")]
    PlanningStateLedger,
    #[serde(rename = "VolumeExitState")]
    VolumeExitState,
    #[serde(rename = "VolumeHealthReport")]
    VolumeHealthReport,
    #[serde(rename = "ArcPlan")]
    ArcPlan,
    #[serde(rename = "ArcContract")]
    ArcContract,
    #[serde(rename = "ArcExitState")]
    ArcExitState,
    #[serde(rename = "ChapterPlan")]
    ChapterPlan,
    #[serde(rename = "SceneContract")]
    SceneContract,
    #[serde(rename = "DramaticAuthoringPlan")]
    DramaticAuthoringPlan,
    #[serde(rename = "SceneStrategySet")]
    SceneStrategySet,
    #[serde(rename = "SceneStrategyDecision")]
    SceneStrategyDecision,
    #[serde(rename = "ContextManifest")]
    ContextManifest,
    #[serde(rename = "ChapterCandidate")]
    ChapterCandidate,
    #[serde(rename = "ReviewReport")]
    ReviewReport,
    #[serde(rename = "NarrativeQualityReport")]
    NarrativeQualityReport,
    #[serde(rename = "RevisionPatch")]
    RevisionPatch,
    #[serde(rename = "GatePolicy")]
    GatePolicy,
    #[serde(rename = "GateCertificate")]
    GateCertificate,
    #[serde(rename = "FinalChapter")]
    FinalChapter,
    #[serde(rename = "ChapterSummary")]
    ChapterSummary,
    #[serde(rename = "ChapterExitState")]
    ChapterExitState,
    #[serde(rename = "ProductionReceipt")]
    ProductionReceipt,
    #[serde(rename = "NarrativeObligationSet")]
    NarrativeObligationSet,
    #[serde(rename = "ForeshadowPlan")]
    ForeshadowPlan,
    #[serde(rename = "CharacterArcPlan")]
    CharacterArcPlan,
    #[serde(rename = "KnowledgePlan")]
    KnowledgePlan,
    #[serde(rename = "MysteryPlan")]
    MysteryPlan,
    #[serde(rename = "ReaderDesireSnapshot")]
    ReaderDesireSnapshot,
    #[serde(rename = "ChangeRequest")]
    ChangeRequest,
    #[serde(rename = "ImpactPlan")]
    ImpactPlan,
    #[serde(rename = "RetconPlan")]
    RetconPlan,
    #[serde(rename = "PlatformPolicy")]
    PlatformPolicy,
    #[serde(rename = "OpeningContract")]
    OpeningContract,
    #[serde(rename = "SimulatedSigningReceipt")]
    SimulatedSigningReceipt,
    #[serde(rename = "PublishedChapterSnapshot")]
    PublishedChapterSnapshot,
    #[serde(rename = "PublishedPerformanceSnapshot")]
    PublishedPerformanceSnapshot,
    #[serde(rename = "PromptPackage")]
    PromptPackage,
    #[serde(rename = "ModelRunReceipt")]
    ModelRunReceipt,
    #[serde(rename = "ExportPackage")]
    ExportPackage,
    #[serde(rename = "BackupReceipt")]
    BackupReceipt,
    #[serde(rename = "IntegrityReport")]
    IntegrityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArtifactStatus {
    #[serde(rename = "CANDIDATE")]
    Candidate,
    #[serde(rename = "VALIDATED")]
    Validated,
    #[serde(rename = "COMMITTED")]
    Committed,
    #[serde(rename = "STALE")]
    Stale,
    #[serde(rename = "REJECTED")]
    Rejected,
    #[serde(rename = "SUPERSEDED")]
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SemanticLifecycle {
    #[serde(rename = "PROPOSAL")]
    Proposal,
    #[serde(rename = "APPROVED_PLAN")]
    ApprovedPlan,
    #[serde(rename = "MATERIALIZED")]
    Materialized,
    #[serde(rename = "CANON")]
    Canon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthorityLevel {
    #[serde(rename = "A")]
    A,
    #[serde(rename = "B")]
    B,
    #[serde(rename = "C")]
    C,
    #[serde(rename = "D")]
    D,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArtifactScopeLevel {
    #[serde(rename = "PROJECT")]
    Project,
    #[serde(rename = "BOOK")]
    Book,
    #[serde(rename = "VOLUME")]
    Volume,
    #[serde(rename = "ARC")]
    Arc,
    #[serde(rename = "CHAPTER")]
    Chapter,
    #[serde(rename = "SCENE")]
    Scene,
    #[serde(rename = "ENTITY")]
    Entity,
    #[serde(rename = "MARKET")]
    Market,
}

pub type CommandContractVersion = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectLifecycle {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "ARCHIVED")]
    Archived,
    #[serde(rename = "DELETING")]
    Deleting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<OpaqueId>,
    pub fact_id: OpaqueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_json: Option<CanonDeltaObjectJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_level: Option<AuthorityLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<CanonDeltaPayloadJson>,
}

pub type CanonDeltaObjectJson = std::collections::BTreeMap<String, serde_json::Value>;

pub type CanonDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObligationDelta {
    pub obligation_id: OpaqueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub obligation_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must_resolve_before_chapter: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prerequisites: Option<Vec<OpaqueId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_ref: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<ObligationDeltaPayloadJson>,
}

pub type ObligationDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForeshadowDelta {
    pub foreshadow_id: OpaqueId,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plant_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recover_window_end_chapter: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forbidden_reveal_before_chapter: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_payload_ref: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<ForeshadowDeltaPayloadJson>,
}

pub type ForeshadowDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDelta {
    pub knowledge_id: OpaqueId,
    pub knower_id: OpaqueId,
    pub fact_id: OpaqueId,
    pub epistemic_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<KnowledgeDeltaPayloadJson>,
}

pub type KnowledgeDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateDelta {
    pub state_id: OpaqueId,
    pub state_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_json: Option<StateDeltaValueJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<StateDeltaPayloadJson>,
}

pub type StateDeltaValueJson = std::collections::BTreeMap<String, serde_json::Value>;

pub type StateDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyEdge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<OpaqueId>,
    pub source_ref: OpaqueId,
    pub target_ref: OpaqueId,
    pub dependency_type: String,
    pub scope: String,
    pub impact_class: String,
    pub source_hash: Sha256,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<DependencyEdgePayloadJson>,
}

pub type DependencyEdgePayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobUpdate {
    pub job_id: OpaqueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_no: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_round: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<JobUpdatePayloadJson>,
}

pub type JobUpdatePayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionUpdate {
    pub decision_id: OpaqueId,
    pub decision_type: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<DecisionUpdatePayloadJson>,
}

pub type DecisionUpdatePayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

pub type SharedDefinitions = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRef {
    pub artifact_id: OpaqueId,
    pub version: i64,
    pub payload_hash: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactScope {
    pub level: ArtifactScopeLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<OpaqueId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactEnvelope {
    pub artifact_id: OpaqueId,
    pub project_id: OpaqueId,
    pub artifact_type: ArtifactType,
    pub scope: ArtifactScope,
    pub version: i64,
    pub schema_version: String,
    pub artifact_status: ArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_lifecycle: Option<SemanticLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_level: Option<AuthorityLevel>,
    pub payload: serde_json::Value,
    pub payload_hash: Sha256,
    pub dependency_refs: Vec<ArtifactRef>,
    pub provenance: ArtifactEnvelopeProvenance,
    pub created_at: Rfc3339Utc,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<Rfc3339Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactEnvelopeProvenance {
    pub created_by: String,
    pub command_id: OpaqueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_run_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_ref: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_manifest_ref: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_refs: Option<Vec<OpaqueId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_certificate_refs: Option<Vec<OpaqueId>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandEnvelope {
    pub command_id: OpaqueId,
    pub command_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    pub requested_at: Rfc3339Utc,
    pub idempotency_key: OpaqueId,
    pub payload: CommandEnvelopePayload,
    pub contract_version: CommandContractVersion,
}

pub type CommandEnvelopePayload = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCommandReceipt {
    pub receipt_id: OpaqueId,
    pub project_id: OpaqueId,
    pub result_project_id: OpaqueId,
    pub command_id: OpaqueId,
    pub idempotency_key: OpaqueId,
    pub command_type: String,
    pub before_revision: i64,
    pub after_revision: i64,
    pub request_digest: Sha256,
    pub committed_at: Rfc3339Utc,
    pub receipt_digest: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitBundle {
    pub commit_id: OpaqueId,
    pub project_id: OpaqueId,
    pub command_id: OpaqueId,
    pub idempotency_key: OpaqueId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_project_revision: Option<i64>,
    pub contract_version: CommandContractVersion,
    pub created_at: Rfc3339Utc,
    pub artifacts: Vec<ArtifactEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_updates: Option<Vec<CommitBundleHeadUpdatesItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canon_delta: Option<Vec<CanonDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obligation_delta: Option<Vec<ObligationDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreshadow_delta: Option<Vec<ForeshadowDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_delta: Option<Vec<KnowledgeDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_delta: Option<Vec<StateDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_edges: Option<Vec<DependencyEdge>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_updates: Option<Vec<JobUpdate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_updates: Option<Vec<DecisionUpdate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_certificates: Option<Vec<GateCertificate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitBundleHeadUpdatesItem {
    pub artifact_type: ArtifactType,
    pub scope: ArtifactScope,
    pub artifact_id: OpaqueId,
    pub version: i64,
    pub payload_hash: Sha256,
}

pub type CommitDeltas = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitReceipt {
    pub receipt_id: OpaqueId,
    pub commit_id: OpaqueId,
    pub project_id: OpaqueId,
    pub command_id: OpaqueId,
    pub idempotency_key: OpaqueId,
    pub before_revision: i64,
    pub after_revision: i64,
    pub bundle_digest: Sha256,
    pub committed_artifact_refs: Vec<ArtifactRef>,
    pub head_changes: Vec<serde_json::Value>,
    pub applied_delta_counts: CommitReceiptAppliedDeltaCounts,
    pub gate_certificate_refs: Vec<OpaqueId>,
    pub committed_at: Rfc3339Utc,
    pub receipt_digest: Sha256,
}

pub type CommitReceiptAppliedDeltaCounts = std::collections::BTreeMap<String, i64>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DomainReceipt {
    pub receipt_id: OpaqueId,
    pub project_id: OpaqueId,
    pub domain: String,
    pub command_id: OpaqueId,
    pub commit_receipt_id: OpaqueId,
    pub created_at: Rfc3339Utc,
    pub receipt_digest: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateCertificate {
    pub certificate_id: OpaqueId,
    pub project_id: OpaqueId,
    pub gate_policy_ref: ArtifactRef,
    pub evidence_refs: Vec<ArtifactRef>,
    pub source_digest: Sha256,
    pub passed_at: Rfc3339Utc,
    pub certificate_digest: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryEnvelope {
    pub project_id: OpaqueId,
    pub scope: String,
    pub pagination: PageRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCatalogRecord {
    pub project_id: OpaqueId,
    pub project_path: String,
    pub title: String,
    pub lifecycle: ProjectLifecycle,
    pub created_at: Rfc3339Utc,
    pub updated_at: Rfc3339Utc,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<Rfc3339Utc>,
    pub project_schema_version: String,
    pub compatibility_state: String,
}
