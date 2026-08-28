use crate::generated::types::{
    CanonDelta, DecisionUpdate, DependencyEdge, ForeshadowDelta, GateCertificate, JobUpdate,
    KnowledgeDelta, ObligationDelta, ReaderDesireDelta, StateDelta,
};
use crate::hash::sha256_canonical;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn invalid(message: impl Into<String>) -> super::CommitError {
    super::CommitError::ContractInvalid(message.into())
}

fn map_json(map: Option<&BTreeMap<String, Value>>) -> Result<String, super::CommitError> {
    Ok(serde_json::to_string(map.unwrap_or(&BTreeMap::new()))?)
}

fn payload_string(map: Option<&BTreeMap<String, Value>>, key: &str) -> Option<String> {
    map.and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn payload_i64(map: Option<&BTreeMap<String, Value>>, key: &str) -> Option<i64> {
    map.and_then(|value| value.get(key)).and_then(Value::as_i64)
}

fn payload_json(map: Option<&BTreeMap<String, Value>>, key: &str) -> Option<String> {
    map.and_then(|value| value.get(key)).map(Value::to_string)
}

fn source_artifact(
    payload: Option<&BTreeMap<String, Value>>,
    fallback: Option<(&str, i64)>,
) -> Result<(String, i64), super::CommitError> {
    let id = payload_string(payload, "sourceArtifactId")
        .or_else(|| fallback.map(|(id, _)| id.to_string()))
        .ok_or_else(|| invalid("missing source artifact id"))?;
    let version = payload_i64(payload, "sourceVersion")
        .or_else(|| fallback.map(|(_, version)| version))
        .ok_or_else(|| invalid("missing source artifact version"))?;
    Ok((id, version))
}

fn row_exists(
    connection: &Connection,
    sql: &str,
    params: impl libsql_rusqlite::Params,
) -> Result<bool, super::CommitError> {
    Ok(connection.query_row(sql, params, |row| row.get(0))?)
}

fn apply_canon(
    connection: &Connection,
    delta: &CanonDelta,
    now: &str,
) -> Result<i64, super::CommitError> {
    let project_id = delta_project(connection)?;
    let mut inserted = 0;
    if let Some(entity_id) = &delta.entity_id {
        let existed = row_exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM canon_entities WHERE project_id = ?1 AND entity_id = ?2)",
            libsql_rusqlite::params![project_id, entity_id],
        )?;
        let payload = delta
            .payload_json
            .as_ref()
            .map(|x| x as &BTreeMap<String, Value>);
        let name = payload_string(payload, "canonicalName").unwrap_or_else(|| entity_id.clone());
        let entity_type = payload_string(payload, "entityType").unwrap_or_else(|| "UNKNOWN".into());
        connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5) ON CONFLICT(entity_id) DO UPDATE SET canonical_name=excluded.canonical_name, entity_type=excluded.entity_type, updated_at=excluded.updated_at", libsql_rusqlite::params![entity_id, project_id, name, entity_type, now])?;
        inserted += i64::from(!existed);
    }
    let subject_id = delta
        .subject_id
        .clone()
        .or_else(|| delta.entity_id.clone())
        .ok_or_else(|| invalid("canon delta requires subjectId or entityId"))?;
    let predicate = delta
        .predicate
        .clone()
        .ok_or_else(|| invalid("canon delta requires predicate"))?;
    let object_json = serde_json::to_string(&delta.object_json.clone().unwrap_or_default())?;
    let payload = delta
        .payload_json
        .as_ref()
        .map(|x| x as &BTreeMap<String, Value>);
    let (source_artifact_id, source_version) = match source_artifact(payload, None) {
        Ok(value) => (Some(value.0), Some(value.1)),
        Err(_) => (None, None),
    };
    let fact_existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM canon_facts WHERE project_id = ?1 AND fact_id = ?2)",
        libsql_rusqlite::params![project_id, delta.fact_id],
    )?;
    connection.execute("INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, valid_from_chapter_id, valid_to_chapter_id, source_artifact_id, source_version, authority_level, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11) ON CONFLICT(fact_id) DO UPDATE SET subject_id=excluded.subject_id, predicate=excluded.predicate, object_json=excluded.object_json, valid_from_chapter_id=excluded.valid_from_chapter_id, valid_to_chapter_id=excluded.valid_to_chapter_id, source_artifact_id=excluded.source_artifact_id, source_version=excluded.source_version, authority_level=excluded.authority_level, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.fact_id, project_id, subject_id, predicate, object_json, delta.valid_from_chapter_id, delta.valid_to_chapter_id, source_artifact_id, source_version, delta.authority_level.as_ref().map(crate::repositories::artifacts::enum_string).transpose()?.unwrap_or_else(|| "C".into()), now])?;
    inserted += i64::from(!fact_existed);
    if let Some(relation_id) = &delta.relation_id {
        let object_id = payload_string(payload, "objectId")
            .or_else(|| {
                delta
                    .object_json
                    .as_ref()
                    .and_then(|object| object.get("objectId"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .ok_or_else(|| invalid("relation canon delta requires payloadJson.objectId"))?;
        let relation_existed = row_exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM canon_relations WHERE project_id = ?1 AND relation_id = ?2)",
            libsql_rusqlite::params![project_id, relation_id],
        )?;
        connection.execute("INSERT INTO canon_relations(relation_id, project_id, subject_id, predicate, object_id, valid_from_chapter_id, valid_to_chapter_id, authority_level, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) ON CONFLICT(relation_id) DO UPDATE SET subject_id=excluded.subject_id, predicate=excluded.predicate, object_id=excluded.object_id, valid_from_chapter_id=excluded.valid_from_chapter_id, valid_to_chapter_id=excluded.valid_to_chapter_id, authority_level=excluded.authority_level, updated_at=excluded.updated_at", libsql_rusqlite::params![relation_id, project_id, subject_id, delta.predicate, object_id, delta.valid_from_chapter_id, delta.valid_to_chapter_id, delta.authority_level.as_ref().map(crate::repositories::artifacts::enum_string).transpose()?.unwrap_or_else(|| "C".into()), now])?;
        inserted += i64::from(!relation_existed);
    }
    if let Some(event_type) = &delta.event_type {
        let chapter_id = payload_string(payload, "chapterId")
            .or_else(|| delta.valid_from_chapter_id.clone())
            .ok_or_else(|| invalid("canon event delta requires payloadJson.chapterId"))?;
        let event_id = payload_string(payload, "eventId")
            .unwrap_or_else(|| format!("event:{}", delta.fact_id));
        let event_existed = row_exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM canon_events WHERE project_id = ?1 AND event_id = ?2)",
            libsql_rusqlite::params![project_id, event_id],
        )?;
        let story_time = payload_string(payload, "storyTime");
        let story_time_end = payload_string(payload, "storyTimeEnd");
        let event_ordinal = payload_i64(payload, "eventOrdinal").unwrap_or(0);
        let source_artifact_id = payload_string(payload, "sourceArtifactId");
        let source_version = payload_i64(payload, "sourceVersion");
        connection.execute("INSERT INTO canon_events(event_id, project_id, chapter_id, event_type, subject_id, object_json, story_time, event_ordinal, story_time_end, source_artifact_id, source_version, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) ON CONFLICT(event_id) DO UPDATE SET chapter_id=excluded.chapter_id, event_type=excluded.event_type, subject_id=excluded.subject_id, object_json=excluded.object_json, story_time=excluded.story_time, event_ordinal=excluded.event_ordinal, story_time_end=excluded.story_time_end, source_artifact_id=excluded.source_artifact_id, source_version=excluded.source_version, payload_json=excluded.payload_json", libsql_rusqlite::params![event_id, project_id, chapter_id, event_type, delta.subject_id, object_json, story_time, event_ordinal, story_time_end, source_artifact_id, source_version, map_json(payload)?, now])?;
        inserted += i64::from(!event_existed);
    }
    Ok(inserted)
}

