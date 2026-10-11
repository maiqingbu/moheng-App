use crate::commit::CommitService;
use crate::generated::types::{
    ArtifactEnvelope, ArtifactRef, CommitBundle, CommitReceipt, ContextManifestR1,
    ContextManifestR1Budget, EvidenceRef, OmittedSource, SourceRef, VolumeProgressProjection,
};
use crate::hash::sha256_canonical;
use crate::repositories::narrative::{
    CanonEventRecord, CanonFactRecord, EvidenceRecord, ForeshadowRecord, KnowledgeRecord,
    MysteryRecord, ObligationRecord, RelationRecord, TaskStateRecord,
};
use libsql_rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextHardSource {
    Canon,
    Continuity,
    Contract,
    Evidence,
    Knowledge,
    Reservation,
    Task,
    Unknown,
}

impl ContextHardSource {
    pub fn code(self) -> &'static str {
        match self {
            Self::Canon => "CANON",
            Self::Continuity => "CONTINUITY",
            Self::Contract => "CONTRACT",
            Self::Evidence => "EVIDENCE",
            Self::Knowledge => "KNOWLEDGE",
            Self::Reservation => "RESERVATION",
            Self::Task => "TASK",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBounds {
    pub project_id: String,
    pub entity_ids: Vec<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentNarrativeStateView {
    pub bounds: RuntimeBounds,
    pub facts: Vec<CanonFactRecord>,
    pub relations: Vec<RelationRecord>,
    pub knowledge: Vec<KnowledgeRecord>,
    pub events: Vec<CanonEventRecord>,
    pub active_responsibilities: Vec<ObligationRecord>,
    pub world_time: Option<String>,
    pub source_refs: Vec<SourceRef>,
    pub truncated: bool,
    #[serde(skip)]
    pub truncated_sources: BTreeSet<ContextHardSource>,
}

impl CurrentNarrativeStateView {
    pub fn from_records(
        bounds: RuntimeBounds,
        facts: Vec<CanonFactRecord>,
        relations: Vec<RelationRecord>,
        knowledge: Vec<KnowledgeRecord>,
        events: Vec<CanonEventRecord>,
        active_responsibilities: Vec<ObligationRecord>,
        truncated: bool,
    ) -> Self {
        let world_time = events
            .iter()
            .filter_map(|event| event.story_time.clone())
            .max();
        let mut source_refs = BTreeSet::new();
        for fact in &facts {
            source_refs.insert(fact.fact_id.clone());
        }
        for relation in &relations {
            source_refs.insert(relation.relation_id.clone());
        }
        for knowledge in &knowledge {
            source_refs.insert(knowledge.knowledge_id.clone());
        }
        for event in &events {
            source_refs.insert(event.event_id.clone());
        }
        for obligation in &active_responsibilities {
            source_refs.insert(obligation.obligation_id.clone());
        }
        let truncated_sources = if truncated {
            [ContextHardSource::Unknown].into_iter().collect()
        } else {
            BTreeSet::new()
        };
        Self {
            bounds,
            facts,
            relations,
            knowledge,
            events,
            active_responsibilities,
            world_time,
            source_refs: source_refs
                .into_iter()
                .map(|ref_value| SourceRef {
                    kind: "authority".into(),
                    ref_: ref_value,
                })
                .collect(),
            truncated,
            truncated_sources,
        }
    }

    pub fn mark_truncated(&mut self, source: ContextHardSource) {
        self.truncated = true;
        self.truncated_sources.insert(source);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeContext {
    pub present_entities: BTreeSet<String>,
    pub acquired_facts: BTreeSet<String>,
    pub satisfied_tasks: BTreeSet<String>,
    pub meetings: BTreeSet<(String, String)>,
    pub current_chapter: i64,
    pub current_volume: Option<String>,
}

pub fn evaluate_resume_condition(
    condition: &Value,
    context: &ResumeContext,
) -> Result<bool, String> {
    let object = condition
        .as_object()
        .ok_or_else(|| "resume condition must be an object".to_string())?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
        .ok_or_else(|| "resume condition requires kind".to_string())?;
    let value = object
        .get("value")
        .ok_or_else(|| "resume condition requires value".to_string())?;
    match kind {
        "NEXT_MEETING" => {
            let value = value
                .as_object()
                .ok_or_else(|| "NEXT_MEETING value must be an object".to_string())?;
            let a = value
                .get("entityA")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "NEXT_MEETING requires entityA".to_string())?;
            let b = value
                .get("entityB")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "NEXT_MEETING requires entityB".to_string())?;
            Ok(context.meetings.contains(&(a.to_string(), b.to_string()))
                || context.meetings.contains(&(b.to_string(), a.to_string())))
        }
        "ENTITY_PRESENT" => Ok(context.present_entities.contains(
            value
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("ENTITY_PRESENT value must be a non-empty id")?,
        )),
        "KNOWLEDGE_ACQUIRED" => Ok(context.acquired_facts.contains(
            value
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("KNOWLEDGE_ACQUIRED value must be a non-empty fact id")?,
        )),
        "TASK_SATISFIED" => Ok(context.satisfied_tasks.contains(
            value
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("TASK_SATISFIED value must be a non-empty task id")?,
        )),
        "AFTER_CHAPTER" => {
            let chapter = value
                .as_i64()
                .filter(|chapter| *chapter >= 0)
                .ok_or_else(|| "AFTER_CHAPTER value must be a non-negative integer".to_string())?;
            Ok(context.current_chapter >= chapter)
        }
        "IN_VOLUME" => Ok(context.current_volume.as_deref()
            == Some(
                value
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or("IN_VOLUME value must be a non-empty volume id")?,
            )),
        _ => Err(format!("unknown resume condition kind: {kind}")),
    }
}

pub fn validate_resume_condition(condition: &Value) -> Result<(), String> {
    evaluate_resume_condition(condition, &ResumeContext::default()).map(|_| ())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityThreadView {
    pub thread_id: String,
    pub thread_class: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub fulfillment_object: Option<String>,
    pub logical_status: String,
    pub source_event_ref: Option<String>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub participant_entities: Vec<String>,
    pub resume_condition: Value,
    pub authority_level: String,
    pub current_disposition: String,
    pub source_refs: Vec<SourceRef>,
}

fn evidence_ref(record: &EvidenceRecord) -> EvidenceRef {
    EvidenceRef {
        artifact_id: record.source_artifact_id.clone(),
        version: record.version,
        range_start: record.range_start,
        range_end: record.range_end,
        quote: record.quote.clone(),
    }
}

pub fn build_continuity_threads(
    obligations: &[ObligationRecord],
    foreshadows: &[ForeshadowRecord],
    mysteries: &[MysteryRecord],
    evidence: &[EvidenceRecord],
    resume_context: &ResumeContext,
) -> Result<Vec<ContinuityThreadView>, String> {
    let mut result = Vec::new();
    for obligation in obligations {
        let logical_status = match obligation.lifecycle.as_str() {
            "RESOLVED" | "RECOVERED" => "RESOLVED",
            "ABANDONED" => "ABANDONED",
            "APPROVED" | "PLANTED" | "PROGRESSING" => {
                if evaluate_resume_condition(&obligation.resume_condition, resume_context)? {
                    "OPEN"
                } else {
                    "SUSPENDED"
                }
            }
            _ => {
                return Err(format!(
                    "unknown obligation lifecycle: {}",
                    obligation.lifecycle
                ))
            }
        };
        let mut evidence_refs = obligation.evidence_refs.clone();
        for item in evidence.iter().filter(|item| {
            item.fact_id.as_deref() == Some(obligation.obligation_id.as_str())
                || (item.event_id.is_some() && item.event_id == obligation.source_event_id)
        }) {
            let reference = evidence_ref(item);
            if !evidence_refs.contains(&reference) {
                evidence_refs.push(reference);
            }
        }
        result.push(ContinuityThreadView {
            thread_id: format!("thread:{}", obligation.obligation_id),
            thread_class: obligation.obligation_type.clone(),
            description: obligation.description.clone(),
            fulfillment_object: obligation.fulfillment_object.clone(),
            logical_status: logical_status.into(),
            source_event_ref: obligation
                .source_event_id
                .clone()
                .map(|id| format!("event:{id}")),
            evidence_refs,
            participant_entities: obligation.participant_entities.clone(),
            resume_condition: obligation.resume_condition.clone(),
            authority_level: obligation.authority_level.clone(),
            current_disposition: obligation.lifecycle.clone(),
            source_refs: vec![SourceRef {
                kind: "obligation".into(),
                ref_: obligation.obligation_id.clone(),
            }],
        });
    }
    for foreshadow in foreshadows {
        let logical_status = match foreshadow.status.as_str() {
            "RESOLVED" | "RECOVERED" => "RESOLVED",
            "ABANDONED" => "ABANDONED",
            "PLANTED" | "REINFORCED" | "STALE" => "OPEN",
            other => return Err(format!("unknown foreshadow status: {other}")),
        };
        result.push(ContinuityThreadView { thread_id: format!("thread:{}", foreshadow.foreshadow_id), thread_class: "FORESHADOW".into(), description: foreshadow.intent.clone(), fulfillment_object: foreshadow.secret_payload_ref.clone(), logical_status: logical_status.into(), source_event_ref: None, evidence_refs: Vec::new(), participant_entities: Vec::new(), resume_condition: json!({"kind":"ENTITY_PRESENT","value":foreshadow.secret_payload_ref}), authority_level: "C".into(), current_disposition: foreshadow.status.clone(), source_refs: vec![SourceRef { kind: "foreshadow".into(), ref_: foreshadow.foreshadow_id.clone() }] });
    }
    for mystery in mysteries {
        let logical_status = match mystery.status.as_str() {
            "RESOLVED" | "RECOVERED" => "RESOLVED",
            "ABANDONED" => "ABANDONED",
            "OPEN" | "PROGRESSING" => "OPEN",
            other => return Err(format!("unknown mystery status: {other}")),
        };
        result.push(ContinuityThreadView {
            thread_id: format!("thread:{}", mystery.mystery_id),
            thread_class: mystery.mystery_type.clone(),
            description: mystery.mystery_type.clone(),
            fulfillment_object: mystery.core_fact_id.clone(),
            logical_status: logical_status.into(),
            source_event_ref: None,
            evidence_refs: Vec::new(),
            participant_entities: Vec::new(),
            resume_condition: Value::Null,
            authority_level: "C".into(),
            current_disposition: mystery.status.clone(),
            source_refs: vec![SourceRef {
                kind: "mystery".into(),
                ref_: mystery.mystery_id.clone(),
            }],
        });
    }
    result.sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VolumeProgressInput {
    pub volume_id: String,
    pub remaining_chapter_capacity: i64,
    pub tasks: Vec<TaskStateRecord>,
    pub generated_revision: i64,
}

pub fn build_volume_progress(input: VolumeProgressInput) -> VolumeProgressProjection {
    let mut active = Vec::new();
    let mut blocked = Vec::new();
    for task in &input.tasks {
        match task.value.get("status").and_then(Value::as_str) {
            Some("ACTIVE") => active.push(task.state_id.clone()),
            Some("BLOCKED") => blocked.push(task.state_id.clone()),
            _ => {}
        }
    }
    active.sort();
    blocked.sort();
    let mut pressure = BTreeMap::new();
    pressure.insert("activeCount".into(), json!(active.len()));
    pressure.insert("blockedCount".into(), json!(blocked.len()));
    pressure.insert("exitGapCount".into(), json!(blocked.len()));
    VolumeProgressProjection {
        volume_id: input.volume_id,
        active_task_ids: active,
        blocked_task_ids: blocked,
        remaining_chapter_capacity: input.remaining_chapter_capacity.max(0),
        planning_pressure: pressure,
        generated_revision: input.generated_revision,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractField {
    pub field_path: String,
    pub source_ref: ArtifactRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_source_refs: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_field_paths: Vec<String>,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPlanTask {
    pub task_ref: String,
    pub contract: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompilerInput {
    pub execution_mode: String,
    pub project_revision: i64,
    pub goal_contract_refs: Vec<ArtifactRef>,
    pub goal_contract_fields: Vec<GoalContractField>,
    pub state: CurrentNarrativeStateView,
    pub continuity_threads: Vec<ContinuityThreadView>,
    pub plan_task_refs: Vec<String>,
    pub plan_task_contexts: Vec<ContextPlanTask>,
    pub task_states: Vec<TaskStateRecord>,
    pub active_task_refs: Vec<String>,
    pub future_reservation_refs: Vec<String>,
    pub selected_evidence: Vec<EvidenceRef>,
    pub omitted_soft_sources: Vec<OmittedSource>,
    pub hard_tokens: i64,
    pub soft_tokens: i64,
}

pub fn context_hard_scope(input: &ContextCompilerInput) -> Value {
    let responsibility_refs = input
        .goal_contract_fields
        .iter()
        .find(|field| {
            field.field_path == "volumeIntent.crossVolumeResponsibilityRefs"
                || field
                    .additional_field_paths
                    .iter()
                    .any(|path| path == "volumeIntent.crossVolumeResponsibilityRefs")
        })
        .and_then(|field| field.value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            input
                .continuity_threads
                .iter()
                .map(|thread| thread.thread_id.clone())
                .collect()
        });
    json!({
        "taskRefs":input.plan_task_refs,
        "responsibilityRefs":responsibility_refs,
        "reservationRefs":input.future_reservation_refs,
        "entityRefs":input.state.bounds.entity_ids
    })
}

pub const CONTEXT_HARD_TOKEN_LIMIT: i64 = 4096;
pub const CONTEXT_HARD_SERIALIZED_BYTE_LIMIT: usize = 32768;
pub const CONTEXT_COMPILER_VERSION: &str = "b06-context-compiler.r7";
pub const CONTEXT_DIGEST_ALGORITHM_VERSION: &str = "sha256-canonical.r2";
pub const CONTEXT_HARD_BUDGET_POLICY_VERSION: &str = "b06-hard-budget.r2";
pub const CONTEXT_TOKEN_ESTIMATOR_VERSION: &str = "unicode-scalar-ascii-triplets.r1";
pub const CONTEXT_HARD_CONTRACT_CLASSIFICATION_VERSION: &str = "b06-contract-hard-fields.r3";
pub const OPENING_HARD_CONTRACT_FIELDS: &[&str] = &[
    "StoryConstitution.storyCore.protagonist",
    "StoryConstitution.storyCore.desire",
    "StoryConstitution.storyCore.mechanism",
    "StoryConstitution.storyCore.conflict",
    "StoryConstitution.storyCore.readerPromise",
    "StoryConstitution.storyCore.worldRules",
    "StoryConstitution.storyCore.forbiddenReveals",
    "MainContract.hardRules",
    "OpeningContract.firstChapterGoal",
    "OpeningContract.firstThreeChapterPromise",
    "OpeningContract.forbiddenReveals",
];
pub const LONG_FORM_HARD_CONTRACT_FIELDS: &[&str] = &[
    "StoryConstitution.storyCore.worldRules",
    "StoryConstitution.storyCore.mechanism",
    "StoryConstitution.storyCore.forbiddenReveals",
    "MainContract.hardRules",
    "BookPlan.futureReservations.applicable",
    "VolumeIntent.mustAchieve",
    "VolumeIntent.mustNotBreak",
    "VolumeIntent.requiredExitConditions",
    "VolumeIntent.futureReservationRefs",
    "VolumeIntent.crossVolumeResponsibilityRefs",
];
pub const LONG_FORM_OPENING_CANON_REFERENCE_RULES: &[&str] = &[
    "source=BookPlan.openingCanonRefs;fields=VolumeIntent.mustAchieve,VolumeIntent.mustNotBreak;hard=id+canonRefs;description=SOFT_CANON_DUPLICATE",
    "source=BookPlan.openingCanonRefs;opening-owner-items;hard=canonicalRefsOnly;description=SOFT_CANON_DUPLICATE",
];

/// Projects only current-volume or earlier reservations from a BookPlan into
/// the LONG_FORM contract set. Volume IDs are resolved by the authority
/// repository before calling this pure classifier.
pub fn project_applicable_future_reservations(
    book_plan: &Value,
    current_volume_no: i64,
    volume_numbers: &BTreeMap<String, i64>,
    explicit_refs: &BTreeSet<String>,
    revealed_refs: &BTreeSet<String>,
) -> Result<(Vec<Value>, usize), String> {
    let reservations = book_plan
        .get("futureReservations")
        .and_then(Value::as_array)
        .ok_or_else(|| "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope=LONG_FORM".to_string())?;
    let mut applicable = BTreeMap::new();
    let mut excluded = 0usize;
    for item in reservations {
        let unavailable =
            || "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope=LONG_FORM".to_string();
        let reservation_id = item
            .get("reservationId")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(unavailable)?;
        let reserved_for_no = item.get("reservedForVolumeNo").and_then(Value::as_i64);
        let reserved_for_id_no = item
            .get("reservedForVolumeId")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(|id| volume_numbers.get(id).copied().ok_or_else(unavailable))
            .transpose()?;
        if reserved_for_no.is_some()
            && reserved_for_id_no.is_some()
            && reserved_for_no != reserved_for_id_no
        {
            return Err(unavailable());
        }
        let reserved_for_no = reserved_for_no.or(reserved_for_id_no);
        let explicitly_selected = explicit_refs.contains(reservation_id);
        if !explicitly_selected
            && (revealed_refs.contains(reservation_id)
                || reserved_for_no.is_some_and(|number| number > current_volume_no))
        {
            excluded += 1;
            continue;
        }

        let prerequisites = item
            .get("prerequisites")
            .and_then(Value::as_array)
            .ok_or_else(unavailable)?;
        let may_foreshadow = item
            .get("mayForeshadow")
            .and_then(Value::as_bool)
            .ok_or_else(unavailable)?;
        let may_reveal = item
            .get("mayReveal")
            .and_then(Value::as_bool)
            .ok_or_else(unavailable)?;
        let mut projection = serde_json::Map::new();
        projection.insert("reservationId".into(), json!(reservation_id));
        for field in [
            "notBeforeVolumeNo",
            "reservedForVolumeNo",
            "reservedForVolumeId",
        ] {
            if let Some(value) = item.get(field).filter(|value| !value.is_null()) {
                projection.insert(field.into(), value.clone());
            }
        }
        if item
            .get("reservedForVolumeNo")
            .and_then(Value::as_i64)
            .is_none()
        {
            if let Some(reserved_for_id_no) = reserved_for_id_no {
                projection.insert("reservedForVolumeNo".into(), json!(reserved_for_id_no));
            }
        }
        projection.insert("mayForeshadow".into(), json!(may_foreshadow));
        projection.insert("mayReveal".into(), json!(may_reveal));
        projection.insert("prerequisites".into(), json!(prerequisites));
        if applicable
            .insert(reservation_id.to_owned(), Value::Object(projection))
            .is_some()
        {
            return Err(unavailable());
        }
    }
    Ok((applicable.into_values().collect(), excluded))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextDigestConfig {
    compiler_version: &'static str,
    digest_algorithm_version: &'static str,
    hard_budget_policy_version: &'static str,
    token_estimator_version: &'static str,
    contract_classification_version: &'static str,
    hard_token_limit: i64,
    hard_serialized_byte_limit: usize,
}

const CURRENT_CONTEXT_DIGEST_CONFIG: ContextDigestConfig = ContextDigestConfig {
    compiler_version: CONTEXT_COMPILER_VERSION,
    digest_algorithm_version: CONTEXT_DIGEST_ALGORITHM_VERSION,
    hard_budget_policy_version: CONTEXT_HARD_BUDGET_POLICY_VERSION,
    token_estimator_version: CONTEXT_TOKEN_ESTIMATOR_VERSION,
    contract_classification_version: CONTEXT_HARD_CONTRACT_CLASSIFICATION_VERSION,
    hard_token_limit: CONTEXT_HARD_TOKEN_LIMIT,
    hard_serialized_byte_limit: CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHardBudgetUsage {
    pub estimated_tokens: i64,
    pub serialized_bytes: usize,
    pub source_sizes: Vec<ContextHardSourceSize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextHardSourceSize {
    pub kind: String,
    pub source_ref: String,
    pub selection_kind: Option<String>,
    pub selection_ref: Option<String>,
    pub field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_source_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_field_paths: Vec<String>,
    pub estimated_tokens: i64,
    pub serialized_bytes: usize,
    pub adjustable: bool,
}

/// Measures the full HARD input payload deterministically. The provider may
/// use a model-specific tokenizer, so admission uses a stable estimate: one
/// unit per non-ASCII scalar and three ASCII bytes per token unit.
pub fn measure_context_hard_budget(
    input: &ContextCompilerInput,
) -> Result<ContextHardBudgetUsage, String> {
    let mut hard_input = input.clone();
    hard_input.hard_tokens = 0;
    hard_input.soft_tokens = 0;
    hard_input.omitted_soft_sources.clear();
    let serialized = serde_json::to_string(&hard_input).map_err(|error| error.to_string())?;
    let ascii_bytes = serialized.bytes().filter(u8::is_ascii).count();
    let non_ascii_scalars = serialized
        .chars()
        .filter(|character| !character.is_ascii())
        .count();
    let estimated_tokens = (ascii_bytes.div_ceil(3) + non_ascii_scalars) as i64;
    Ok(ContextHardBudgetUsage {
        estimated_tokens,
        serialized_bytes: serialized.len(),
        source_sizes: context_hard_source_sizes(input)?,
    })
}

fn context_hard_source_sizes(
    input: &ContextCompilerInput,
) -> Result<Vec<ContextHardSourceSize>, String> {
    fn push(
        output: &mut Vec<ContextHardSourceSize>,
        kind: &str,
        source_ref: &str,
        selection: Option<(&str, &str)>,
        field_path: Option<&str>,
        value: &Value,
        adjustable: bool,
    ) -> Result<(), String> {
        let selection_kind = selection.map(|(kind, _)| kind);
        let selection_ref = selection.map(|(_, reference)| reference);
        let serialized = serde_json::to_string(&json!({
            "kind":kind,
            "sourceRef":source_ref,
            "selectionKind":selection_kind,
            "selectionRef":selection_ref,
            "fieldPath":field_path,
            "value":value
        }))
        .map_err(|error| error.to_string())?;
        let ascii_bytes = serialized.bytes().filter(u8::is_ascii).count();
        let non_ascii_scalars = serialized
            .chars()
            .filter(|character| !character.is_ascii())
            .count();
        output.push(ContextHardSourceSize {
            kind: kind.into(),
            source_ref: source_ref.into(),
            selection_kind: selection_kind.map(ToOwned::to_owned),
            selection_ref: selection_ref.map(ToOwned::to_owned),
            field_path: field_path.map(ToOwned::to_owned),
            additional_source_refs: Vec::new(),
            additional_field_paths: Vec::new(),
            estimated_tokens: (ascii_bytes.div_ceil(3) + non_ascii_scalars) as i64,
            serialized_bytes: serialized.len(),
            adjustable,
        });
        Ok(())
    }

    let mut sizes = Vec::new();
    for field in &input.goal_contract_fields {
        let field_value = json!({
            "value":field.value,
            "additionalSourceRefs":field.additional_source_refs,
            "additionalFieldPaths":field.additional_field_paths
        });
        push(
            &mut sizes,
            "CONTRACT",
            &field.source_ref.artifact_id,
            None,
            Some(&field.field_path),
            &field_value,
            false,
        )?;
        let size = sizes.last_mut().expect("contract source was just added");
        size.additional_source_refs = field
            .additional_source_refs
            .iter()
            .map(|source| {
                format!(
                    "{}@{}#{}",
                    source.artifact_id, source.version, source.payload_hash
                )
            })
            .collect();
        size.additional_field_paths = field.additional_field_paths.clone();
    }
    for entity_id in &input.state.bounds.entity_ids {
        push(
            &mut sizes,
            "ENTITY",
            entity_id,
            Some(("ENTITY", entity_id)),
            None,
            &json!({"entityId":entity_id}),
            true,
        )?;
    }
    for fact in &input.state.facts {
        push(
            &mut sizes,
            "CANON",
            &fact.fact_id,
            Some(("ENTITY", &fact.subject_id)),
            None,
            &serde_json::to_value(fact).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for relation in &input.state.relations {
        push(
            &mut sizes,
            "CANON",
            &relation.relation_id,
            Some(("ENTITY", &relation.subject_id)),
            None,
            &serde_json::to_value(relation).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for knowledge in &input.state.knowledge {
        push(
            &mut sizes,
            "KNOWLEDGE",
            &knowledge.knowledge_id,
            Some(("ENTITY", &knowledge.knower_id)),
            None,
            &serde_json::to_value(knowledge).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for event in &input.state.events {
        push(
            &mut sizes,
            "CANON",
            &event.event_id,
            None,
            None,
            &serde_json::to_value(event).map_err(|error| error.to_string())?,
            false,
        )?;
    }
    for responsibility in &input.state.active_responsibilities {
        push(
            &mut sizes,
            "CONTINUITY",
            &responsibility.obligation_id,
            Some(("RESPONSIBILITY", &responsibility.obligation_id)),
            None,
            &serde_json::to_value(responsibility).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for thread in &input.continuity_threads {
        push(
            &mut sizes,
            "CONTINUITY",
            &thread.thread_id,
            Some(("RESPONSIBILITY", &thread.thread_id)),
            None,
            &serde_json::to_value(thread).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for task_ref in &input.plan_task_refs {
        push(
            &mut sizes,
            "TASK",
            task_ref,
            Some(("TASK", task_ref)),
            None,
            &json!({"taskRef":task_ref}),
            true,
        )?;
    }
    for task in &input.plan_task_contexts {
        push(
            &mut sizes,
            "TASK",
            &task.task_ref,
            Some(("TASK", &task.task_ref)),
            None,
            &task.contract,
            true,
        )?;
    }
    for task in &input.task_states {
        push(
            &mut sizes,
            "TASK",
            &task.state_id,
            Some(("TASK", &task.state_id)),
            None,
            &serde_json::to_value(task).map_err(|error| error.to_string())?,
            true,
        )?;
    }
    for reservation_ref in &input.future_reservation_refs {
        push(
            &mut sizes,
            "RESERVATION",
            reservation_ref,
            Some(("RESERVATION", reservation_ref)),
            None,
            &json!({"reservationRef":reservation_ref}),
            true,
        )?;
    }
    for evidence in &input.selected_evidence {
        let reference = format!(
            "{}@{}:{}-{}",
            evidence.artifact_id,
            evidence.version,
            evidence.range_start.unwrap_or_default(),
            evidence.range_end.unwrap_or_default()
        );
        push(
            &mut sizes,
            "EVIDENCE",
            &reference,
            None,
            None,
            &serde_json::to_value(evidence).map_err(|error| error.to_string())?,
            false,
        )?;
    }
    sizes.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.source_ref.cmp(&b.source_ref))
            .then_with(|| a.field_path.cmp(&b.field_path))
    });
    Ok(sizes)
}

pub fn validate_context_hard_budget(
    input: &ContextCompilerInput,
) -> Result<ContextHardBudgetUsage, String> {
    let usage = measure_context_hard_budget(input)?;
    if usage.estimated_tokens > CONTEXT_HARD_TOKEN_LIMIT
        || usage.serialized_bytes > CONTEXT_HARD_SERIALIZED_BYTE_LIMIT
    {
        let scope = context_scope(&input.state.bounds);
        let details = serde_json::to_string(&json!({
            "usedTokens":usage.estimated_tokens,
            "limitTokens":CONTEXT_HARD_TOKEN_LIMIT,
            "serializedBytes":usage.serialized_bytes,
            "limitBytes":CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
            "scope":scope,
            "budgetPolicyVersion":CONTEXT_HARD_BUDGET_POLICY_VERSION,
            "contractClassificationVersion":CONTEXT_HARD_CONTRACT_CLASSIFICATION_VERSION,
            "sourceSizes":usage.source_sizes
        }))
        .map_err(|error| error.to_string())?;
        return Err(format!("CONTEXT_HARD_BUDGET_EXCEEDED|details={details}"));
    }
    Ok(usage)
}

fn context_scope(bounds: &RuntimeBounds) -> String {
    if let Some(chapter_id) = bounds.chapter_id.as_deref() {
        format!("CHAPTER:{chapter_id}")
    } else if let Some(volume_id) = bounds.volume_id.as_deref() {
        format!("VOLUME:{volume_id}")
    } else {
        format!("PROJECT:{}", bounds.project_id)
    }
}

pub fn relevant_state_digest(input: &ContextCompilerInput) -> Result<String, String> {
    relevant_state_digest_with_config(input, CURRENT_CONTEXT_DIGEST_CONFIG)
}

fn relevant_state_digest_with_config(
    input: &ContextCompilerInput,
    config: ContextDigestConfig,
) -> Result<String, String> {
    validate_context_hard_completeness(input)?;
    let mut goal_contract_refs = input.goal_contract_refs.clone();
    goal_contract_refs.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    let mut goal_contract_fields = input.goal_contract_fields.clone();
    for field in &mut goal_contract_fields {
        field.additional_source_refs.sort_by(|a, b| {
            a.artifact_id
                .cmp(&b.artifact_id)
                .then_with(|| a.version.cmp(&b.version))
        });
        field.additional_field_paths.sort();
    }
    goal_contract_fields.sort_by(|a, b| {
        a.source_ref
            .artifact_id
            .cmp(&b.source_ref.artifact_id)
            .then_with(|| a.field_path.cmp(&b.field_path))
    });
    let mut facts = input.state.facts.clone();
    facts.sort_by(|a, b| a.fact_id.cmp(&b.fact_id));
    let mut relations = input.state.relations.clone();
    relations.sort_by(|a, b| a.relation_id.cmp(&b.relation_id));
    let mut knowledge = input.state.knowledge.clone();
    knowledge.sort_by(|a, b| a.knowledge_id.cmp(&b.knowledge_id));
    let mut events = input.state.events.clone();
    events.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    let mut threads = input.continuity_threads.clone();
    threads.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    let mut plan_task_refs = input.plan_task_refs.clone();
    plan_task_refs.sort();
    let mut plan_task_contexts = input.plan_task_contexts.clone();
    plan_task_contexts.sort_by(|a, b| a.task_ref.cmp(&b.task_ref));
    let mut task_states = input.task_states.clone();
    task_states.sort_by(|a, b| a.state_id.cmp(&b.state_id));
    let mut task_refs = input.active_task_refs.clone();
    task_refs.sort();
    let mut entity_ids = input.state.bounds.entity_ids.clone();
    entity_ids.sort();
    let mut future_reservation_refs = input.future_reservation_refs.clone();
    future_reservation_refs.sort();
    let mut selected_evidence = input.selected_evidence.clone();
    selected_evidence.sort_by(|a, b| {
        a.artifact_id
            .cmp(&b.artifact_id)
            .then_with(|| a.range_start.cmp(&b.range_start))
            .then_with(|| a.range_end.cmp(&b.range_end))
    });
    sha256_canonical(&json!({
        "executionMode":input.execution_mode,
        "contextCompilerVersion":config.compiler_version,
        "digestAlgorithmVersion":config.digest_algorithm_version,
        "hardBudgetPolicy":{
            "version":config.hard_budget_policy_version,
            "tokenEstimatorVersion":config.token_estimator_version,
            "tokenLimit":config.hard_token_limit,
            "serializedByteLimit":config.hard_serialized_byte_limit
        },
        "hardContractClassification":{
            "version":config.contract_classification_version,
            "openingFields":OPENING_HARD_CONTRACT_FIELDS,
            "longFormFields":LONG_FORM_HARD_CONTRACT_FIELDS,
            "longFormOpeningCanonReferenceRules":LONG_FORM_OPENING_CANON_REFERENCE_RULES
        },
        "facts":facts,
        "relations":relations,
        "knowledge":knowledge,
        "events":events,
        "threads":threads,
        "planTaskRefs":plan_task_refs,
        "planTaskContexts":plan_task_contexts,
        "taskStates":task_states,
        "activeTaskRefs":task_refs,
        "entityRefs":entity_ids,
        "goalContractRefs":goal_contract_refs,
        "goalContractFields":goal_contract_fields,
        "futureReservationRefs":future_reservation_refs,
        "selectedEvidence":selected_evidence
    }))
    .map_err(|error| error.to_string())
}

pub fn compile_context_manifest(
    mut input: ContextCompilerInput,
) -> Result<ContextManifestR1, String> {
    validate_context_hard_completeness(&input)?;
    let hard_budget_usage = measure_context_hard_budget(&input)?;
    input
        .goal_contract_refs
        .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    input
        .selected_evidence
        .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    input.active_task_refs.sort();
    input
        .continuity_threads
        .sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    let digest = relevant_state_digest(&input)?;
    let mut state_sources = input.state.source_refs.clone();
    state_sources.sort_by(|a, b| a.ref_.cmp(&b.ref_));
    let hard_source_count = (input.goal_contract_fields.len()
        + state_sources.len()
        + input.continuity_threads.len()
        + input.plan_task_refs.len()
        + input.plan_task_contexts.len()
        + input.task_states.len()
        + input.selected_evidence.len()) as i64;
    Ok(ContextManifestR1 {
        payload_schema: json!("context-manifest.r1"),
        execution_mode: input.execution_mode,
        compiled_from_project_revision: input.project_revision,
        compiled_from_state_digest: digest,
        goal_contract_refs: input.goal_contract_refs,
        active_volume_task_refs: input.active_task_refs,
        continuity_thread_refs: input
            .continuity_threads
            .iter()
            .map(|thread| thread.thread_id.clone())
            .collect(),
        future_reservation_refs: input.future_reservation_refs,
        narrative_state_sources: state_sources,
        selected_evidence: input.selected_evidence,
        hard_source_count,
        omitted_soft_sources: input.omitted_soft_sources,
        budget: ContextManifestR1Budget {
            hard_tokens: hard_budget_usage.estimated_tokens,
            soft_tokens: input.soft_tokens.max(0),
            total_tokens: hard_budget_usage.estimated_tokens + input.soft_tokens.max(0),
        },
    })
}

pub fn validate_context_hard_completeness(input: &ContextCompilerInput) -> Result<(), String> {
    let mut sources = input.state.truncated_sources.clone();
    if input.state.truncated && sources.is_empty() {
        sources.insert(ContextHardSource::Unknown);
    }
    if sources.is_empty() {
        validate_context_hard_budget(input)?;
        return Ok(());
    }
    let sources = sources
        .into_iter()
        .map(ContextHardSource::code)
        .collect::<Vec<_>>()
        .join(",");
    let scope = if let Some(chapter_id) = input.state.bounds.chapter_id.as_deref() {
        format!("CHAPTER:{chapter_id}")
    } else if let Some(volume_id) = input.state.bounds.volume_id.as_deref() {
        format!("VOLUME:{volume_id}")
    } else {
        format!("PROJECT:{}", input.state.bounds.project_id)
    };
    Err(format!(
        "CONTEXT_HARD_INCOMPLETE|sources={sources}|scope={scope}"
    ))
}

/// B06's persisted opening context owner. Callers provide only a typed
/// authority request; this function resolves the bounded authority input,
/// compiles the manifest with the existing narrative compiler, and records the
/// resulting artifact through the frozen CommitService.
pub fn commit_opening_context_owner(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    authority_request: &Value,
    expected_revision: i64,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<(CommitReceipt, ArtifactRef), String> {
    if authority_request
        .get("executionMode")
        .and_then(Value::as_str)
        != Some("OPENING_BOOTSTRAP")
    {
        return Err("B06_OPENING_CONTEXT_MODE_REQUIRED".into());
    }
    let input = crate::commands::context_input_from_authority(
        connection,
        authority_request,
        project_id,
        expected_revision,
    )?;
    let context = compile_context_manifest(input)?;
    let payload = serde_json::to_value(&context).map_err(|error| error.to_string())?;
    let payload_hash =
        crate::hash::sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let chapter_id = authority_request
        .get("chapterId")
        .and_then(Value::as_str)
        .ok_or_else(|| "B06_OPENING_CHAPTER_REQUIRED".to_string())?;
    let artifact_id = format!("context:{project_id}:{command_id}");
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"ContextManifest",
        "scope":{"level":"CHAPTER","chapterId":chapter_id},
        "version":1,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"APPROVED_PLAN",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":context.goal_contract_refs,
        "provenance":{"createdBy":"B06::ContextCompilerOwner","commandId":command_id,"stepId":"B06::ContextCompilerOwner"},
        "createdAt":now
    }))
    .map_err(|error| format!("B06_CONTEXT_ARTIFACT_INVALID:{error}"))?;
    let reference = ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    };
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":now,
        "artifacts":[artifact],
        "receiptDraft":{"domain":"B06_OPENING_CONTEXT","artifactRefs":[reference.clone()],"payload":{"owner":"B06::ContextCompilerOwner","executionMode":"OPENING_BOOTSTRAP","compiledFromStateDigest":context.compiled_from_state_digest}}
    }))
    .map_err(|error| format!("B06_CONTEXT_BUNDLE_INVALID:{error}"))?;
    let receipt =
        CommitService::commit(connection, &bundle, now).map_err(|error| error.to_string())?;
    Ok((receipt, reference))
}

/// B06 context owner for an ordinary LONG_FORM chapter. The compiler reads
/// the same bounded authority input as the opening path, but the caller may
/// select the execution mode explicitly. Persistence still goes through the
/// existing ContextManifest artifact and CommitService boundary.
#[allow(clippy::too_many_arguments)]
pub fn commit_context_manifest_owner(
    connection: &Connection,
    project_id: &str,
    authority_request: &Value,
    expected_revision: i64,
    command_id: &str,
    idempotency_key: &str,
    execution_mode: &str,
    now: &str,
) -> Result<(CommitReceipt, ArtifactRef), String> {
    if !matches!(execution_mode, "OPENING_BOOTSTRAP" | "LONG_FORM") {
        return Err("B06_CONTEXT_MODE_REQUIRED".into());
    }
    let mut authority = authority_request.clone();
    authority["executionMode"] = json!(execution_mode);
    let input = crate::commands::context_input_from_authority(
        connection,
        &authority,
        project_id,
        expected_revision,
    )?;
    let context = compile_context_manifest(input)?;
    let payload = serde_json::to_value(&context).map_err(|error| error.to_string())?;
    let payload_hash =
        crate::hash::sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let chapter_id = authority
        .get("chapterId")
        .and_then(Value::as_str)
        .ok_or_else(|| "B06_CHAPTER_REQUIRED".to_string())?;
    let artifact_id = format!("context:{project_id}:{command_id}");
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"ContextManifest",
        "scope":{"level":"CHAPTER","chapterId":chapter_id},
        "version":1,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"APPROVED_PLAN",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":context.goal_contract_refs,
        "provenance":{"createdBy":"B06::ContextCompilerOwner","commandId":command_id,"stepId":"B06::ContextCompilerOwner"},
        "createdAt":now
    }))
    .map_err(|error| format!("B06_CONTEXT_ARTIFACT_INVALID:{error}"))?;
    let reference = ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    };
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":now,
        "artifacts":[artifact],
        "receiptDraft":{"domain":"B06_CONTEXT_MANIFEST","artifactRefs":[reference.clone()],"payload":{"owner":"B06::ContextCompilerOwner","executionMode":execution_mode,"compiledFromStateDigest":context.compiled_from_state_digest}}
    }))
    .map_err(|error| format!("B06_CONTEXT_BUNDLE_INVALID:{error}"))?;
    let receipt =
        CommitService::commit(connection, &bundle, now).map_err(|error| error.to_string())?;
    Ok((receipt, reference))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStaleStatus {
    Valid,
    PlanStale,
}

pub fn evaluate_plan_stale(
    _compiled_revision: i64,
    compiled_digest: &str,
    _current_revision: i64,
    current_digest: &str,
) -> PlanStaleStatus {
    if compiled_digest == current_digest {
        PlanStaleStatus::Valid
    } else {
        PlanStaleStatus::PlanStale
    }
}

#[cfg(test)]
mod b2_context_tests {
    use super::*;

    fn empty_input() -> ContextCompilerInput {
        ContextCompilerInput {
            execution_mode: "LONG_FORM".into(),
            project_revision: 7,
            goal_contract_refs: Vec::new(),
            goal_contract_fields: Vec::new(),
            state: CurrentNarrativeStateView::from_records(
                RuntimeBounds {
                    project_id: "project:b2".into(),
                    entity_ids: Vec::new(),
                    volume_id: Some("volume:b2".into()),
                    chapter_id: Some("chapter:b2".into()),
                    limit: 100,
                },
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                false,
            ),
            continuity_threads: Vec::new(),
            plan_task_refs: Vec::new(),
            plan_task_contexts: Vec::new(),
            task_states: Vec::new(),
            active_task_refs: Vec::new(),
            future_reservation_refs: Vec::new(),
            selected_evidence: Vec::new(),
            omitted_soft_sources: Vec::new(),
            hard_tokens: 0,
            soft_tokens: 0,
        }
    }

    #[test]
    fn rv14_same_revision_digest_change_requires_recompile() {
        assert_eq!(
            evaluate_plan_stale(7, "old-context-digest", 7, "new-context-digest"),
            PlanStaleStatus::PlanStale
        );
    }

    #[test]
    fn rv14_digest_changes_when_hard_budget_policy_version_changes() {
        let input = empty_input();
        let original = relevant_state_digest(&input).unwrap();
        let next_policy = ContextDigestConfig {
            hard_budget_policy_version: "b06-hard-budget.r3",
            ..CURRENT_CONTEXT_DIGEST_CONFIG
        };
        let updated = relevant_state_digest_with_config(&input, next_policy).unwrap();
        assert_ne!(original, updated);
    }

    #[test]
    fn rv14_digest_versions_contract_classification_and_compiler_rules() {
        let input = empty_input();
        let original = relevant_state_digest(&input).unwrap();
        assert_eq!(original, relevant_state_digest(&input).unwrap());

        let next_classification = ContextDigestConfig {
            contract_classification_version: "b06-contract-hard-fields.r4",
            ..CURRENT_CONTEXT_DIGEST_CONFIG
        };
        let reclassified = relevant_state_digest_with_config(&input, next_classification).unwrap();
        assert_ne!(original, reclassified);

        let next_compiler = ContextDigestConfig {
            compiler_version: "b06-context-compiler.r8",
            ..CURRENT_CONTEXT_DIGEST_CONFIG
        };
        let recompiled = relevant_state_digest_with_config(&input, next_compiler).unwrap();
        assert_ne!(original, recompiled);
    }

    #[test]
    fn rv09_future_reservation_default_scope_excludes_future_and_revealed_items() {
        let book_plan = json!({"futureReservations":[
            {"reservationId":"reservation:earlier","reservedForVolumeNo":2,"notBeforeVolumeNo":2,"prerequisites":[],"mayForeshadow":true,"mayReveal":true},
            {"reservationId":"reservation:current","reservedForVolumeNo":3,"notBeforeVolumeNo":3,"prerequisites":[],"mayForeshadow":true,"mayReveal":true},
            {"reservationId":"reservation:future","reservedForVolumeNo":4,"notBeforeVolumeNo":4,"prerequisites":[],"mayForeshadow":true,"mayReveal":true},
            {"reservationId":"reservation:revealed","reservedForVolumeNo":2,"notBeforeVolumeNo":2,"prerequisites":[],"mayForeshadow":true,"mayReveal":true},
            {"reservationId":"reservation:unbounded","prerequisites":[],"mayForeshadow":true,"mayReveal":true}
        ]});
        let revealed = BTreeSet::from(["reservation:revealed".to_string()]);
        let (reservations, excluded) = project_applicable_future_reservations(
            &book_plan,
            3,
            &BTreeMap::new(),
            &BTreeSet::new(),
            &revealed,
        )
        .unwrap();
        let refs = reservations
            .iter()
            .filter_map(|reservation| reservation["reservationId"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            vec![
                "reservation:current",
                "reservation:earlier",
                "reservation:unbounded"
            ]
        );
        assert_eq!(excluded, 2);
    }

    #[test]
    fn rv09_explicit_future_reservation_keeps_gate_metadata_in_hard_projection() {
        let book_plan = json!({"futureReservations":[{
            "reservationId":"reservation:future:5",
            "reservedForVolumeId":"volume:5",
            "notBeforeVolumeNo":5,
            "prerequisites":["fact:before-reveal"],
            "mayForeshadow":true,
            "mayReveal":false
        }]});
        let explicit = BTreeSet::from(["reservation:future:5".to_string()]);
        let volume_numbers = BTreeMap::from([("volume:5".to_string(), 5)]);
        let (reservations, excluded) = project_applicable_future_reservations(
            &book_plan,
            1,
            &volume_numbers,
            &explicit,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(excluded, 0);
        assert_eq!(reservations.len(), 1);
        assert_eq!(reservations[0]["reservationId"], "reservation:future:5");
        assert_eq!(reservations[0]["reservedForVolumeId"], "volume:5");
        assert_eq!(reservations[0]["reservedForVolumeNo"], 5);
        assert_eq!(reservations[0]["notBeforeVolumeNo"], 5);
        assert_eq!(
            reservations[0]["prerequisites"],
            json!(["fact:before-reveal"])
        );
        assert_eq!(reservations[0]["mayForeshadow"], true);
        assert_eq!(reservations[0]["mayReveal"], false);
    }

    fn fx1_obligation(source_event_id: Option<&str>) -> ObligationRecord {
        ObligationRecord {
            obligation_id: "obligation:promise".into(),
            obligation_type: "PROMISE".into(),
            authority_level: "B".into(),
            lifecycle: "APPROVED".into(),
            description: "兑现承诺".into(),
            fulfillment_object: None,
            source_event_id: source_event_id.map(ToOwned::to_owned),
            resume_condition: json!({"kind":"AFTER_CHAPTER","value":0}),
            participant_entities: Vec::new(),
            evidence_refs: Vec::new(),
        }
    }

    fn fx1_evidence(
        evidence_id: &str,
        fact_id: Option<&str>,
        event_id: Option<&str>,
        quote: &str,
    ) -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: evidence_id.into(),
            fact_id: fact_id.map(ToOwned::to_owned),
            relation_id: None,
            event_id: event_id.map(ToOwned::to_owned),
            final_chapter_id: "chapter:one".into(),
            source_artifact_id: "final:one".into(),
            version: 1,
            evidence_type: "ACTUAL_NARRATIVE".into(),
            quote: Some(quote.into()),
            range_start: Some(0),
            range_end: Some(quote.chars().count() as i64),
        }
    }

    #[test]
    fn fx1_evidence_without_event_is_not_attached_to_unrelated_obligation() {
        let threads = build_continuity_threads(
            &[fx1_obligation(None)],
            &[],
            &[],
            &[fx1_evidence(
                "evidence:unrelated",
                Some("fact:unrelated"),
                None,
                "无关事实",
            )],
            &ResumeContext::default(),
        )
        .unwrap();

        assert!(threads[0].evidence_refs.is_empty());
    }

    #[test]
    fn fx1_evidence_matches_only_same_source_event() {
        let threads = build_continuity_threads(
            &[fx1_obligation(Some("event:promise"))],
            &[],
            &[],
            &[
                fx1_evidence("evidence:matching", None, Some("event:promise"), "承诺原文"),
                fx1_evidence(
                    "evidence:other-event",
                    None,
                    Some("event:other"),
                    "其他事件",
                ),
                fx1_evidence(
                    "evidence:matching-fact",
                    Some("obligation:promise"),
                    Some("event:other"),
                    "责任事实原文",
                ),
            ],
            &ResumeContext::default(),
        )
        .unwrap();

        let quotes = threads[0]
            .evidence_refs
            .iter()
            .filter_map(|reference| reference.quote.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(quotes, vec!["承诺原文", "责任事实原文"]);
    }
}
