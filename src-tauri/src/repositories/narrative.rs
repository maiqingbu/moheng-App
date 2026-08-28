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
pub struct CanonFactRecord {
    pub fact_id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_json: Value,
    pub authority_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeRecord {
    pub knowledge_id: String,
    pub knower_id: String,
    pub fact_id: Option<String>,
    pub epistemic_state: String,
    pub visibility_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObligationRecord {
    pub obligation_id: String,
    pub obligation_type: String,
    pub authority_level: String,
    pub lifecycle: String,
    pub source_event_id: Option<String>,
    pub resume_condition: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskStateRecord {
    pub state_id: String,
    pub value: Value,
    pub chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        let mut records = Vec::new();
        for entity_id in entity_ids.iter().take(Self::limit(limit) as usize) {
            let remaining = Self::limit(limit) - records.len() as i64;
            if remaining <= 0 {
                break;
            }
            let mut statement = connection.prepare("SELECT fact_id, subject_id, predicate, object_json, authority_level FROM canon_facts WHERE project_id=?1 AND subject_id=?2 AND superseded_by IS NULL ORDER BY fact_id LIMIT ?3")?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, entity_id, remaining],
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
        Ok(records)
    }

    pub fn knowledge_for_entities(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<KnowledgeRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT knowledge_id, knower_id, fact_id, epistemic_state, visibility_scope FROM canon_knowledge WHERE project_id=?1 AND knower_id IN (SELECT value FROM json_each(?2)) ORDER BY knowledge_id LIMIT ?3")?;
        let ids = serde_json::to_string(entity_ids)?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, ids, Self::limit(limit)],
            |row| {
                Ok(KnowledgeRecord {
                    knowledge_id: row.get(0)?,
                    knower_id: row.get(1)?,
                    fact_id: row.get(2)?,
                    epistemic_state: row.get(3)?,
                    visibility_scope: row.get(4)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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

    pub fn active_obligations(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<ObligationRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT obligation_id, obligation_type, authority_level, lifecycle, source_event_id, resume_condition_json FROM narrative_obligations WHERE project_id=?1 AND lifecycle IN ('APPROVED','PLANTED','PROGRESSING') ORDER BY updated_at DESC, obligation_id LIMIT ?2")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, Self::limit(limit)],
            |row| {
                let resume_condition: String = row.get(5)?;
                Ok(ObligationRecord {
                    obligation_id: row.get(0)?,
                    obligation_type: row.get(1)?,
                    authority_level: row.get(2)?,
                    lifecycle: row.get(3)?,
                    source_event_id: row.get(4)?,
                    resume_condition: serde_json::from_str(&resume_condition)
                        .unwrap_or(Value::Object(Default::default())),
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn task_states(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        limit: i64,
    ) -> Result<Vec<TaskStateRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare("SELECT state_id, value_json, chapter_id FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' AND json_extract(payload_json, '$.volumeId')=?2 ORDER BY updated_at DESC, state_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, volume_id, Self::limit(limit)],
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
}