fn delta_project(connection: &Connection) -> Result<String, super::CommitError> {
    connection
        .query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })
        .map_err(Into::into)
}

fn apply_obligation(
    connection: &Connection,
    delta: &ObligationDelta,
    fallback: Option<(&str, i64)>,
    now: &str,
) -> Result<i64, super::CommitError> {
    let project_id = delta_project(connection)?;
    let payload = delta
        .payload_json
        .as_ref()
        .map(|x| x as &BTreeMap<String, Value>);
    let (artifact_id, version) = source_artifact(payload, fallback)?;
    let existed = row_exists(connection, "SELECT EXISTS(SELECT 1 FROM narrative_obligations WHERE project_id = ?1 AND obligation_id = ?2)", libsql_rusqlite::params![project_id, delta.obligation_id])?;
    connection.execute("INSERT INTO narrative_obligations(obligation_id, project_id, scope, obligation_type, authority_level, source_artifact_id, source_version, description, must_resolve_before_chapter, prerequisites_json, lifecycle, decision_ref, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13) ON CONFLICT(obligation_id) DO UPDATE SET scope=excluded.scope, obligation_type=excluded.obligation_type, authority_level=excluded.authority_level, source_artifact_id=excluded.source_artifact_id, source_version=excluded.source_version, description=excluded.description, must_resolve_before_chapter=excluded.must_resolve_before_chapter, prerequisites_json=excluded.prerequisites_json, lifecycle=excluded.lifecycle, decision_ref=excluded.decision_ref, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.obligation_id, project_id, delta.scope.clone().unwrap_or_else(|| "CHAPTER".into()), delta.obligation_type, delta.authority_level.clone().unwrap_or_else(|| "C".into()), artifact_id, version, delta.description.clone().unwrap_or_else(|| delta.obligation_id.clone()), delta.must_resolve_before_chapter, serde_json::to_string(&delta.prerequisites.clone().unwrap_or_default())?, delta.lifecycle.clone().unwrap_or_else(|| "PROPOSED".into()), delta.decision_ref, now])?;
    Ok(i64::from(!existed))
}

fn apply_foreshadow(
    connection: &Connection,
    delta: &ForeshadowDelta,
    now: &str,
) -> Result<i64, super::CommitError> {
    let project_id = delta_project(connection)?;
    let payload = delta
        .payload_json
        .as_ref()
        .map(|x| x as &BTreeMap<String, Value>);
    let source_authority = payload_string(payload, "sourceAuthority").unwrap_or_else(|| "C".into());
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM foreshadows WHERE project_id = ?1 AND foreshadow_id = ?2)",
        libsql_rusqlite::params![project_id, delta.foreshadow_id],
    )?;
    connection.execute("INSERT INTO foreshadows(foreshadow_id, project_id, intent, secret_payload_ref, plant_chapter_id, recover_window_end_chapter, forbidden_reveal_before_chapter, status, source_authority, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10) ON CONFLICT(foreshadow_id) DO UPDATE SET intent=excluded.intent, secret_payload_ref=excluded.secret_payload_ref, plant_chapter_id=excluded.plant_chapter_id, recover_window_end_chapter=excluded.recover_window_end_chapter, forbidden_reveal_before_chapter=excluded.forbidden_reveal_before_chapter, status=excluded.status, source_authority=excluded.source_authority, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.foreshadow_id, project_id, delta.intent, delta.secret_payload_ref, delta.plant_chapter_id, delta.recover_window_end_chapter, delta.forbidden_reveal_before_chapter, delta.status.clone().unwrap_or_else(|| "PLANTED".into()), source_authority, now])?;
    Ok(i64::from(!existed))
}

