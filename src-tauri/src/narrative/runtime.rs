use crate::generated::types::{
    ArtifactRef, ContextManifestR1, ContextManifestR1Budget, EvidenceRef, OmittedSource, SourceRef,
    VolumeProgressProjection,
};
use crate::hash::sha256_canonical;
use crate::repositories::narrative::{
    CanonEventRecord, CanonFactRecord, EvidenceRecord, ForeshadowRecord, KnowledgeRecord,
    MysteryRecord, ObligationRecord, RelationRecord, TaskStateRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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
            source_refs.insert(format!("fact:{}", fact.fact_id));
        }
        for relation in &relations {
            source_refs.insert(format!("relation:{}", relation.relation_id));
        }
        for knowledge in &knowledge {
            source_refs.insert(format!("knowledge:{}", knowledge.knowledge_id));
        }
        for event in &events {
            source_refs.insert(format!("event:{}", event.event_id));
        }
        for obligation in &active_responsibilities {
            source_refs.insert(format!("obligation:{}", obligation.obligation_id));
        }
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
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityThreadView {
    pub thread_id: String,
    pub thread_class: String,
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
        let evidence_refs = evidence
            .iter()
            .filter(|item| {
                item.fact_id.as_deref() == Some(obligation.obligation_id.as_str())
                    || item.event_id.as_deref() == obligation.source_event_id.as_deref()
            })
            .map(evidence_ref)
            .collect();
        result.push(ContinuityThreadView {
            thread_id: format!("thread:{}", obligation.obligation_id),
            thread_class: obligation.obligation_type.clone(),
            logical_status: logical_status.into(),
            source_event_ref: obligation
                .source_event_id
                .clone()
                .map(|id| format!("event:{id}")),
            evidence_refs,
            participant_entities: Vec::new(),
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
        result.push(ContinuityThreadView { thread_id: format!("thread:{}", foreshadow.foreshadow_id), thread_class: "FORESHADOW".into(), logical_status: logical_status.into(), source_event_ref: None, evidence_refs: Vec::new(), participant_entities: Vec::new(), resume_condition: json!({"kind":"ENTITY_PRESENT","value":foreshadow.secret_payload_ref}), authority_level: "C".into(), current_disposition: foreshadow.status.clone(), source_refs: vec![SourceRef { kind: "foreshadow".into(), ref_: foreshadow.foreshadow_id.clone() }] });
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
    pub target_chapters: i64,
    pub current_chapter: i64,
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
        remaining_chapter_capacity: (input.target_chapters - input.current_chapter).max(0),
        planning_pressure: pressure,
        generated_revision: input.generated_revision,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompilerInput {
    pub execution_mode: String,
    pub project_revision: i64,
    pub goal_contract_refs: Vec<ArtifactRef>,
    pub state: CurrentNarrativeStateView,
    pub continuity_threads: Vec<ContinuityThreadView>,
    pub active_task_refs: Vec<String>,
    pub future_reservation_refs: Vec<String>,
    pub selected_evidence: Vec<EvidenceRef>,
    pub omitted_soft_sources: Vec<OmittedSource>,
    pub hard_tokens: i64,
    pub soft_tokens: i64,
}

pub fn relevant_state_digest(input: &ContextCompilerInput) -> Result<String, String> {
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
    let mut task_refs = input.active_task_refs.clone();
    task_refs.sort();
    sha256_canonical(&json!({"facts":facts,"relations":relations,"knowledge":knowledge,"events":events,"threads":threads,"activeTaskRefs":task_refs,"goalContractRefs":input.goal_contract_refs,"futureReservationRefs":input.future_reservation_refs})).map_err(|error| error.to_string())
}

pub fn compile_context_manifest(
    mut input: ContextCompilerInput,
) -> Result<ContextManifestR1, String> {
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
    let hard_source_count = (state_sources.len()
        + input.continuity_threads.len()
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
            hard_tokens: input.hard_tokens.max(0),
            soft_tokens: input.soft_tokens.max(0),
            total_tokens: input.hard_tokens.max(0) + input.soft_tokens.max(0),
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStaleStatus {
    Valid,
    PlanStale,
}

pub fn evaluate_plan_stale(
    compiled_revision: i64,
    compiled_digest: &str,
    current_revision: i64,
    current_digest: &str,
) -> PlanStaleStatus {
    if compiled_revision == current_revision || compiled_digest == current_digest {
        PlanStaleStatus::Valid
    } else {
        PlanStaleStatus::PlanStale
    }
}
