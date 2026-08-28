// DO NOT EDIT - generated from contracts/schemas.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalConstraint {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroDirection {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FutureReservation {
    pub reservation_id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_for_volume_no: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_for_volume_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_volume_no: Option<i64>,
    pub prerequisites: Vec<String>,
    pub may_foreshadow: bool,
    pub may_reveal: bool,
    pub authority_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeIntentSeed {
    pub seed_id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Condition {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    #[serde(rename = "NOT_STARTED")]
    NotStarted,
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "SATISFIED")]
    Satisfied,
    #[serde(rename = "BLOCKED")]
    Blocked,
    #[serde(rename = "INVALIDATED")]
    Invalidated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeTask {
    pub task_id: String,
    pub class: String,
    pub objective: String,
    pub depends_on_task_ids: Vec<String>,
    pub continuity_thread_refs: Vec<String>,
    pub reservation_refs: Vec<String>,
    pub completion_predicate: Condition,
    pub estimated_chapter_cost: VolumeTaskEstimatedChapterCost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_pressure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_pressure: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeTaskEstimatedChapterCost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    pub preferred: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRef {
    pub kind: String,
    pub ref_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRef {
    pub artifact_id: String,
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmittedSource {
    pub source_ref: SourceRef,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HorizonSlot {
    pub slot_no: i64,
    pub objective: String,
    pub tentative: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContinuityThreadProjection {
    pub thread_id: String,
    pub status: String,
    pub thread_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub authority_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeProgressProjection {
    pub volume_id: String,
    pub active_task_ids: Vec<String>,
    pub blocked_task_ids: Vec<String>,
    pub remaining_chapter_capacity: i64,
    pub planning_pressure: VolumeProgressProjectionPlanningPressure,
    pub generated_revision: i64,
}

pub type VolumeProgressProjectionPlanningPressure =
    std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProgress {
    pub task_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_advanced_chapter_id: Option<String>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub remaining_conditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<TaskProgressMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProgressMetric {
    pub current: i64,
    pub target: i64,
}

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
pub struct HeadChange {
    pub artifact_type: ArtifactType,
    pub scope: ArtifactScope,
    pub artifact_id: OpaqueId,
    pub version: i64,
    pub payload_hash: Sha256,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderDesireDelta {
    pub desire_id: OpaqueId,
    pub desire_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_payoff_chapter: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_touched_chapter_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<ReaderDesireDeltaPayloadJson>,
}

pub type ReaderDesireDeltaPayloadJson = std::collections::BTreeMap<String, serde_json::Value>;

pub type NarrativeDefinitions = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookPlanR1 {
    pub payload_schema: serde_json::Value,
    pub planning_maturity: String,
    pub story_promise: String,
    pub protagonist_macro_direction: String,
    pub ending_boundary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must_achieve: Option<Vec<GoalConstraint>>,
    pub must_not_break: Vec<GoalConstraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_macro_directions: Option<Vec<MacroDirection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_mystery_goals: Option<Vec<MacroDirection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub future_reservations: Option<Vec<FutureReservation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_intent_seeds: Option<Vec<VolumeIntentSeed>>,
    pub opening_hypotheses: Vec<String>,
    pub global_length_budget: BookPlanR1GlobalLengthBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookPlanR1GlobalLengthBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_words: Option<i64>,
    pub preferred_words: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_words: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeIntentR1 {
    pub payload_schema: serde_json::Value,
    pub volume_function: String,
    pub entry_conditions: Vec<Condition>,
    pub must_achieve: Vec<GoalConstraint>,
    pub must_not_break: Vec<GoalConstraint>,
    pub required_exit_conditions: Vec<Condition>,
    pub future_reservation_refs: Vec<String>,
    pub cross_volume_responsibility_refs: Vec<String>,
    pub target_chapter_range: VolumeIntentR1TargetChapterRange,
    pub target_word_range: VolumeIntentR1TargetWordRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeIntentR1TargetChapterRange {
    pub min: i64,
    pub preferred: i64,
    pub max: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeIntentR1TargetWordRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    pub preferred: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumePlanR1 {
    pub payload_schema: serde_json::Value,
    pub volume_intent_ref: ArtifactRef,
    pub route_version: i64,
    pub tasks: Vec<VolumeTask>,
    pub route_invariants: Vec<String>,
    pub required_exit_conditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climax_reservation: Option<VolumePlanR1ClimaxReservation>,
    pub capacity_budget: VolumePlanR1CapacityBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumePlanR1ClimaxReservation {
    pub task_id: String,
    pub min_remaining_capacity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumePlanR1CapacityBudget {
    pub min_chapters: i64,
    pub preferred_chapters: i64,
    pub max_chapters: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextManifestR1 {
    pub payload_schema: serde_json::Value,
    pub execution_mode: String,
    pub compiled_from_project_revision: i64,
    pub compiled_from_state_digest: String,
    pub goal_contract_refs: Vec<ArtifactRef>,
    pub active_volume_task_refs: Vec<String>,
    pub continuity_thread_refs: Vec<String>,
    pub future_reservation_refs: Vec<String>,
    pub narrative_state_sources: Vec<SourceRef>,
    pub selected_evidence: Vec<EvidenceRef>,
    pub hard_source_count: i64,
    pub omitted_soft_sources: Vec<OmittedSource>,
    pub budget: ContextManifestR1Budget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextManifestR1Budget {
    pub hard_tokens: i64,
    pub soft_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChapterPlanR1 {
    pub payload_schema: serde_json::Value,
    pub chapter_id: String,
    pub compiled_from_project_revision: i64,
    pub compiled_from_state_digest: String,
    pub context_manifest_ref: ArtifactRef,
    pub chapter_goal: String,
    pub active_task_refs: Vec<String>,
    pub continuity_actions: Vec<ChapterPlanR1ContinuityActionsItem>,
    pub required_state_outcomes: Vec<Condition>,
    pub forbidden_transitions: Vec<Condition>,
    pub knowledge_boundaries: Vec<String>,
    pub reservation_refs: Vec<String>,
    pub scene_execution: Vec<serde_json::Value>,
    pub horizon_preview: Vec<HorizonSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChapterPlanR1ContinuityActionsItem {
    pub thread_ref: String,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChapterExitStateR1 {
    pub payload_schema: serde_json::Value,
    pub chapter_id: String,
    pub closed_at: String,
    pub state_digest: String,
    pub active_thread_refs: Vec<String>,
    pub task_states: Vec<TaskProgress>,
    pub canon_event_refs: Vec<String>,
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolumeExitStateR1 {
    pub payload_schema: serde_json::Value,
    pub volume_id: String,
    pub closed_at: String,
    pub state_digest: String,
    pub remaining_task_ids: Vec<String>,
    pub unresolved_thread_refs: Vec<String>,
    pub next_volume_eligibility: String,
    pub handoff_evidence_refs: Vec<EvidenceRef>,
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
    pub head_updates: Option<Vec<HeadChange>>,
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
    pub reader_desire_delta: Option<Vec<ReaderDesireDelta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_certificates: Option<Vec<GateCertificate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_draft: Option<ReceiptDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitDeltas {
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
    pub reader_desire_delta: Option<Vec<ReaderDesireDelta>>,
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
    pub head_changes: Vec<HeadChange>,
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
    pub artifact_refs: Vec<ArtifactRef>,
    pub payload: DomainReceiptPayload,
    pub created_at: Rfc3339Utc,
    pub receipt_digest: Sha256,
}

pub type DomainReceiptPayload = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateCertificate {
    pub certificate_id: OpaqueId,
    pub project_id: OpaqueId,
    pub gate_policy_ref: ArtifactRef,
    pub subject_ref: ArtifactRef,
    pub evidence_refs: Vec<ArtifactRef>,
    pub source_digest: Sha256,
    pub passed_at: Rfc3339Utc,
    pub certificate_digest: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptDraft {
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ReceiptDraftPayload>,
}

pub type ReceiptDraftPayload = std::collections::BTreeMap<String, serde_json::Value>;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    pub budget_id: OpaqueId,
    pub project_id: OpaqueId,
    pub chapter_id: OpaqueId,
    pub importance_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy_ref: Option<OpaqueId>,
    pub target_model_runs: i64,
    pub soft_max_model_runs: i64,
    pub hard_max_model_runs: i64,
    pub input_token_budget: i64,
    pub output_token_budget: i64,
    pub repair_run_limit: i64,
    pub wall_clock_budget_ms: i64,
    pub cost_budget_cents: i64,
    pub strategy_search_budget: i64,
    pub frozen_at: Rfc3339Utc,
}