fn apply_knowledge(
    connection: &Connection,
    delta: &KnowledgeDelta,
    now: &str,
) -> Result<i64, super::CommitError> {
    let project_id = delta_project(connection)?;
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM canon_knowledge WHERE project_id = ?1 AND knowledge_id = ?2)",
        libsql_rusqlite::params![project_id, delta.knowledge_id],
    )?;
    connection.execute("INSERT INTO canon_knowledge(knowledge_id, project_id, knower_id, fact_id, epistemic_state, visibility_scope, from_chapter_id, until_chapter_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) ON CONFLICT(knowledge_id) DO UPDATE SET knower_id=excluded.knower_id, fact_id=excluded.fact_id, epistemic_state=excluded.epistemic_state, visibility_scope=excluded.visibility_scope, from_chapter_id=excluded.from_chapter_id, until_chapter_id=excluded.until_chapter_id, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.knowledge_id, project_id, delta.knower_id, delta.fact_id, delta.epistemic_state, delta.visibility_scope, delta.from_chapter_id, delta.until_chapter_id, now])?;
    Ok(i64::from(!existed))
}

fn apply_state(
    connection: &Connection,
    delta: &StateDelta,
    now: &str,
) -> Result<i64, super::CommitError> {
    if delta.state_type.trim().is_empty() {
        return Err(invalid("state delta requires stateType"));
    }
    let project_id = delta_project(connection)?;
    let value_json = serde_json::to_value(delta.value_json.clone().unwrap_or_default())?;
    if delta.state_type == "volume.task.progress" {
        crate::generated::validators::validate_task_progress(&value_json).map_err(invalid)?;
        crate::narrative::validate_task_progress_semantics(&value_json).map_err(invalid)?;
    }
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM state_deltas WHERE project_id = ?1 AND state_id = ?2)",
        libsql_rusqlite::params![project_id, delta.state_id],
    )?;
    connection.execute("INSERT INTO state_deltas(state_id, project_id, state_type, value_json, chapter_id, payload_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7) ON CONFLICT(state_id) DO UPDATE SET state_type=excluded.state_type, value_json=excluded.value_json, chapter_id=excluded.chapter_id, payload_json=excluded.payload_json, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.state_id, project_id, delta.state_type, serde_json::to_string(&delta.value_json.clone().unwrap_or_default())?, delta.chapter_id, map_json(delta.payload_json.as_ref().map(|x| x as &BTreeMap<String, Value>))?, now])?;
    Ok(i64::from(!existed))
}

