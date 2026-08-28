use crate::generated::types::ArtifactRef;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum NarrativeRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanonFactRecord {
    pub fact_id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_json: Value,
    pub authority_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRecord {
    pub knowledge_id: String,
    pub knower_id: String,
    pub fact_id: Option<String>,
    pub epistemic_state: String,
    pub visibility_scope: Option<String>,
    pub from_chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelationRecord {
    pub relation_id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_id: String,
    pub authority_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub fact_id: Option<String>,
    pub relation_id: Option<String>,
    pub event_id: Option<String>,
    pub final_chapter_id: String,
    pub source_artifact_id: String,
    pub version: i64,
    pub evidence_type: String,
    pub quote: Option<String>,
    pub range_start: Option<i64>,
    pub range_end: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForeshadowRecord {
    pub foreshadow_id: String,
    pub intent: String,
    pub status: String,
    pub plant_chapter_id: Option<String>,
    pub recover_window_end_chapter: Option<String>,
    pub secret_payload_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MysteryRecord {
    pub mystery_id: String,
    pub mystery_type: String,
    pub status: String,
    pub core_fact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObligationRecord {
    pub obligation_id: String,
    pub obligation_type: String,
    pub authority_level: String,
    pub lifecycle: String,
    pub source_event_id: Option<String>,
    pub resume_condition: Value,
    pub participant_entities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskStateRecord {
    pub state_id: String,
    pub value: Value,
    pub chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanonEventRecord {
    pub event_id: String,
    pub chapter_id: String,
    pub event_type: String,
    pub story_time: Option<String>,
    pub event_ordinal: i64,
}

pub struct NarrativeRepository;

impl NarrativeRepository {
    fn limit(limit: i64) -> i64 {
        limit.clamp(1, 100)
    }

    fn ids_json(ids: &[&str]) -> Result<String, NarrativeRepositoryError> {
        Ok(serde_json::to_string(ids)?)
    }

    pub fn goal_contract_refs(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<ArtifactRef>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type IN ('StoryConstitution','BookPlan','MainContract','VolumeIntent') ORDER BY artifact_type, scope_key LIMIT ?2")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, Self::limit(limit)],
            |row| {
                Ok(ArtifactRef {
                    artifact_id: row.get(0)?,
                    version: row.get(1)?,
                    payload_hash: row.get(2)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn current_canon(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<CanonFactRecord>, NarrativeRepositoryError> {
        Ok(Self::current_canon_bounded(connection, project_id, entity_ids, limit)?.0)
    }

    pub fn current_canon_bounded(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<CanonFactRecord>, bool), NarrativeRepositoryError> {
        let mut records = Vec::new();
        let bound = Self::limit(limit);
        for entity_id in entity_ids.iter().take(bound as usize) {
            let remaining = bound - records.len() as i64;
            if remaining <= 0 {
                break;
            }
            let mut statement = connection.prepare("SELECT fact_id, subject_id, predicate, object_json, authority_level FROM canon_facts WHERE project_id=?1 AND subject_id=?2 AND superseded_by IS NULL ORDER BY fact_id LIMIT ?3")?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, entity_id, remaining + 1],
                |row| {
                    let object_json: String = row.get(3)?;
                    Ok(CanonFactRecord {
                        fact_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        predicate: row.get(2)?,
                        object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                        authority_level: row.get(4)?,
                    })
                },
            )?;
            records.extend(rows.collect::<Result<Vec<_>, _>>()?);
        }
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn current_relations(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<RelationRecord>, NarrativeRepositoryError> {
        let ids = Self::ids_json(entity_ids)?;
        let mut statement = connection.prepare("SELECT relation_id, subject_id, predicate, object_id, authority_level FROM canon_relations WHERE project_id=?1 AND (subject_id IN (SELECT value FROM json_each(?2)) OR object_id IN (SELECT value FROM json_each(?2))) ORDER BY relation_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, ids, Self::limit(limit)],
            |row| {
                Ok(RelationRecord {
                    relation_id: row.get(0)?,
                    subject_id: row.get(1)?,
                    predicate: row.get(2)?,
                    object_id: row.get(3)?,
                    authority_level: row.get(4)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn canon_evidence_by_ref(
        connection: &Connection,
        project_id: &str,
        reference: &str,
        limit: i64,
    ) -> Result<Vec<EvidenceRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT evidence_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end FROM canon_evidence WHERE project_id=?1 AND (fact_id=?2 OR relation_id=?2 OR event_id=?2 OR source_artifact_id=?2) ORDER BY created_at DESC, evidence_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, reference, Self::limit(limit)],
            |row| {
                Ok(EvidenceRecord {
                    evidence_id: row.get(0)?,
                    fact_id: row.get(1)?,
                    relation_id: row.get(2)?,
                    event_id: row.get(3)?,
                    final_chapter_id: row.get(4)?,
                    source_artifact_id: row.get(5)?,
                    version: row.get(6)?,
                    evidence_type: row.get(7)?,
                    quote: row.get(8)?,
                    range_start: row.get(9)?,
                    range_end: row.get(10)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn knowledge_for_entities(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<KnowledgeRecord>, NarrativeRepositoryError> {
        Ok(Self::knowledge_for_entities_bounded(connection, project_id, entity_ids, limit)?.0)
    }

    pub fn knowledge_for_entities_bounded(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<KnowledgeRecord>, bool), NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT knowledge_id, knower_id, fact_id, epistemic_state, visibility_scope, from_chapter_id FROM canon_knowledge WHERE project_id=?1 AND knower_id IN (SELECT value FROM json_each(?2)) ORDER BY knowledge_id LIMIT ?3")?;
        let ids = serde_json::to_string(entity_ids)?;
        let bound = Self::limit(limit);
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, ids, bound + 1],
            |row| {
                Ok(KnowledgeRecord {
                    knowledge_id: row.get(0)?,
                    knower_id: row.get(1)?,
                    fact_id: row.get(2)?,
                    epistemic_state: row.get(3)?,
                    visibility_scope: row.get(4)?,
                    from_chapter_id: row.get(5)?,
                })
            },
        )?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn canon_events(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<Vec<CanonEventRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT event_id, chapter_id, event_type, story_time, event_ordinal FROM canon_events WHERE project_id=?1 AND chapter_id=?2 ORDER BY event_ordinal, event_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapter_id, Self::limit(limit)],
            |row| {
                Ok(CanonEventRecord {
                    event_id: row.get(0)?,
                    chapter_id: row.get(1)?,
                    event_type: row.get(2)?,
                    story_time: row.get(3)?,
                    event_ordinal: row.get(4)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn active_obligations_for_relevance(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        references: &[&str],
        limit: i64,
    ) -> Result<Vec<ObligationRecord>, NarrativeRepositoryError> {
        let entity_ids = Self::ids_json(entity_ids)?;
        let references = Self::ids_json(references)?;
        let mut statement = connection.prepare("SELECT o.obligation_id, o.obligation_type, o.authority_level, o.lifecycle, o.source_event_id, o.resume_condition_json, (SELECT COALESCE(json_group_array(d.entity_id), '[]') FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IS NOT NULL) FROM narrative_obligations AS o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND (o.obligation_id IN (SELECT value FROM json_each(?2)) OR o.source_event_id IN (SELECT value FROM json_each(?2)) OR EXISTS (SELECT 1 FROM dependency_edges AS d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IN (SELECT value FROM json_each(?3)))) ORDER BY o.updated_at DESC, o.obligation_id LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, references, entity_ids, Self::limit(limit)],
            |row| {
                let resume_condition: String = row.get(5)?;
                let participants: String = row.get(6)?;
                Ok(ObligationRecord {
                    obligation_id: row.get(0)?,
                    obligation_type: row.get(1)?,
                    authority_level: row.get(2)?,
                    lifecycle: row.get(3)?,
                    source_event_id: row.get(4)?,
                    resume_condition: serde_json::from_str(&resume_condition)
                        .unwrap_or(Value::Object(Default::default())),
                    participant_entities: serde_json::from_str(&participants).unwrap_or_default(),
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn active_foreshadows(
        connection: &Connection,
        project_id: &str,
        chapter_refs: &[&str],
        responsibility_refs: &[&str],
        limit: i64,
    ) -> Result<Vec<ForeshadowRecord>, NarrativeRepositoryError> {
        let chapters = Self::ids_json(chapter_refs)?;
        let refs = Self::ids_json(responsibility_refs)?;
        let mut statement = connection.prepare("SELECT foreshadow_id, intent, status, plant_chapter_id, recover_window_end_chapter, secret_payload_ref FROM foreshadows WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE') AND (plant_chapter_id IN (SELECT value FROM json_each(?2)) OR recover_window_end_chapter IN (SELECT value FROM json_each(?2)) OR foreshadow_id IN (SELECT value FROM json_each(?3)) OR secret_payload_ref IN (SELECT value FROM json_each(?3))) ORDER BY updated_at DESC, foreshadow_id LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapters, refs, Self::limit(limit)],
            |row| {
                Ok(ForeshadowRecord {
                    foreshadow_id: row.get(0)?,
                    intent: row.get(1)?,
                    status: row.get(2)?,
                    plant_chapter_id: row.get(3)?,
                    recover_window_end_chapter: row.get(4)?,
                    secret_payload_ref: row.get(5)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn open_mysteries(
        connection: &Connection,
        project_id: &str,
        references: &[&str],
        limit: i64,
    ) -> Result<Vec<MysteryRecord>, NarrativeRepositoryError> {
        let refs = Self::ids_json(references)?;
        let mut statement = connection.prepare("SELECT mystery_id, mystery_type, status, core_fact_id FROM mystery_states WHERE project_id=?1 AND status IN ('OPEN','PROGRESSING') AND (mystery_id IN (SELECT value FROM json_each(?2)) OR core_fact_id IN (SELECT value FROM json_each(?2))) ORDER BY updated_at DESC, mystery_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, refs, Self::limit(limit)],
            |row| {
                Ok(MysteryRecord {
                    mystery_id: row.get(0)?,
                    mystery_type: row.get(1)?,
                    status: row.get(2)?,
                    core_fact_id: row.get(3)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn task_states_by_ids(
        connection: &Connection,
        project_id: &str,
        task_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<TaskStateRecord>, NarrativeRepositoryError> {
        let task_ids = Self::ids_json(task_ids)?;
        let mut statement = connection.prepare("SELECT state_id, value_json, chapter_id FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' AND state_id IN (SELECT value FROM json_each(?2)) ORDER BY updated_at DESC, state_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, task_ids, Self::limit(limit)],
            |row| {
                let value: String = row.get(1)?;
                Ok(TaskStateRecord {
                    state_id: row.get(0)?,
                    value: serde_json::from_str(&value)
                        .unwrap_or(Value::Object(Default::default())),
                    chapter_id: row.get(2)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn recent_evidence(
        connection: &Connection,
        project_id: &str,
        chapter_ids: &[&str],
        source_refs: &[&str],
        limit: i64,
    ) -> Result<Vec<EvidenceRecord>, NarrativeRepositoryError> {
        Ok(
            Self::recent_evidence_bounded(connection, project_id, chapter_ids, source_refs, limit)?
                .0,
        )
    }

    pub fn recent_evidence_bounded(
        connection: &Connection,
        project_id: &str,
        chapter_ids: &[&str],
        source_refs: &[&str],
        limit: i64,
    ) -> Result<(Vec<EvidenceRecord>, bool), NarrativeRepositoryError> {
        let chapters = Self::ids_json(chapter_ids)?;
        let refs = Self::ids_json(source_refs)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT evidence_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end FROM canon_evidence WHERE project_id=?1 AND (final_chapter_id IN (SELECT value FROM json_each(?2)) OR source_artifact_id IN (SELECT value FROM json_each(?3))) ORDER BY created_at DESC, evidence_id LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapters, refs, bound + 1],
            |row| {
                Ok(EvidenceRecord {
                    evidence_id: row.get(0)?,
                    fact_id: row.get(1)?,
                    relation_id: row.get(2)?,
                    event_id: row.get(3)?,
                    final_chapter_id: row.get(4)?,
                    source_artifact_id: row.get(5)?,
                    version: row.get(6)?,
                    evidence_type: row.get(7)?,
                    quote: row.get(8)?,
                    range_start: row.get(9)?,
                    range_end: row.get(10)?,
                })
            },
        )?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn evidence_for_refs_bounded(
        connection: &Connection,
        project_id: &str,
        references: &[&str],
        limit: i64,
    ) -> Result<(Vec<EvidenceRecord>, bool), NarrativeRepositoryError> {
        let refs = Self::ids_json(references)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT evidence_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end FROM canon_evidence WHERE project_id=?1 AND (fact_id IN (SELECT value FROM json_each(?2)) OR relation_id IN (SELECT value FROM json_each(?2)) OR event_id IN (SELECT value FROM json_each(?2)) OR source_artifact_id IN (SELECT value FROM json_each(?2))) ORDER BY created_at DESC, evidence_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, refs, bound + 1],
            |row| {
                Ok(EvidenceRecord {
                    evidence_id: row.get(0)?,
                    fact_id: row.get(1)?,
                    relation_id: row.get(2)?,
                    event_id: row.get(3)?,
                    final_chapter_id: row.get(4)?,
                    source_artifact_id: row.get(5)?,
                    version: row.get(6)?,
                    evidence_type: row.get(7)?,
                    quote: row.get(8)?,
                    range_start: row.get(9)?,
                    range_end: row.get(10)?,
                })
            },
        )?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn task_states_bounded(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<TaskStateRecord>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT state_id, value_json, chapter_id FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' ORDER BY updated_at DESC, state_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| {
            let value: String = row.get(1)?;
            Ok(TaskStateRecord {
                state_id: row.get(0)?,
                value: serde_json::from_str(&value).unwrap_or(Value::Object(Default::default())),
                chapter_id: row.get(2)?,
            })
        })?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn artifact_payload(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
        scope_key: &str,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 AND h.scope_key=?3", libsql_rusqlite::params![project_id, artifact_type, scope_key], |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) }).optional().map_err(Into::into)
    }

    pub fn dependency_edges_for_entity(
        connection: &Connection,
        project_id: &str,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT edge_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash FROM dependency_edges WHERE project_id=?1 AND entity_id=?2 ORDER BY edge_id LIMIT ?3")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, entity_id, Self::limit(limit)], |row| Ok(serde_json::json!({"edgeId":row.get::<_,String>(0)?,"sourceRef":row.get::<_,String>(1)?,"targetRef":row.get::<_,String>(2)?,"dependencyType":row.get::<_,String>(3)?,"scope":row.get::<_,String>(4)?,"impactClass":row.get::<_,String>(5)?,"sourceHash":row.get::<_,String>(6)?})))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn volume_plan_route(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        connection.query_row("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumePlan' AND scope_key=?2", libsql_rusqlite::params![project_id, volume_id], |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? })).optional().map_err(Into::into)
    }

    pub fn latest_artifact_payload(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 ORDER BY h.scope_key LIMIT 1", libsql_rusqlite::params![project_id, artifact_type], |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) }).optional().map_err(Into::into)
    }
}
