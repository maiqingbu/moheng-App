use crate::generated::types::ArtifactRef;
use crate::narrative::{build_continuity_threads, ContinuityThreadView, ResumeContext};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const TASK_STATE_CHUNK_SIZE: usize = 100;
pub const CONTINUITY_SOURCE_CHUNK_SIZE: i64 = 100;
pub const TASK_STATES_BY_IDS_SQL: &str = "SELECT state_id, value_json, chapter_id FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' AND state_id IN (SELECT value FROM json_each(?2)) ORDER BY updated_at DESC, state_id LIMIT ?3";
pub const ARTIFACT_PAYLOAD_BY_REF_SQL: &str = "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.version=?3 AND a.payload_hash=?4";

#[derive(Debug, thiserror::Error)]
pub enum NarrativeRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("continuity projection: {0}")]
    ContinuityProjection(String),
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
pub struct SemanticCanonFactRecord {
    pub fact_id: String,
    pub object_json: Value,
    pub valid_from_chapter_id: Option<String>,
    pub valid_to_chapter_id: Option<String>,
    pub story_time_from: Option<String>,
    pub story_time_to: Option<String>,
    pub valid_from_chapter_no: Option<i64>,
    pub valid_to_chapter_no: Option<i64>,
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
    #[serde(default)]
    pub from_story_time: Option<String>,
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
pub struct ObligationAuthorityRecord {
    pub obligation_type: String,
    pub scope: String,
    pub authority_level: String,
    pub description: String,
    pub must_resolve_before_chapter: Option<String>,
    pub prerequisites: Value,
    pub decision_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForeshadowAuthorityRecord {
    pub intent: String,
    pub status: String,
    pub plant_chapter_id: Option<String>,
    pub recover_window_end_chapter: Option<String>,
    pub secret_payload_ref: Option<String>,
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
    /// Builds the complete, formal B06 continuity projection for a project.
    /// Consumers such as B14 receive thread state, not a second obligation
    /// count, so current logical status remains owned by the narrative layer.
    pub fn continuity_threads_complete(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<ContinuityThreadView>, NarrativeRepositoryError> {
        let current_chapter: i64 = connection.query_row(
            "SELECT COALESCE(MAX(chapter_no), 0) FROM chapters WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let current_volume = connection
            .query_row(
                "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no DESC, volume_id DESC LIMIT 1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let mut statement = connection.prepare(
            "SELECT o.obligation_id,o.obligation_type,o.authority_level,o.lifecycle,o.source_event_id,o.prerequisites_json,(SELECT COALESCE(json_group_array(d.entity_id), '[]') FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IS NOT NULL) FROM narrative_obligations o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING','RESOLVED','ABANDONED') ORDER BY o.obligation_id",
        )?;
        let mut obligations = Vec::new();
        for row in statement.query_map([project_id], |row| {
            let resume_condition: String = row.get(5)?;
            let participants: String = row.get(6)?;
            Ok(ObligationRecord {
                obligation_id: row.get(0)?,
                obligation_type: row.get(1)?,
                authority_level: row.get(2)?,
                lifecycle: row.get(3)?,
                source_event_id: row.get(4)?,
                resume_condition: serde_json::from_str(&resume_condition)
                    .unwrap_or_else(|_| Value::Object(Default::default())),
                participant_entities: serde_json::from_str(&participants).unwrap_or_default(),
            })
        })? {
            obligations.push(row?);
        }
        let foreshadows = Self::active_foreshadows_complete(connection, project_id)?;
        let mysteries = Self::open_mysteries_complete(connection, project_id)?;
        let evidence = Self::recent_evidence_complete(connection, project_id)?;
        build_continuity_threads(
            &obligations,
            &foreshadows,
            &mysteries,
            &evidence,
            &ResumeContext {
                current_chapter,
                current_volume,
                ..Default::default()
            },
        )
        .map_err(NarrativeRepositoryError::ContinuityProjection)
    }

    fn volume_scope_key(volume_id: &str) -> Result<String, NarrativeRepositoryError> {
        let bytes = crate::hash::canonical_bytes(
            &serde_json::json!({"level":"VOLUME","volumeId":volume_id}),
        )?;
        String::from_utf8(bytes).map_err(|error| {
            NarrativeRepositoryError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error,
            )))
        })
    }
    fn chapter_scope_key(chapter_id: &str) -> Result<String, NarrativeRepositoryError> {
        let bytes = crate::hash::canonical_bytes(
            &serde_json::json!({"level":"CHAPTER","chapterId":chapter_id}),
        )?;
        String::from_utf8(bytes).map_err(|error| {
            NarrativeRepositoryError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error,
            )))
        })
    }
    fn limit(limit: i64) -> i64 {
        limit.clamp(1, 100)
    }

    fn ids_json(ids: &[&str]) -> Result<String, NarrativeRepositoryError> {
        Ok(serde_json::to_string(ids)?)
    }

    pub fn project_revision(
        connection: &Connection,
        project_id: &str,
    ) -> Result<i64, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn canon_entity_exists(
        connection: &Connection,
        project_id: &str,
        entity_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_entities WHERE project_id=?1 AND entity_id=?2)",
                libsql_rusqlite::params![project_id, entity_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn current_canon_facts_for_semantic_key(
        connection: &Connection,
        project_id: &str,
        subject_id: &str,
        predicate: &str,
    ) -> Result<Vec<SemanticCanonFactRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT f.fact_id, f.object_json, f.valid_from_chapter_id, f.valid_to_chapter_id, f.story_time_from, f.story_time_to, from_chapter.chapter_no, to_chapter.chapter_no FROM canon_facts f LEFT JOIN chapters from_chapter ON from_chapter.project_id=f.project_id AND from_chapter.chapter_id=f.valid_from_chapter_id LEFT JOIN chapters to_chapter ON to_chapter.project_id=f.project_id AND to_chapter.chapter_id=f.valid_to_chapter_id WHERE f.project_id=?1 AND f.subject_id=?2 AND f.predicate=?3 AND f.superseded_by IS NULL ORDER BY f.fact_id",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, subject_id, predicate],
            |row| {
                let object_json: String = row.get(1)?;
                Ok(SemanticCanonFactRecord {
                    fact_id: row.get(0)?,
                    object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                    valid_from_chapter_id: row.get(2)?,
                    valid_to_chapter_id: row.get(3)?,
                    story_time_from: row.get(4)?,
                    story_time_to: row.get(5)?,
                    valid_from_chapter_no: row.get(6)?,
                    valid_to_chapter_no: row.get(7)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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

    pub fn goal_contract_refs_for_volume(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        limit: i64,
    ) -> Result<Vec<ArtifactRef>, NarrativeRepositoryError> {
        let canonical_scope = Self::volume_scope_key(volume_id)?;
        let mut statement = connection.prepare("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND (artifact_type IN ('StoryConstitution','BookPlan','MainContract') OR (artifact_type='VolumeIntent' AND scope_key IN (?2, ?3))) ORDER BY artifact_type, scope_key LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, canonical_scope, volume_id, Self::limit(limit)],
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

    /// Returns only already committed chapters immediately preceding the
    /// requested opening chapter. The bounded window prevents opening
    /// bootstrap from widening into a whole-book history scan.
    pub fn previous_committed_chapter_ids(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<Vec<String>, NarrativeRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT previous.chapter_id FROM chapters current JOIN chapters previous ON previous.project_id=current.project_id AND previous.volume_id=current.volume_id WHERE current.project_id=?1 AND current.chapter_id=?2 AND previous.chapter_no < current.chapter_no AND previous.status='COMMITTED' ORDER BY previous.chapter_no DESC, previous.chapter_id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapter_id, Self::limit(limit)],
            |row| row.get(0),
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn canon_facts_for_chapters_bounded(
        connection: &Connection,
        project_id: &str,
        chapter_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<CanonFactRecord>, bool), NarrativeRepositoryError> {
        let ids = Self::ids_json(chapter_ids)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare(
            "SELECT fact_id, subject_id, predicate, object_json, authority_level FROM canon_facts WHERE project_id=?1 AND superseded_by IS NULL AND (source_chapter_id IN (SELECT value FROM json_each(?2)) OR valid_from_chapter_id IN (SELECT value FROM json_each(?2))) ORDER BY fact_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, ids, bound + 1],
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
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn knowledge_for_chapters_bounded(
        connection: &Connection,
        project_id: &str,
        chapter_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<KnowledgeRecord>, bool), NarrativeRepositoryError> {
        let ids = Self::ids_json(chapter_ids)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare(
            "SELECT k.knowledge_id, k.knower_id, k.fact_id, k.epistemic_state, k.visibility_scope, k.from_chapter_id, COALESCE((SELECT tp.story_time FROM canon_timepoints tp WHERE tp.project_id=k.project_id AND tp.chapter_id=k.from_chapter_id), (SELECT e.story_time FROM canon_evidence ce JOIN canon_events e ON e.project_id=ce.project_id AND e.event_id=ce.event_id WHERE ce.project_id=k.project_id AND ce.fact_id=k.fact_id ORDER BY e.event_ordinal, e.event_id, ce.evidence_id LIMIT 1)) FROM canon_knowledge k WHERE k.project_id=?1 AND k.from_chapter_id IN (SELECT value FROM json_each(?2)) ORDER BY k.knowledge_id LIMIT ?3",
        )?;
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
                    from_story_time: row.get(6)?,
                })
            },
        )?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn canon_events_for_chapters_bounded(
        connection: &Connection,
        project_id: &str,
        chapter_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<CanonEventRecord>, bool), NarrativeRepositoryError> {
        let ids = Self::ids_json(chapter_ids)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare(
            "SELECT event_id, chapter_id, event_type, story_time, event_ordinal FROM canon_events WHERE project_id=?1 AND chapter_id IN (SELECT value FROM json_each(?2)) ORDER BY chapter_id DESC, event_ordinal, event_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, ids, bound + 1],
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
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
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

    pub fn has_more_relations(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<bool, NarrativeRepositoryError> {
        let ids = Self::ids_json(entity_ids)?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM canon_relations WHERE project_id=?1 AND (subject_id IN (SELECT value FROM json_each(?2)) OR object_id IN (SELECT value FROM json_each(?2))) LIMIT 1 OFFSET ?3)", libsql_rusqlite::params![project_id, ids, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
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
        let mut statement = connection.prepare("SELECT k.knowledge_id, k.knower_id, k.fact_id, k.epistemic_state, k.visibility_scope, k.from_chapter_id, COALESCE((SELECT tp.story_time FROM canon_timepoints tp WHERE tp.project_id=k.project_id AND tp.chapter_id=k.from_chapter_id), (SELECT e.story_time FROM canon_evidence ce JOIN canon_events e ON e.project_id=ce.project_id AND e.event_id=ce.event_id WHERE ce.project_id=k.project_id AND ce.fact_id=k.fact_id ORDER BY e.event_ordinal, e.event_id, ce.evidence_id LIMIT 1)) FROM canon_knowledge k WHERE k.project_id=?1 AND k.knower_id IN (SELECT value FROM json_each(?2)) ORDER BY k.knowledge_id LIMIT ?3")?;
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
                    from_story_time: row.get(6)?,
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

    pub fn canon_events_bounded(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<(Vec<CanonEventRecord>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT event_id, chapter_id, event_type, story_time, event_ordinal FROM canon_events WHERE project_id=?1 AND chapter_id=?2 ORDER BY event_ordinal, event_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapter_id, bound + 1],
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
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn establishing_event_for_knowledge(
        connection: &Connection,
        project_id: &str,
        knowledge: &KnowledgeRecord,
    ) -> Result<Option<(CanonEventRecord, EvidenceRecord)>, NarrativeRepositoryError> {
        let Some(fact_id) = knowledge.fact_id.as_deref() else {
            return Ok(None);
        };
        connection.query_row("SELECT e.event_id, e.chapter_id, e.event_type, e.story_time, e.event_ordinal, ce.evidence_id, ce.fact_id, ce.relation_id, ce.event_id, ce.final_chapter_id, ce.source_artifact_id, ce.source_version, ce.evidence_type, ce.quote, ce.range_start, ce.range_end FROM canon_evidence ce JOIN canon_events e ON e.project_id=ce.project_id AND e.event_id=ce.event_id WHERE ce.project_id=?1 AND ce.fact_id=?2 ORDER BY e.event_ordinal, e.event_id, ce.evidence_id LIMIT 1", libsql_rusqlite::params![project_id, fact_id], |row| {
            Ok((CanonEventRecord { event_id: row.get(0)?, chapter_id: row.get(1)?, event_type: row.get(2)?, story_time: row.get(3)?, event_ordinal: row.get(4)? }, EvidenceRecord { evidence_id: row.get(5)?, fact_id: row.get(6)?, relation_id: row.get(7)?, event_id: row.get(8)?, final_chapter_id: row.get(9)?, source_artifact_id: row.get(10)?, version: row.get(11)?, evidence_type: row.get(12)?, quote: row.get(13)?, range_start: row.get(14)?, range_end: row.get(15)? }))
        }).optional().map_err(Into::into)
    }

    pub fn dependency_edges_for_entities_bounded(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        limit: i64,
    ) -> Result<(Vec<Value>, bool), NarrativeRepositoryError> {
        let ids = Self::ids_json(entity_ids)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT edge_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash FROM dependency_edges WHERE project_id=?1 AND entity_id IN (SELECT value FROM json_each(?2)) ORDER BY edge_id LIMIT ?3")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, ids, bound + 1], |row| Ok(serde_json::json!({"edgeId":row.get::<_,String>(0)?,"sourceRef":row.get::<_,String>(1)?,"targetRef":row.get::<_,String>(2)?,"dependencyType":row.get::<_,String>(3)?,"scope":row.get::<_,String>(4)?,"impactClass":row.get::<_,String>(5)?,"sourceHash":row.get::<_,String>(6)?})))?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn model_runs_bounded(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<Value>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT model_run_id, provider, model, capability, status, predicted_cost_cents, reserved_cost_cents FROM model_runs WHERE project_id=?1 ORDER BY updated_at DESC, model_run_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| Ok(serde_json::json!({"modelRunId":row.get::<_,String>(0)?,"provider":row.get::<_,String>(1)?,"model":row.get::<_,String>(2)?,"capability":row.get::<_,String>(3)?,"status":row.get::<_,String>(4)?,"predictedCostCents":row.get::<_,i64>(5)?,"reservedCostCents":row.get::<_,i64>(6)?})))?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn commit_receipts_bounded(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<Value>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT receipt_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_digest, committed_at FROM commit_receipts WHERE project_id=?1 ORDER BY committed_at DESC, receipt_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| Ok(serde_json::json!({"receiptId":row.get::<_,String>(0)?,"commitId":row.get::<_,String>(1)?,"commandId":row.get::<_,String>(2)?,"idempotencyKey":row.get::<_,String>(3)?,"beforeRevision":row.get::<_,i64>(4)?,"afterRevision":row.get::<_,i64>(5)?,"bundleDigest":row.get::<_,String>(6)?,"receiptDigest":row.get::<_,String>(7)?,"committedAt":row.get::<_,String>(8)?})))?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn integrity_status_bounded(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<Value>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT integrity_run_id, status, failure_count, report_json, created_at FROM integrity_runs WHERE project_id=?1 ORDER BY created_at DESC, integrity_run_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| Ok(serde_json::json!({"integrityRunId":row.get::<_,String>(0)?,"status":row.get::<_,String>(1)?,"failureCount":row.get::<_,i64>(2)?,"report":serde_json::from_str::<Value>(&row.get::<_,String>(3)?).unwrap_or(Value::Null),"createdAt":row.get::<_,String>(4)?})))?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = records.len() as i64 > bound;
        records.truncate(bound as usize);
        Ok((records, truncated))
    }

    pub fn has_more_events(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection.query_row("SELECT EXISTS(SELECT 1 FROM canon_events WHERE project_id=?1 AND chapter_id=?2 LIMIT 1 OFFSET ?3)", libsql_rusqlite::params![project_id, chapter_id, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
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
        let mut statement = connection.prepare("SELECT o.obligation_id, o.obligation_type, o.authority_level, o.lifecycle, o.source_event_id, o.resume_condition_json, (SELECT COALESCE(json_group_array(d.entity_id), '[]') FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IS NOT NULL) FROM narrative_obligations AS o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND ((o.obligation_id IN (SELECT value FROM json_each(?2)) OR o.source_event_id IN (SELECT value FROM json_each(?2)) OR EXISTS (SELECT 1 FROM dependency_edges AS d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IN (SELECT value FROM json_each(?3)))) OR (?2='[]' AND ?3='[]')) ORDER BY o.updated_at DESC, o.obligation_id LIMIT ?4")?;
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

    pub fn has_more_obligations(
        connection: &Connection,
        project_id: &str,
        entity_ids: &[&str],
        references: &[&str],
        limit: i64,
    ) -> Result<bool, NarrativeRepositoryError> {
        let entity_ids = Self::ids_json(entity_ids)?;
        let references = Self::ids_json(references)?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM narrative_obligations AS o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND ((o.obligation_id IN (SELECT value FROM json_each(?2)) OR o.source_event_id IN (SELECT value FROM json_each(?2)) OR EXISTS (SELECT 1 FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IN (SELECT value FROM json_each(?3)))) OR (?2='[]' AND ?3='[]')) LIMIT 1 OFFSET ?4)", libsql_rusqlite::params![project_id, references, entity_ids, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
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
        let mut statement = connection.prepare("SELECT foreshadow_id, intent, status, plant_chapter_id, recover_window_end_chapter, secret_payload_ref FROM foreshadows WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE') AND ((plant_chapter_id IN (SELECT value FROM json_each(?2)) OR recover_window_end_chapter IN (SELECT value FROM json_each(?2)) OR foreshadow_id IN (SELECT value FROM json_each(?3)) OR secret_payload_ref IN (SELECT value FROM json_each(?3))) OR (?2='[]' AND ?3='[]')) ORDER BY updated_at DESC, foreshadow_id LIMIT ?4")?;
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

    /// Reads the formal Continuity foreshadow source completely without
    /// widening the shared bounded reader. Each page is indexed, project
    /// scoped, and capped at the existing 100-row contract.
    pub fn active_foreshadows_complete(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<ForeshadowRecord>, NarrativeRepositoryError> {
        let mut result = Vec::new();
        let mut offset = 0_i64;
        loop {
            let mut statement = connection.prepare(
                "SELECT foreshadow_id, intent, status, plant_chapter_id, recover_window_end_chapter, secret_payload_ref FROM foreshadows WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE') ORDER BY updated_at DESC, foreshadow_id LIMIT ?2 OFFSET ?3",
            )?;
            let page = statement
                .query_map(
                    libsql_rusqlite::params![project_id, CONTINUITY_SOURCE_CHUNK_SIZE, offset],
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
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let page_len = page.len() as i64;
            result.extend(page);
            if page_len < CONTINUITY_SOURCE_CHUNK_SIZE {
                break;
            }
            offset += CONTINUITY_SOURCE_CHUNK_SIZE;
        }
        Ok(result)
    }

    pub fn has_more_foreshadows(
        connection: &Connection,
        project_id: &str,
        chapter_refs: &[&str],
        responsibility_refs: &[&str],
        limit: i64,
    ) -> Result<bool, NarrativeRepositoryError> {
        let chapters = Self::ids_json(chapter_refs)?;
        let refs = Self::ids_json(responsibility_refs)?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM foreshadows WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE') AND (plant_chapter_id IN (SELECT value FROM json_each(?2)) OR recover_window_end_chapter IN (SELECT value FROM json_each(?2)) OR foreshadow_id IN (SELECT value FROM json_each(?3)) OR secret_payload_ref IN (SELECT value FROM json_each(?3))) LIMIT 1 OFFSET ?4)", libsql_rusqlite::params![project_id, chapters, refs, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
    }

    pub fn open_mysteries(
        connection: &Connection,
        project_id: &str,
        references: &[&str],
        limit: i64,
    ) -> Result<Vec<MysteryRecord>, NarrativeRepositoryError> {
        let refs = Self::ids_json(references)?;
        let mut statement = connection.prepare("SELECT mystery_id, mystery_type, status, core_fact_id FROM mystery_states WHERE project_id=?1 AND status IN ('OPEN','PROGRESSING') AND ((mystery_id IN (SELECT value FROM json_each(?2)) OR core_fact_id IN (SELECT value FROM json_each(?2))) OR ?2='[]') ORDER BY updated_at DESC, mystery_id LIMIT ?3")?;
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

    /// Complete, stable, chunked read for the formal mystery source.
    pub fn open_mysteries_complete(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<MysteryRecord>, NarrativeRepositoryError> {
        let mut result = Vec::new();
        let mut offset = 0_i64;
        loop {
            let mut statement = connection.prepare(
                "SELECT mystery_id, mystery_type, status, core_fact_id FROM mystery_states WHERE project_id=?1 AND status IN ('OPEN','PROGRESSING') ORDER BY updated_at DESC, mystery_id LIMIT ?2 OFFSET ?3",
            )?;
            let page = statement
                .query_map(
                    libsql_rusqlite::params![project_id, CONTINUITY_SOURCE_CHUNK_SIZE, offset],
                    |row| {
                        Ok(MysteryRecord {
                            mystery_id: row.get(0)?,
                            mystery_type: row.get(1)?,
                            status: row.get(2)?,
                            core_fact_id: row.get(3)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let page_len = page.len() as i64;
            result.extend(page);
            if page_len < CONTINUITY_SOURCE_CHUNK_SIZE {
                break;
            }
            offset += CONTINUITY_SOURCE_CHUNK_SIZE;
        }
        Ok(result)
    }

    pub fn has_more_mysteries(
        connection: &Connection,
        project_id: &str,
        references: &[&str],
        limit: i64,
    ) -> Result<bool, NarrativeRepositoryError> {
        let refs = Self::ids_json(references)?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM mystery_states WHERE project_id=?1 AND status IN ('OPEN','PROGRESSING') AND (mystery_id IN (SELECT value FROM json_each(?2)) OR core_fact_id IN (SELECT value FROM json_each(?2))) LIMIT 1 OFFSET ?3)", libsql_rusqlite::params![project_id, refs, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
    }

    pub fn task_states_by_ids(
        connection: &Connection,
        project_id: &str,
        task_ids: &[&str],
        limit: i64,
    ) -> Result<Vec<TaskStateRecord>, NarrativeRepositoryError> {
        let task_ids = Self::ids_json(task_ids)?;
        let mut statement = connection.prepare(TASK_STATES_BY_IDS_SQL)?;
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

    /// Reads the complete current Task Reality for a caller-provided set of
    /// task IDs without widening the shared bounded reader.  Each chunk stays
    /// within the repository's existing 100-row contract, and the final
    /// result follows the caller's stable, de-duplicated ID order.
    pub fn task_states_by_ids_complete(
        connection: &Connection,
        project_id: &str,
        task_ids: &[&str],
    ) -> Result<Vec<TaskStateRecord>, NarrativeRepositoryError> {
        let mut seen = BTreeSet::new();
        let unique_task_ids = task_ids
            .iter()
            .copied()
            .filter(|task_id| seen.insert(*task_id))
            .collect::<Vec<_>>();
        if unique_task_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut states = Vec::new();
        for chunk in unique_task_ids.chunks(TASK_STATE_CHUNK_SIZE) {
            states.extend(Self::task_states_by_ids(
                connection,
                project_id,
                chunk,
                chunk.len() as i64,
            )?);
        }

        let states_by_id = states
            .into_iter()
            .map(|state| (state.state_id.clone(), state))
            .collect::<BTreeMap<_, _>>();
        Ok(unique_task_ids
            .into_iter()
            .filter_map(|task_id| states_by_id.get(task_id).cloned())
            .collect())
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

    /// Complete read of the same current evidence scope consumed by the B06
    /// Continuity producer. History remains bounded by the formal source
    /// predicate; only the page limit is traversed completely.
    pub fn recent_evidence_complete(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<EvidenceRecord>, NarrativeRepositoryError> {
        let mut result = Vec::new();
        let mut offset = 0_i64;
        loop {
            let mut statement = connection.prepare(
                "SELECT evidence_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end FROM canon_evidence WHERE project_id=?1 ORDER BY created_at DESC, evidence_id LIMIT ?2 OFFSET ?3",
            )?;
            let page = statement
                .query_map(
                    libsql_rusqlite::params![project_id, CONTINUITY_SOURCE_CHUNK_SIZE, offset],
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
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let page_len = page.len() as i64;
            result.extend(page);
            if page_len < CONTINUITY_SOURCE_CHUNK_SIZE {
                break;
            }
            offset += CONTINUITY_SOURCE_CHUNK_SIZE;
        }
        Ok(result)
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
        let mut statement = connection.prepare("SELECT evidence_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end FROM canon_evidence WHERE project_id=?1 AND ((final_chapter_id IN (SELECT value FROM json_each(?2)) OR source_artifact_id IN (SELECT value FROM json_each(?3))) OR (?2='[]' AND ?3='[]')) ORDER BY created_at DESC, evidence_id LIMIT ?4")?;
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
        let canonical_scope = if artifact_type == "VolumeIntent" || artifact_type == "VolumePlan" {
            Self::volume_scope_key(scope_key)?
        } else if artifact_type == "ChapterPlan" {
            Self::chapter_scope_key(scope_key)?
        } else {
            scope_key.to_string()
        };
        connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 AND h.scope_key IN (?3, ?4) ORDER BY CASE WHEN h.scope_key=?3 THEN 0 ELSE 1 END LIMIT 1", libsql_rusqlite::params![project_id, artifact_type, canonical_scope, scope_key], |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) }).optional().map_err(Into::into)
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
        let canonical_scope = Self::volume_scope_key(volume_id)?;
        connection.query_row("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumePlan' AND scope_key IN (?2, ?3) ORDER BY CASE WHEN scope_key=?2 THEN 0 ELSE 1 END LIMIT 1", libsql_rusqlite::params![project_id, canonical_scope, volume_id], |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? })).optional().map_err(Into::into)
    }

    pub fn canonical_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
        scope_key: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        let canonical_scope = if artifact_type == "VolumeIntent" || artifact_type == "VolumePlan" {
            Self::volume_scope_key(scope_key)?
        } else if artifact_type == "ChapterPlan" {
            Self::chapter_scope_key(scope_key)?
        } else {
            scope_key.to_string()
        };
        connection.query_row(
            "SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 AND scope_key IN (?3, ?4) ORDER BY CASE WHEN scope_key=?3 THEN 0 ELSE 1 END LIMIT 1",
            libsql_rusqlite::params![project_id, artifact_type, canonical_scope, scope_key],
            |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
        ).optional().map_err(Into::into)
    }

    pub fn chapter_volume_id(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT volume_id FROM chapters WHERE project_id=?1 AND chapter_id=?2",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn volume_exists(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM volumes WHERE project_id=?1 AND volume_id=?2)",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn volume_number(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<i64>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT volume_no FROM volumes WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn volume_target_chapters(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<i64>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT MAX(chapter_end - chapter_start + 1, 1) FROM volumes WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn chapter_id_for_number(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        chapter_no: i64,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND volume_id=?2 AND chapter_no=?3",
                libsql_rusqlite::params![project_id, volume_id, chapter_no],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn chapter_number(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<i64>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_id=?2",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn next_artifact_version(
        connection: &Connection,
        project_id: &str,
        artifact_id: &str,
    ) -> Result<i64, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT COALESCE(MAX(version),0)+1 FROM artifacts WHERE project_id=?1 AND artifact_id=?2",
                libsql_rusqlite::params![project_id, artifact_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn completed_planning_model_run_exists(
        connection: &Connection,
        project_id: &str,
        model_run_id: &str,
        context_manifest_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM model_runs WHERE project_id=?1 AND model_run_id=?2 AND capability='PLAN_STRATEGY' AND status='COMPLETED' AND context_manifest_ref=?3)",
                libsql_rusqlite::params![project_id, model_run_id, context_manifest_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn planning_model_run_binds_plan(
        connection: &Connection,
        project_id: &str,
        model_run_id: &str,
        context_manifest_id: &str,
        plan: &Value,
    ) -> Result<bool, NarrativeRepositoryError> {
        let parsed_json: Option<String> = connection
            .query_row(
                "SELECT p.parsed_json FROM model_parse_results p JOIN model_runs r ON r.model_run_id=p.model_run_id AND r.project_id=p.project_id WHERE p.project_id=?1 AND p.model_run_id=?2 AND p.status='OK' AND r.capability='PLAN_STRATEGY' AND r.status='COMPLETED' AND r.context_manifest_ref=?3",
                libsql_rusqlite::params![project_id, model_run_id, context_manifest_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(parsed_json) = parsed_json else {
            return Ok(false);
        };
        let parsed: Value = serde_json::from_str(&parsed_json)?;
        let plan_digest = crate::hash::sha256_canonical(plan).map_err(|error| {
            NarrativeRepositoryError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error,
            )))
        })?;
        let output_digest = parsed
            .get("outputDigest")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                parsed
                    .get("plan")
                    .and_then(|value| crate::hash::sha256_canonical(value).ok())
            });
        Ok(output_digest.as_deref() == Some(plan_digest.as_str()))
    }

    pub fn exactly_one_primary_planning_call(
        connection: &Connection,
        project_id: &str,
        context_manifest_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='PLAN_STRATEGY' AND context_manifest_ref=?2",
            libsql_rusqlite::params![project_id, context_manifest_id],
            |row| row.get(0),
        )?;
        Ok(count == 1)
    }

    pub fn approved_book_plan_exists(
        connection: &Connection,
        project_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan' AND json_extract(p.payload_json, '$.planningMaturity')='APPROVED')",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn opening_canon_reference_exists(
        connection: &Connection,
        project_id: &str,
        reference: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_facts WHERE project_id=?1 AND fact_id=?2) OR EXISTS(SELECT 1 FROM canon_relations WHERE project_id=?1 AND relation_id=?2) OR EXISTS(SELECT 1 FROM canon_events WHERE project_id=?1 AND event_id=?2)",
                libsql_rusqlite::params![project_id, reference],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn signing_evidence_reference_exists(
        connection: &Connection,
        project_id: &str,
        reference: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND artifact_id=?2) OR EXISTS(SELECT 1 FROM canon_evidence WHERE project_id=?1 AND evidence_id=?2)",
                libsql_rusqlite::params![project_id, reference],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn signing_receipt_passes(
        connection: &Connection,
        project_id: &str,
        receipt_id: &str,
        opening_canon_refs: &[String],
    ) -> Result<bool, NarrativeRepositoryError> {
        let signing_status: Option<String> = connection
            .query_row(
                "SELECT signing_status FROM project_runtime_state WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        if signing_status.as_deref() != Some("PASS") {
            return Ok(false);
        }
        let payload: Option<String> = connection
            .query_row(
                "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.artifact_type='SimulatedSigningReceipt' AND a.artifact_status='COMMITTED'",
                libsql_rusqlite::params![project_id, receipt_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(false);
        };
        let payload: Value = serde_json::from_str(&payload)?;
        let stored_hash: Option<String> = connection
            .query_row(
                "SELECT a.payload_hash FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version AND p.payload_hash=a.payload_hash WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.artifact_type='SimulatedSigningReceipt' AND a.artifact_status='COMMITTED'",
                libsql_rusqlite::params![project_id, receipt_id],
                |row| row.get(0),
            )
            .optional()?;
        let payload_hash = crate::hash::sha256_canonical(&payload).map_err(|error| {
            NarrativeRepositoryError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error,
            )))
        })?;
        if stored_hash.as_deref() != Some(payload_hash.as_str()) {
            return Ok(false);
        }
        let verdict_pass = payload.get("status").and_then(Value::as_str) == Some("PASS")
            || payload.get("verdict").and_then(Value::as_str) == Some("PASS");
        let opening_chapter_refs = payload
            .get("openingChapterRefs")
            .and_then(Value::as_array)
            .map(|refs| refs.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut opening_numbers = opening_chapter_refs
            .iter()
            .filter_map(|chapter_id| {
                connection
                    .query_row(
                        "SELECT c.chapter_no FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' WHERE c.project_id=?1 AND c.chapter_id=?2 AND c.status='COMMITTED' AND c.final_artifact_id IS NOT NULL AND c.final_version IS NOT NULL",
                        libsql_rusqlite::params![project_id, chapter_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .ok()
                    .flatten()
            })
            .collect::<Vec<_>>();
        opening_numbers.sort_unstable();
        let opening_chapters = opening_chapter_refs.len() == 3
            && opening_numbers == [1, 2, 3]
            && opening_chapter_refs.iter().all(|chapter_id| {
                connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND chapter_id=?2)",
                        libsql_rusqlite::params![project_id, chapter_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(false)
            });
        let opening_chapter_json = serde_json::to_string(&opening_chapter_refs)?;
        let receipt_canon_refs = payload
            .get("openingCanonRefs")
            .and_then(Value::as_array)
            .map(|refs| {
                let mut receipt_refs = refs.iter().filter_map(Value::as_str).collect::<Vec<_>>();
                receipt_refs.sort_unstable();
                let mut requested_refs = opening_canon_refs
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                requested_refs.sort_unstable();
                receipt_refs == requested_refs
                    && opening_canon_refs.iter().all(|reference| {
                        connection
                            .query_row(
                                "SELECT EXISTS(SELECT 1 FROM canon_evidence e WHERE e.project_id=?1 AND (e.evidence_id=?2 OR e.fact_id=?2 OR e.relation_id=?2 OR e.event_id=?2) AND e.final_chapter_id IN (SELECT value FROM json_each(?3)))",
                                libsql_rusqlite::params![project_id, reference, opening_chapter_json],
                                |row| row.get(0),
                            )
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(false);
        Ok(verdict_pass && opening_chapters && receipt_canon_refs)
    }

    pub fn latest_artifact_payload(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 ORDER BY h.scope_key LIMIT 1", libsql_rusqlite::params![project_id, artifact_type], |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) }).optional().map_err(Into::into)
    }

    pub fn artifact_payload_by_ref(
        connection: &Connection,
        project_id: &str,
        reference: &ArtifactRef,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        connection
            .query_row(
                ARTIFACT_PAYLOAD_BY_REF_SQL,
                libsql_rusqlite::params![
                    project_id,
                    reference.artifact_id,
                    reference.version,
                    reference.payload_hash
                ],
                |row| {
                    let raw: String = row.get(0)?;
                    Ok(serde_json::from_str(&raw).unwrap_or(Value::Null))
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn canonical_project_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 ORDER BY scope_key LIMIT 1",
                libsql_rusqlite::params![project_id, artifact_type],
                |row| {
                    Ok(ArtifactRef {
                        artifact_id: row.get(0)?,
                        version: row.get(1)?,
                        payload_hash: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn canon_fact_exists(
        connection: &Connection,
        project_id: &str,
        fact_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_facts WHERE project_id=?1 AND fact_id=?2)",
                libsql_rusqlite::params![project_id, fact_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn b07_chapter_plan_payload(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        let scope = Self::chapter_scope_key(chapter_id)?;
        connection.query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2 AND json_extract(a.provenance_json,'$.createdBy')='B07::PlanningOwner'",
            libsql_rusqlite::params![project_id, scope],
            |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) },
        ).optional().map_err(Into::into)
    }

    pub fn obligation_authority(
        connection: &Connection,
        project_id: &str,
        obligation_id: &str,
    ) -> Result<Option<ObligationAuthorityRecord>, NarrativeRepositoryError> {
        connection.query_row(
            "SELECT obligation_type,scope,authority_level,description,must_resolve_before_chapter,prerequisites_json,decision_ref FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
            libsql_rusqlite::params![project_id, obligation_id],
            |row| {
                let prerequisites: String = row.get(5)?;
                Ok(ObligationAuthorityRecord {
                    obligation_type: row.get(0)?, scope: row.get(1)?, authority_level: row.get(2)?,
                    description: row.get(3)?, must_resolve_before_chapter: row.get(4)?,
                    prerequisites: serde_json::from_str(&prerequisites).unwrap_or_else(|_| Value::Array(vec![])),
                    decision_ref: row.get(6)?,
                })
            },
        ).optional().map_err(Into::into)
    }

    pub fn foreshadow_authority(
        connection: &Connection,
        project_id: &str,
        foreshadow_id: &str,
    ) -> Result<Option<ForeshadowAuthorityRecord>, NarrativeRepositoryError> {
        connection.query_row(
            "SELECT intent,status,plant_chapter_id,recover_window_end_chapter,secret_payload_ref FROM foreshadows WHERE project_id=?1 AND foreshadow_id=?2",
            libsql_rusqlite::params![project_id, foreshadow_id],
            |row| Ok(ForeshadowAuthorityRecord { intent: row.get(0)?, status: row.get(1)?, plant_chapter_id: row.get(2)?, recover_window_end_chapter: row.get(3)?, secret_payload_ref: row.get(4)? }),
        ).optional().map_err(Into::into)
    }
}