fn apply_dependency(
    connection: &Connection,
    delta: &DependencyEdge,
    now: &str,
) -> Result<i64, super::CommitError> {
    if delta.source_hash.len() != 64 || !delta.source_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid(
            "dependency sourceHash must be a 64-character hex digest",
        ));
    }
    let project_id = delta_project(connection)?;
    let payload = delta
        .payload_json
        .as_ref()
        .map(|x| x as &BTreeMap<String, Value>);
    let edge_id = delta.edge_id.clone().unwrap_or_else(|| {
        format!(
            "edge:{}",
            &sha256_canonical(
                &json!({"source":delta.source_ref,"target":delta.target_ref,"scope":delta.scope})
            )
            .unwrap_or_default()[..24]
        )
    });
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM dependency_edges WHERE project_id = ?1 AND edge_id = ?2)",
        libsql_rusqlite::params![project_id, edge_id],
    )?;
    connection.execute("INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, selector, entity_id, chapter_range_json, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) ON CONFLICT(edge_id) DO UPDATE SET source_ref=excluded.source_ref, target_ref=excluded.target_ref, dependency_type=excluded.dependency_type, scope=excluded.scope, impact_class=excluded.impact_class, source_hash=excluded.source_hash, selector=excluded.selector, entity_id=excluded.entity_id, chapter_range_json=excluded.chapter_range_json, created_by=excluded.created_by", libsql_rusqlite::params![edge_id, project_id, delta.source_ref, delta.target_ref, delta.dependency_type, delta.scope, delta.impact_class, delta.source_hash, delta.selector, delta.entity_id, payload_json(payload, "chapterRangeJson").unwrap_or_else(|| "{}".into()), payload_string(payload, "createdBy").unwrap_or_else(|| "CommitService".into()), now])?;
    Ok(i64::from(!existed))
}

fn apply_job(
    connection: &Connection,
    delta: &JobUpdate,
    now: &str,
) -> Result<i64, super::CommitError> {
    let project_id = delta_project(connection)?;
    let payload = delta
        .payload_json
        .as_ref()
        .map(|x| x as &BTreeMap<String, Value>);
    let job_type = payload_string(payload, "jobType").unwrap_or_else(|| "CHAPTER".into());
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM jobs WHERE project_id = ?1 AND job_id = ?2)",
        libsql_rusqlite::params![project_id, delta.job_id],
    )?;
    connection.execute("INSERT INTO jobs(job_id, project_id, job_type, status, current_stage, attempt_no, repair_round, error_code, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) ON CONFLICT(job_id) DO UPDATE SET status=COALESCE(?4,status), current_stage=COALESCE(?5,current_stage), attempt_no=COALESCE(?6,attempt_no), repair_round=COALESCE(?7,repair_round), error_code=?8, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.job_id, project_id, job_type, delta.status.clone().unwrap_or_else(|| "QUEUED".into()), delta.current_stage, delta.attempt_no.unwrap_or(0), delta.repair_round.unwrap_or(0), delta.error_code, now])?;
    Ok(i64::from(!existed))
}

fn apply_decision(
    connection: &Connection,
    delta: &DecisionUpdate,
    now: &str,
) -> Result<i64, super::CommitError> {
    if delta.decision_type.trim().is_empty() || delta.outcome.trim().is_empty() {
        return Err(invalid("decision delta requires decisionType and outcome"));
    }
    let project_id = delta_project(connection)?;
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM decisions WHERE project_id = ?1 AND decision_id = ?2)",
        libsql_rusqlite::params![project_id, delta.decision_id],
    )?;
    connection.execute("INSERT INTO decisions(decision_id, project_id, decision_type, outcome, reason, source_ref, payload_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) ON CONFLICT(decision_id) DO UPDATE SET decision_type=excluded.decision_type, outcome=excluded.outcome, reason=excluded.reason, source_ref=excluded.source_ref, payload_json=excluded.payload_json, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.decision_id, project_id, delta.decision_type, delta.outcome, delta.reason, delta.source_ref, map_json(delta.payload_json.as_ref().map(|x| x as &BTreeMap<String, Value>))?, now])?;
    Ok(i64::from(!existed))
}

