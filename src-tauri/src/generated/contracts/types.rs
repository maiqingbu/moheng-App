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
    #[serde(rename = "ContextManifest")]
    ContextManifest,
    #[serde(rename = "ChapterCandidate")]
    ChapterCandidate,
    #[serde(rename = "ReviewReport")]
    ReviewReport,
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
    pub canon_delta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obligation_delta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreshadow_delta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_delta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_delta: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_edges: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_updates: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_updates: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_certificates: Option<Vec<serde_json::Value>>,
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