fn apply_reader_desire(
    connection: &Connection,
    delta: &ReaderDesireDelta,
    now: &str,
) -> Result<i64, super::CommitError> {
    if delta.desire_type.trim().is_empty() {
        return Err(invalid("reader desire delta requires desireType"));
    }
    let project_id = delta_project(connection)?;
    let existed = row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM reader_desires WHERE project_id = ?1 AND desire_id = ?2)",
        libsql_rusqlite::params![project_id, delta.desire_id],
    )?;
    connection.execute("INSERT INTO reader_desires(desire_id, project_id, desire_type, status, expected_payoff_chapter, created_chapter_id, last_touched_chapter_id, payload_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) ON CONFLICT(desire_id) DO UPDATE SET desire_type=excluded.desire_type, status=excluded.status, expected_payoff_chapter=excluded.expected_payoff_chapter, created_chapter_id=excluded.created_chapter_id, last_touched_chapter_id=excluded.last_touched_chapter_id, payload_json=excluded.payload_json, updated_at=excluded.updated_at", libsql_rusqlite::params![delta.desire_id, project_id, delta.desire_type, delta.status.clone().unwrap_or_else(|| "OPEN".into()), delta.expected_payoff_chapter, delta.created_chapter_id, delta.last_touched_chapter_id, map_json(delta.payload_json.as_ref().map(|x| x as &BTreeMap<String, Value>))?, now])?;
    Ok(i64::from(!existed))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_all(
    connection: &Connection,
    canon: Option<&Vec<CanonDelta>>,
    obligations: Option<&Vec<ObligationDelta>>,
    foreshadows: Option<&Vec<ForeshadowDelta>>,
    knowledge: Option<&Vec<KnowledgeDelta>>,
    states: Option<&Vec<StateDelta>>,
    dependencies: Option<&Vec<DependencyEdge>>,
    jobs: Option<&Vec<JobUpdate>>,
    decisions: Option<&Vec<DecisionUpdate>>,
    reader_desires: Option<&Vec<ReaderDesireDelta>>,
    now: &str,
    fallback_artifact: Option<(&str, i64)>,
) -> Result<BTreeMap<String, i64>, super::CommitError> {
    let mut counts = BTreeMap::new();
    let mut canon_count = 0;
    if let Some(items) = canon {
        for item in items {
            canon_count += apply_canon(connection, item, now)?;
        }
    }
    counts.insert("canon".into(), canon_count);
    let mut obligation_count = 0;
    if let Some(items) = obligations {
        for item in items {
            obligation_count += apply_obligation(connection, item, fallback_artifact, now)?;
        }
    }
    counts.insert("obligation".into(), obligation_count);
    let mut foreshadow_count = 0;
    if let Some(items) = foreshadows {
        for item in items {
            foreshadow_count += apply_foreshadow(connection, item, now)?;
        }
    }
    counts.insert("foreshadow".into(), foreshadow_count);
    let mut knowledge_count = 0;
    if let Some(items) = knowledge {
        for item in items {
            knowledge_count += apply_knowledge(connection, item, now)?;
        }
    }
    counts.insert("knowledge".into(), knowledge_count);
    let mut state_count = 0;
    if let Some(items) = states {
        for item in items {
            state_count += apply_state(connection, item, now)?;
        }
    }
    counts.insert("state".into(), state_count);
    let mut dependency_count = 0;
    if let Some(items) = dependencies {
        for item in items {
            dependency_count += apply_dependency(connection, item, now)?;
        }
    }
    counts.insert("dependency".into(), dependency_count);
    let mut job_count = 0;
    if let Some(items) = jobs {
        for item in items {
            job_count += apply_job(connection, item, now)?;
        }
    }
    counts.insert("job".into(), job_count);
    let mut decision_count = 0;
    if let Some(items) = decisions {
        for item in items {
            decision_count += apply_decision(connection, item, now)?;
        }
    }
    counts.insert("decision".into(), decision_count);
    let mut reader_desire_count = 0;
    if let Some(items) = reader_desires {
        for item in items {
            reader_desire_count += apply_reader_desire(connection, item, now)?;
        }
    }
    counts.insert("readerDesire".into(), reader_desire_count);
    Ok(counts)
}

pub(crate) fn certificate_digest(
    certificate: &GateCertificate,
) -> Result<String, super::CommitError> {
    let mut value = serde_json::to_value(certificate)?;
    value["certificateDigest"] = Value::String(String::new());
    Ok(sha256_canonical(&value)?)
}

pub(crate) fn certificate_source_digest(
    certificate: &GateCertificate,
) -> Result<String, super::CommitError> {
    Ok(sha256_canonical(&json!({
        "gatePolicyRef": certificate.gate_policy_ref,
        "subjectRef": certificate.subject_ref,
        "evidenceRefs": certificate.evidence_refs
    }))?)
}
