use crate::generated::types::ArtifactRef;
use crate::narrative::{build_continuity_threads, ContinuityThreadView, ResumeContext};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    #[error("narrative invariant: {0}")]
    Invariant(String),
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

#[derive(Debug, Clone)]
pub struct SemanticCanonFactGroup {
    pub subject_id: String,
    pub predicate: String,
    pub facts: Vec<SemanticCanonFactRecord>,
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
    pub description: String,
    pub fulfillment_object: Option<String>,
    pub source_event_id: Option<String>,
    pub resume_condition: Value,
    pub participant_entities: Vec<String>,
    pub evidence_refs: Vec<crate::generated::types::EvidenceRef>,
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
    pub resume_condition: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResponsibilityAdjustmentRecord {
    pub obligation_id: String,
    pub scope: String,
    pub obligation_type: String,
    pub authority_level: String,
    pub lifecycle: String,
    pub description: String,
    pub must_resolve_before_chapter: Option<String>,
    pub prerequisites: Value,
    pub decision_ref: Option<String>,
    pub resume_condition: Value,
    pub fulfillment_object: Option<String>,
    pub source_artifact_id: String,
    pub source_version: i64,
    pub evidence_refs: Vec<crate::generated::types::EvidenceRef>,
}

/// The authoritative timing fields used by Planning Pressure. The plan is a
/// consumer of this projection, never a second source for thread age or
/// deadlines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityPressureRecord {
    pub thread_id: String,
    pub lifecycle: String,
    pub authority_level: String,
    pub must_resolve_before_chapter: Option<i64>,
    pub last_touched_chapter_no: Option<i64>,
    pub resume_condition: Value,
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
pub struct RecentCommittedChapter {
    pub chapter_id: String,
    pub volume_id: String,
    pub chapter_no: i64,
    pub title: String,
    pub word_count: i64,
    pub final_version: Option<i64>,
    pub updated_at: String,
}

/// B02's bounded opening snapshot.  It is a read projection assembled from
/// committed chapter/artifact/evidence rows; it is never persisted as a second
/// authority.  The signing owner uses this exact snapshot for both the verdict
/// and the evidence carried by the SimulatedSigningReceipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpeningEvaluationChapter {
    pub chapter_id: String,
    pub chapter_no: i64,
    pub final_ref: Option<ArtifactRef>,
    pub evidence: Vec<EvidenceRecord>,
    pub canon_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpeningEvaluationSnapshot {
    pub chapters: Vec<OpeningEvaluationChapter>,
    pub opening_canon_refs: Vec<String>,
    pub signing_evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpeningActualFact {
    pub fact_id: String,
    pub subject_id: String,
    pub subject_name: Option<String>,
    pub predicate: String,
    pub object_json: Value,
    pub source_chapter_id: Option<String>,
    pub source_artifact_id: Option<String>,
    pub source_version: Option<i64>,
    pub evidence_range: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpeningActualRelation {
    pub relation_id: String,
    pub subject_id: String,
    pub subject_name: Option<String>,
    pub predicate: String,
    pub object_id: String,
    pub object_name: Option<String>,
    pub source_chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpeningActualEvent {
    pub event_id: String,
    pub chapter_id: String,
    pub event_type: String,
    pub subject_id: Option<String>,
    pub object_json: Value,
    pub story_time: Option<String>,
    pub payload_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpeningActualState {
    pub chapters: Vec<OpeningEvaluationChapter>,
    pub facts: Vec<OpeningActualFact>,
    pub relations: Vec<OpeningActualRelation>,
    pub knowledge: Vec<KnowledgeRecord>,
    pub events: Vec<OpeningActualEvent>,
    pub evidence: Vec<EvidenceRecord>,
    pub semantic_digest: String,
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
    pub fn chapter_title(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT title FROM chapters WHERE project_id=?1 AND chapter_id=?2 AND status='COMMITTED'",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Reads bounded bodies from canonical FinalChapter heads for the
    /// deterministic signing originality check. A truncated result cannot be
    /// treated as proof of originality.
    pub fn canonical_final_bodies(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<(String, String)>, bool), NarrativeRepositoryError> {
        let bound = limit.clamp(1, 10_000);
        let mut statement = connection.prepare(
            "SELECT h.artifact_id,p.payload_json FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version AND a.payload_hash=h.payload_hash AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version AND p.payload_hash=a.payload_hash WHERE h.project_id=?1 AND h.artifact_type='FinalChapter' ORDER BY h.artifact_id LIMIT ?2",
        )?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| {
            let artifact_id: String = row.get(0)?;
            let payload: String = row.get(1)?;
            Ok((artifact_id, payload))
        })?;
        let mut bodies = Vec::new();
        for row in rows {
            let (artifact_id, raw_payload) = row?;
            let payload: Value = serde_json::from_str(&raw_payload)?;
            if let Some(body) = payload.get("body").and_then(Value::as_str) {
                bodies.push((artifact_id, body.to_owned()));
            }
        }
        let truncated = bodies.len() > bound as usize;
        bodies.truncate(bound as usize);
        Ok((bodies, truncated))
    }

    pub fn canonical_final_head_refs(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<ArtifactRef>, bool), NarrativeRepositoryError> {
        let bound = limit.clamp(1, 10_000);
        let mut statement = connection.prepare(
            "SELECT h.artifact_id,h.version,h.payload_hash FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version AND a.payload_hash=h.payload_hash AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' WHERE h.project_id=?1 AND h.artifact_type='FinalChapter' ORDER BY h.artifact_id LIMIT ?2",
        )?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| {
            Ok(ArtifactRef {
                artifact_id: row.get(0)?,
                version: row.get(1)?,
                payload_hash: row.get(2)?,
            })
        })?;
        let mut refs = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = refs.len() > bound as usize;
        refs.truncate(bound as usize);
        Ok((refs, truncated))
    }

    pub fn committed_final_chapter_head(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT h.artifact_id,h.version,h.payload_hash FROM chapters c JOIN canonical_heads h ON h.project_id=c.project_id AND h.artifact_type='FinalChapter' AND h.scope_level='CHAPTER' AND json_extract(h.scope_key,'$.chapterId')=c.chapter_id JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version AND a.payload_hash=h.payload_hash AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED' WHERE c.project_id=?1 AND c.chapter_id=?2 AND c.status='COMMITTED' AND c.final_artifact_id=h.artifact_id AND c.final_version=h.version",
                libsql_rusqlite::params![project_id, chapter_id],
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

    pub fn receipt_bound_opening_finals_are_current(
        connection: &Connection,
        project_id: &str,
        payload: &Value,
    ) -> Result<bool, NarrativeRepositoryError> {
        let opening = crate::repositories::project_entry::opening_chapters(connection, project_id)
            .map_err(|error| NarrativeRepositoryError::Invariant(error.to_string()))?;
        let opening_count = crate::repositories::project_entry::OPENING_CHAPTER_COUNT as usize;
        if opening.len() != opening_count
            || opening.iter().enumerate().any(|(index, chapter)| {
                chapter.status != "COMMITTED" || chapter.chapter_no != index as i64 + 1
            })
        {
            return Ok(false);
        }
        let Some(bound_chapter_ids) = payload
            .get("openingChapterRefs")
            .and_then(Value::as_array)
            .and_then(|refs| refs.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
        else {
            return Ok(false);
        };
        if bound_chapter_ids.len() != opening.len()
            || bound_chapter_ids
                .iter()
                .zip(&opening)
                .any(|(bound_id, chapter)| *bound_id != chapter.chapter_id)
        {
            return Ok(false);
        }
        let Some(chapter_evidence) = payload.get("chapterEvidence").and_then(Value::as_array)
        else {
            return Ok(false);
        };
        if chapter_evidence.len() != opening.len() {
            return Ok(false);
        }
        for chapter in &opening {
            let Some(bound) = chapter_evidence.iter().find(|item| {
                item.get("chapterId").and_then(Value::as_str) == Some(chapter.chapter_id.as_str())
            }) else {
                return Ok(false);
            };
            if bound.get("chapterNo").and_then(Value::as_i64) != Some(chapter.chapter_no) {
                return Ok(false);
            }
            let Some(bound_final_ref) = bound
                .get("finalChapterRef")
                .cloned()
                .and_then(|value| serde_json::from_value::<ArtifactRef>(value).ok())
            else {
                return Ok(false);
            };
            let Some(current_final_ref) =
                Self::committed_final_chapter_head(connection, project_id, &chapter.chapter_id)?
            else {
                return Ok(false);
            };
            if bound_final_ref.artifact_id != current_final_ref.artifact_id
                || bound_final_ref.version != current_final_ref.version
                || bound_final_ref.payload_hash != current_final_ref.payload_hash
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn recent_committed_chapters(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<RecentCommittedChapter>, NarrativeRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT chapter_id,volume_id,chapter_no,title,word_count,final_version,updated_at FROM chapters WHERE project_id=?1 AND status='COMMITTED' ORDER BY chapter_no DESC,chapter_id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, Self::limit(limit)],
            |row| {
                Ok(RecentCommittedChapter {
                    chapter_id: row.get(0)?,
                    volume_id: row.get(1)?,
                    chapter_no: row.get(2)?,
                    title: row.get(3)?,
                    word_count: row.get(4)?,
                    final_version: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

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
            "SELECT o.obligation_id,o.obligation_type,o.authority_level,o.lifecycle,o.source_event_id,o.resume_condition_json,(SELECT COALESCE(json_group_array(d.entity_id), '[]') FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IS NOT NULL),o.description,(SELECT d.payload_json FROM decisions d WHERE d.project_id=o.project_id AND d.decision_id=o.decision_ref LIMIT 1) FROM narrative_obligations o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING','RESOLVED','ABANDONED') ORDER BY o.obligation_id",
        )?;
        let mut obligations = Vec::new();
        for row in statement.query_map([project_id], |row| {
            let resume_condition: String = row.get(5)?;
            let participants: String = row.get(6)?;
            let promotion_payload: Option<String> = row.get(8)?;
            let fulfillment_object = promotion_payload
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .and_then(|payload| {
                    payload
                        .get("fulfillmentObject")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                });
            Ok(ObligationRecord {
                obligation_id: row.get(0)?,
                obligation_type: row.get(1)?,
                authority_level: row.get(2)?,
                lifecycle: row.get(3)?,
                description: row.get(7)?,
                fulfillment_object,
                source_event_id: row.get(4)?,
                resume_condition: serde_json::from_str(&resume_condition)
                    .unwrap_or_else(|_| Value::Object(Default::default())),
                participant_entities: serde_json::from_str(&participants).unwrap_or_default(),
                evidence_refs: Vec::new(),
            })
        })? {
            obligations.push(row?);
        }
        let obligation_ids = obligations
            .iter()
            .map(|obligation| obligation.obligation_id.clone())
            .collect::<Vec<_>>();
        let mut evidence_refs =
            Self::obligation_evidence_refs_by_ids(connection, project_id, &obligation_ids)?;
        for obligation in &mut obligations {
            obligation.evidence_refs = evidence_refs
                .remove(&obligation.obligation_id)
                .unwrap_or_default();
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
        Self::current_canon_facts_for_semantic_key_bounded(
            connection, project_id, subject_id, predicate, 100,
        )
        .map(|(facts, _)| facts)
    }

    pub fn current_canon_facts_for_semantic_key_bounded(
        connection: &Connection,
        project_id: &str,
        subject_id: &str,
        predicate: &str,
        limit: i64,
    ) -> Result<(Vec<SemanticCanonFactRecord>, bool), NarrativeRepositoryError> {
        let query_limit = Self::limit(limit).saturating_add(1);
        let mut statement = connection.prepare(
            "SELECT f.fact_id, f.object_json, f.valid_from_chapter_id, f.valid_to_chapter_id, f.story_time_from, f.story_time_to, from_chapter.chapter_no, to_chapter.chapter_no FROM canon_facts f LEFT JOIN chapters from_chapter ON from_chapter.project_id=f.project_id AND from_chapter.chapter_id=f.valid_from_chapter_id LEFT JOIN chapters to_chapter ON to_chapter.project_id=f.project_id AND to_chapter.chapter_id=f.valid_to_chapter_id WHERE f.project_id=?1 AND f.subject_id=?2 AND f.predicate=?3 AND f.superseded_by IS NULL ORDER BY f.fact_id LIMIT ?4",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, subject_id, predicate, query_limit],
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
        let mut facts = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(NarrativeRepositoryError::from)?;
        let truncated = facts.len() > limit as usize;
        facts.truncate(limit as usize);
        Ok((facts, truncated))
    }

    pub fn current_canon_semantic_groups_bounded(
        connection: &Connection,
        project_id: &str,
        predicates: &[String],
        limit: usize,
    ) -> Result<(Vec<SemanticCanonFactGroup>, bool), NarrativeRepositoryError> {
        let predicates_json = serde_json::to_string(predicates)?;
        let query_limit = (limit as i64).saturating_add(1);
        let mut statement = connection.prepare(
            "SELECT f.subject_id,f.predicate,f.fact_id,f.object_json,f.valid_from_chapter_id,f.valid_to_chapter_id,f.story_time_from,f.story_time_to,from_chapter.chapter_no,to_chapter.chapter_no FROM canon_facts f LEFT JOIN chapters from_chapter ON from_chapter.project_id=f.project_id AND from_chapter.chapter_id=f.valid_from_chapter_id LEFT JOIN chapters to_chapter ON to_chapter.project_id=f.project_id AND to_chapter.chapter_id=f.valid_to_chapter_id WHERE f.project_id=?1 AND f.superseded_by IS NULL AND f.predicate IN (SELECT value FROM json_each(?2)) ORDER BY f.subject_id,f.predicate,f.fact_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, predicates_json, query_limit],
            |row| {
                let object_json: String = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    SemanticCanonFactRecord {
                        fact_id: row.get(2)?,
                        object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                        valid_from_chapter_id: row.get(4)?,
                        valid_to_chapter_id: row.get(5)?,
                        story_time_from: row.get(6)?,
                        story_time_to: row.get(7)?,
                        valid_from_chapter_no: row.get(8)?,
                        valid_to_chapter_no: row.get(9)?,
                    },
                ))
            },
        )?;
        let mut facts = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(NarrativeRepositoryError::from)?;
        let truncated = facts.len() > limit;
        facts.truncate(limit);
        let mut groups = BTreeMap::<(String, String), Vec<SemanticCanonFactRecord>>::new();
        for (subject_id, predicate, fact) in facts {
            groups
                .entry((subject_id, predicate))
                .or_default()
                .push(fact);
        }
        Ok((
            groups
                .into_iter()
                .map(|((subject_id, predicate), facts)| SemanticCanonFactGroup {
                    subject_id,
                    predicate,
                    facts,
                })
                .collect(),
            truncated,
        ))
    }

    pub fn canon_fact_timing(
        connection: &Connection,
        project_id: &str,
        fact_id: &str,
    ) -> Result<Option<CanonFactTimingRecord>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT COALESCE(f.story_time_from,(SELECT e.story_time FROM canon_evidence ce JOIN canon_events e ON e.project_id=ce.project_id AND e.event_id=ce.event_id WHERE ce.project_id=f.project_id AND ce.fact_id=f.fact_id ORDER BY e.event_ordinal,e.event_id,ce.evidence_id LIMIT 1)),c.chapter_no FROM canon_facts f LEFT JOIN chapters c ON c.project_id=f.project_id AND c.chapter_id=f.source_chapter_id WHERE f.project_id=?1 AND f.fact_id=?2",
                libsql_rusqlite::params![project_id, fact_id],
                |row| {
                    Ok(CanonFactTimingRecord {
                        story_time: row.get(0)?,
                        source_chapter_no: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn goal_contract_refs(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<ArtifactRef>, NarrativeRepositoryError> {
        Ok(Self::goal_contract_refs_bounded(connection, project_id, limit)?.0)
    }

    pub fn goal_contract_refs_bounded(
        connection: &Connection,
        project_id: &str,
        limit: i64,
    ) -> Result<(Vec<ArtifactRef>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type IN ('StoryConstitution','BookPlan','MainContract','VolumeIntent') ORDER BY artifact_type, scope_key LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id, bound + 1], |row| {
            Ok(ArtifactRef {
                artifact_id: row.get(0)?,
                version: row.get(1)?,
                payload_hash: row.get(2)?,
            })
        })?;
        let mut refs = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = refs.len() as i64 > bound;
        refs.truncate(bound as usize);
        Ok((refs, truncated))
    }

    pub fn goal_contract_refs_for_volume(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        limit: i64,
    ) -> Result<Vec<ArtifactRef>, NarrativeRepositoryError> {
        Ok(
            Self::goal_contract_refs_for_volume_bounded(connection, project_id, volume_id, limit)?
                .0,
        )
    }

    pub fn goal_contract_refs_for_volume_bounded(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        limit: i64,
    ) -> Result<(Vec<ArtifactRef>, bool), NarrativeRepositoryError> {
        let canonical_scope = Self::volume_scope_key(volume_id)?;
        let bound = Self::limit(limit);
        let mut statement = connection.prepare("SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id=?1 AND (artifact_type IN ('StoryConstitution','BookPlan','MainContract') OR (artifact_type='VolumeIntent' AND scope_key IN (?2, ?3))) ORDER BY artifact_type, scope_key LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, canonical_scope, volume_id, bound + 1],
            |row| {
                Ok(ArtifactRef {
                    artifact_id: row.get(0)?,
                    version: row.get(1)?,
                    payload_hash: row.get(2)?,
                })
            },
        )?;
        let mut refs = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = refs.len() as i64 > bound;
        refs.truncate(bound as usize);
        Ok((refs, truncated))
    }

    /// Reads the bounded authority set consumed by B03's deterministic volume
    /// compilers.  This is a read projection only: all returned values come
    /// from the existing Canon/obligation tables and no planning authority is
    /// created here.
    pub fn volume_planning_sources(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        limit: i64,
    ) -> Result<Value, NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let chapter_ids = {
            let mut statement = connection.prepare(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND volume_id=?2 ORDER BY chapter_no,chapter_id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(
                    libsql_rusqlite::params![project_id, volume_id, bound],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let chapter_json = serde_json::to_string(&chapter_ids)?;
        let facts = {
            let mut statement = connection.prepare(
                "SELECT fact_id,subject_id,predicate,object_json,authority_level FROM canon_facts WHERE project_id=?1 AND superseded_by IS NULL ORDER BY fact_id LIMIT ?2",
            )?;
            let rows = statement
                .query_map(libsql_rusqlite::params![project_id, bound], |row| {
                    let object_json: String = row.get(3)?;
                    Ok(CanonFactRecord {
                        fact_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        predicate: row.get(2)?,
                        object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                        authority_level: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let relations = {
            let mut statement = connection.prepare(
                "SELECT relation_id,subject_id,predicate,object_id,authority_level FROM canon_relations WHERE project_id=?1 AND superseded_by IS NULL ORDER BY relation_id LIMIT ?2",
            )?;
            let rows = statement
                .query_map(libsql_rusqlite::params![project_id, bound], |row| {
                    Ok(RelationRecord {
                        relation_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        predicate: row.get(2)?,
                        object_id: row.get(3)?,
                        authority_level: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let knowledge = {
            let mut statement = connection.prepare(
                "SELECT knowledge_id,knower_id,fact_id,epistemic_state,visibility_scope,from_chapter_id FROM canon_knowledge WHERE project_id=?1 ORDER BY knowledge_id LIMIT ?2",
            )?;
            let rows = statement
                .query_map(libsql_rusqlite::params![project_id, bound], |row| {
                    Ok(KnowledgeRecord {
                        knowledge_id: row.get(0)?,
                        knower_id: row.get(1)?,
                        fact_id: row.get(2)?,
                        epistemic_state: row.get(3)?,
                        visibility_scope: row.get(4)?,
                        from_chapter_id: row.get(5)?,
                        from_story_time: None,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let events = {
            let mut statement = connection.prepare(
                "SELECT event_id,chapter_id,event_type,subject_id,object_json,story_time,payload_json,event_ordinal FROM canon_events WHERE project_id=?1 AND (chapter_id IN (SELECT value FROM json_each(?2)) OR ?2='[]') ORDER BY chapter_id,event_ordinal,event_id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(
                    libsql_rusqlite::params![project_id, chapter_json, bound],
                    |row| {
                        let object_json: Option<String> = row.get(4)?;
                        let payload_json: String = row.get(6)?;
                        Ok(json!({
                            "eventId": row.get::<_, String>(0)?,
                            "chapterId": row.get::<_, String>(1)?,
                            "eventType": row.get::<_, String>(2)?,
                            "subjectId": row.get::<_, Option<String>>(3)?,
                            "objectJson": object_json.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()).unwrap_or(Value::Null),
                            "storyTime": row.get::<_, Option<String>>(5)?,
                            "payloadJson": serde_json::from_str::<Value>(&payload_json).unwrap_or_else(|_| json!({})),
                            "eventOrdinal": row.get::<_, i64>(7)?,
                        }))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let evidence = {
            let mut statement = connection.prepare(
                "SELECT evidence_id,fact_id,relation_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end FROM canon_evidence WHERE project_id=?1 AND (final_chapter_id IN (SELECT value FROM json_each(?2)) OR ?2='[]') ORDER BY created_at,evidence_id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(
                    libsql_rusqlite::params![project_id, chapter_json, bound],
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
            rows
        };
        let responsibilities = {
            let volume_scope = Self::volume_scope_key(volume_id)?;
            let mut statement = connection.prepare(
                "SELECT o.obligation_id,o.obligation_type,o.scope,o.authority_level,o.lifecycle,o.description,o.must_resolve_before_chapter,o.prerequisites_json,o.source_event_id,o.resume_condition_json,o.source_artifact_id,o.source_version FROM narrative_obligations o WHERE o.project_id=?1 AND o.scope IN ('BOOK','VOLUME') AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND (o.scope='BOOK' OR EXISTS (SELECT 1 FROM artifacts a WHERE a.project_id=o.project_id AND a.artifact_id=o.source_artifact_id AND a.version=o.source_version AND a.scope_level='VOLUME' AND a.scope_key IN (?3,?4))) ORDER BY o.authority_level,o.obligation_id LIMIT ?2",
            )?;
            let rows = statement
                .query_map(
                    libsql_rusqlite::params![project_id, bound, volume_scope, volume_id],
                    |row| {
                    let prerequisites: String = row.get(7)?;
                    let resume_condition: String = row.get(9)?;
                    Ok(json!({
                        "obligationId": row.get::<_, String>(0)?,
                        "obligationType": row.get::<_, String>(1)?,
                        "scope": row.get::<_, String>(2)?,
                        "authorityLevel": row.get::<_, String>(3)?,
                        "lifecycle": row.get::<_, String>(4)?,
                        "description": row.get::<_, String>(5)?,
                        "mustResolveBeforeChapter": row.get::<_, Option<String>>(6)?,
                        "prerequisites": serde_json::from_str::<Value>(&prerequisites).unwrap_or_else(|_| json!([])),
                        "sourceEventId": row.get::<_, Option<String>>(8)?,
                        "resumeCondition": serde_json::from_str::<Value>(&resume_condition).unwrap_or_else(|_| json!({})),
                        "sourceArtifactId": row.get::<_, String>(10)?,
                        "sourceVersion": row.get::<_, i64>(11)?,
                    }))
                    }
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        Ok(json!({
            "volumeId": volume_id,
            "chapterIds": chapter_ids,
            "facts": facts,
            "relations": relations,
            "knowledge": knowledge,
            "events": events,
            "evidence": evidence,
            "responsibilities": responsibilities,
        }))
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
        let bound = Self::limit(limit);
        let entity_ids = Self::ids_json(entity_ids)?;
        let mut statement = connection.prepare("SELECT fact_id, subject_id, predicate, object_json, authority_level FROM canon_facts WHERE project_id=?1 AND subject_id IN (SELECT value FROM json_each(?2)) AND superseded_by IS NULL ORDER BY fact_id LIMIT ?3")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, entity_ids, bound + 1],
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

    /// Returns the bounded entity set needed to continue from the most recent
    /// committed chapters in a volume.  LONG_FORM callers may omit an
    /// explicit entity selection, but they must still receive real scoped
    /// authority instead of an empty context.
    pub fn recent_chapter_entity_ids(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<Vec<String>, NarrativeRepositoryError> {
        Ok(Self::recent_chapter_entity_ids_bounded(
            connection, project_id, volume_id, chapter_id, limit,
        )?
        .0)
    }

    pub fn recent_chapter_entity_ids_bounded(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        limit: i64,
    ) -> Result<(Vec<String>, bool), NarrativeRepositoryError> {
        let bound = Self::limit(limit);
        let chapter_ids = Self::ids_json(
            &Self::previous_committed_chapter_ids(connection, project_id, chapter_id, 2)?
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
        let mut statement = connection.prepare(
            "SELECT entity_id FROM (SELECT subject_id AS entity_id FROM canon_facts WHERE project_id=?1 AND (source_chapter_id IN (SELECT value FROM json_each(?2)) OR valid_from_chapter_id IN (SELECT value FROM json_each(?2))) UNION SELECT subject_id AS entity_id FROM canon_relations WHERE project_id=?1 AND valid_from_chapter_id IN (SELECT value FROM json_each(?2)) UNION SELECT object_id AS entity_id FROM canon_relations WHERE project_id=?1 AND valid_from_chapter_id IN (SELECT value FROM json_each(?2)) UNION SELECT knower_id AS entity_id FROM canon_knowledge WHERE project_id=?1 AND from_chapter_id IN (SELECT value FROM json_each(?2))) WHERE entity_id IS NOT NULL ORDER BY entity_id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, chapter_ids, bound + 1],
            |row| row.get(0),
        )?;
        let mut ids = rows.collect::<Result<Vec<_>, _>>()?;
        let truncated = ids.len() as i64 > bound;
        ids.truncate(bound as usize);
        // The volume argument is part of the authority boundary: a chapter
        // from another volume must never widen the continuation context.
        if ids.is_empty() {
            let chapter_exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND volume_id=?2 AND chapter_id=?3)",
                libsql_rusqlite::params![project_id, volume_id, chapter_id],
                |row| row.get(0),
            )?;
            if !chapter_exists {
                return Err(NarrativeRepositoryError::Invariant(
                    "CHAPTER_VOLUME_MISMATCH".into(),
                ));
            }
        }
        Ok((ids, truncated))
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
        let mut statement = connection.prepare("SELECT o.obligation_id,o.obligation_type,o.authority_level,o.lifecycle,o.source_event_id,o.resume_condition_json,(SELECT COALESCE(json_group_array(d.entity_id), '[]') FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IS NOT NULL),o.description,(SELECT d.payload_json FROM decisions d WHERE d.project_id=o.project_id AND d.decision_id=o.decision_ref LIMIT 1) FROM narrative_obligations AS o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND o.authority_level IN ('A','B') AND ((o.obligation_id IN (SELECT value FROM json_each(?2)) OR o.source_event_id IN (SELECT value FROM json_each(?2)) OR EXISTS (SELECT 1 FROM dependency_edges AS d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IN (SELECT value FROM json_each(?3)))) OR (?2='[]' AND ?3='[]')) ORDER BY o.updated_at DESC, o.obligation_id LIMIT ?4")?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, references, entity_ids, Self::limit(limit)],
            |row| {
                let resume_condition: String = row.get(5)?;
                let participants: String = row.get(6)?;
                let promotion_payload: Option<String> = row.get(8)?;
                let fulfillment_object = promotion_payload
                    .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                    .and_then(|payload| {
                        payload
                            .get("fulfillmentObject")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    });
                Ok(ObligationRecord {
                    obligation_id: row.get(0)?,
                    obligation_type: row.get(1)?,
                    authority_level: row.get(2)?,
                    lifecycle: row.get(3)?,
                    description: row.get(7)?,
                    fulfillment_object,
                    source_event_id: row.get(4)?,
                    resume_condition: serde_json::from_str(&resume_condition)
                        .unwrap_or(Value::Object(Default::default())),
                    participant_entities: serde_json::from_str(&participants).unwrap_or_default(),
                    evidence_refs: Vec::new(),
                })
            },
        )?;
        let mut obligations = rows.collect::<Result<Vec<_>, _>>()?;
        let obligation_ids = obligations
            .iter()
            .map(|obligation| obligation.obligation_id.clone())
            .collect::<Vec<_>>();
        let mut evidence_refs =
            Self::obligation_evidence_refs_by_ids(connection, project_id, &obligation_ids)?;
        for obligation in &mut obligations {
            obligation.evidence_refs = evidence_refs
                .remove(&obligation.obligation_id)
                .unwrap_or_default();
        }
        Ok(obligations)
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
        connection.query_row("SELECT EXISTS(SELECT 1 FROM narrative_obligations AS o WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') AND o.authority_level IN ('A','B') AND ((o.obligation_id IN (SELECT value FROM json_each(?2)) OR o.source_event_id IN (SELECT value FROM json_each(?2)) OR EXISTS (SELECT 1 FROM dependency_edges d WHERE d.project_id=o.project_id AND (d.source_ref=o.obligation_id OR d.target_ref=o.obligation_id) AND d.entity_id IN (SELECT value FROM json_each(?3)))) OR (?2='[]' AND ?3='[]')) LIMIT 1 OFFSET ?4)", libsql_rusqlite::params![project_id, references, entity_ids, Self::limit(limit)], |row| row.get(0)).map_err(Into::into)
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

    pub fn chapter_status(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT status FROM chapters WHERE project_id=?1 AND chapter_id=?2",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn next_unsubmitted_chapter_id(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND volume_id=?2 AND status!='COMMITTED' ORDER BY chapter_no,chapter_id LIMIT 1",
                libsql_rusqlite::params![project_id, volume_id],
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

    pub fn future_reservation_volume_numbers(
        connection: &Connection,
        project_id: &str,
        reservations: &[Value],
    ) -> Result<BTreeMap<String, i64>, NarrativeRepositoryError> {
        let mut volume_numbers = BTreeMap::new();
        for volume_id in reservations
            .iter()
            .filter_map(|item| item.get("reservedForVolumeId").and_then(Value::as_str))
            .filter(|volume_id| !volume_id.trim().is_empty())
        {
            let volume_no =
                Self::volume_number(connection, project_id, volume_id)?.ok_or_else(|| {
                    NarrativeRepositoryError::Invariant(
                        "FUTURE_RESERVATION_VOLUME_NOT_FOUND".into(),
                    )
                })?;
            volume_numbers.insert(volume_id.to_owned(), volume_no);
        }
        Ok(volume_numbers)
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

    /// Reads only committed chapters inside the project-entry opening boundary
    /// (Volume 1, chapter numbers 1..=OPENING_CHAPTER_COUNT) and their current
    /// FinalChapter heads.
    pub fn opening_evaluation_snapshot(
        connection: &Connection,
        project_id: &str,
    ) -> Result<OpeningEvaluationSnapshot, NarrativeRepositoryError> {
        let mut chapters = Vec::new();
        let mut opening_canon_refs = BTreeSet::new();
        let mut signing_evidence_refs = Vec::new();
        let opening_chapters =
            crate::repositories::project_entry::opening_chapters(connection, project_id)
                .map_err(|error| NarrativeRepositoryError::Invariant(error.to_string()))?;
        for chapter in opening_chapters
            .into_iter()
            .filter(|chapter| chapter.status == "COMMITTED")
        {
            let chapter_id = chapter.chapter_id;
            let chapter_no = chapter.chapter_no;
            let final_ref =
                Self::committed_final_chapter_head(connection, project_id, &chapter_id)?;
            let mut evidence_statement = connection.prepare(
                "SELECT evidence_id,fact_id,relation_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end FROM canon_evidence WHERE project_id=?1 AND final_chapter_id=?2 ORDER BY evidence_id LIMIT 100",
            )?;
            let evidence = evidence_statement
                .query_map(libsql_rusqlite::params![project_id, chapter_id], |row| {
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
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut canon_refs = BTreeSet::new();
            for item in &evidence {
                for reference in [&item.fact_id, &item.relation_id, &item.event_id]
                    .into_iter()
                    .flatten()
                {
                    canon_refs.insert(reference.clone());
                    opening_canon_refs.insert(reference.clone());
                }
                signing_evidence_refs.push(item.evidence_id.clone());
            }
            chapters.push(OpeningEvaluationChapter {
                chapter_id,
                chapter_no,
                final_ref,
                evidence,
                canon_refs: canon_refs.into_iter().collect(),
            });
        }
        Ok(OpeningEvaluationSnapshot {
            chapters,
            opening_canon_refs: opening_canon_refs.into_iter().collect(),
            signing_evidence_refs,
        })
    }

    /// Compiles the bounded, semantic opening reality used by B02.  IDs select
    /// rows, but the returned state always contains the row content and its
    /// provenance so planning cannot mistake an ID digest for narrative
    /// absorption.
    pub fn opening_actual_state(
        connection: &Connection,
        project_id: &str,
        snapshot: &OpeningEvaluationSnapshot,
    ) -> Result<OpeningActualState, NarrativeRepositoryError> {
        let chapter_ids = snapshot
            .chapters
            .iter()
            .map(|chapter| chapter.chapter_id.as_str())
            .collect::<Vec<_>>();
        let chapter_json = Self::ids_json(&chapter_ids)?;
        let canon_json = Self::ids_json(
            &snapshot
                .opening_canon_refs
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
        let mut facts_statement = connection.prepare(
            "SELECT f.fact_id,f.subject_id,e.canonical_name,f.predicate,f.object_json,f.source_chapter_id,f.source_artifact_id,f.source_version,f.evidence_range_json FROM canon_facts f LEFT JOIN canon_entities e ON e.project_id=f.project_id AND e.entity_id=f.subject_id WHERE f.project_id=?1 AND (f.fact_id IN (SELECT value FROM json_each(?2)) OR f.source_chapter_id IN (SELECT value FROM json_each(?3))) ORDER BY f.fact_id LIMIT 100",
        )?;
        let facts = facts_statement
            .query_map(
                libsql_rusqlite::params![project_id, canon_json, chapter_json],
                |row| {
                    let object_json: String = row.get(4)?;
                    let evidence_range: Option<String> = row.get(8)?;
                    Ok(OpeningActualFact {
                        fact_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        subject_name: row.get(2)?,
                        predicate: row.get(3)?,
                        object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                        source_chapter_id: row.get(5)?,
                        source_artifact_id: row.get(6)?,
                        source_version: row.get(7)?,
                        evidence_range: evidence_range
                            .and_then(|value| serde_json::from_str(&value).ok()),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut relations_statement = connection.prepare(
            "SELECT r.relation_id,r.subject_id,se.canonical_name,r.predicate,r.object_id,oe.canonical_name,r.valid_from_chapter_id FROM canon_relations r LEFT JOIN canon_entities se ON se.project_id=r.project_id AND se.entity_id=r.subject_id LEFT JOIN canon_entities oe ON oe.project_id=r.project_id AND oe.entity_id=r.object_id WHERE r.project_id=?1 AND (r.relation_id IN (SELECT value FROM json_each(?2)) OR r.valid_from_chapter_id IN (SELECT value FROM json_each(?3))) ORDER BY r.relation_id LIMIT 100",
        )?;
        let relations = relations_statement
            .query_map(
                libsql_rusqlite::params![project_id, canon_json, chapter_json],
                |row| {
                    Ok(OpeningActualRelation {
                        relation_id: row.get(0)?,
                        subject_id: row.get(1)?,
                        subject_name: row.get(2)?,
                        predicate: row.get(3)?,
                        object_id: row.get(4)?,
                        object_name: row.get(5)?,
                        source_chapter_id: row.get(6)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let fact_ids = Self::ids_json(
            &facts
                .iter()
                .map(|fact| fact.fact_id.as_str())
                .collect::<Vec<_>>(),
        )?;
        let mut knowledge_statement = connection.prepare(
            "SELECT knowledge_id,knower_id,fact_id,epistemic_state,visibility_scope,from_chapter_id FROM canon_knowledge WHERE project_id=?1 AND (fact_id IN (SELECT value FROM json_each(?2)) OR from_chapter_id IN (SELECT value FROM json_each(?3))) ORDER BY knowledge_id LIMIT 100",
        )?;
        let knowledge = knowledge_statement
            .query_map(
                libsql_rusqlite::params![project_id, fact_ids, chapter_json],
                |row| {
                    Ok(KnowledgeRecord {
                        knowledge_id: row.get(0)?,
                        knower_id: row.get(1)?,
                        fact_id: row.get(2)?,
                        epistemic_state: row.get(3)?,
                        visibility_scope: row.get(4)?,
                        from_chapter_id: row.get(5)?,
                        from_story_time: None,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut events_statement = connection.prepare(
            "SELECT event_id,chapter_id,event_type,subject_id,object_json,story_time,payload_json FROM canon_events WHERE project_id=?1 AND chapter_id IN (SELECT value FROM json_each(?2)) ORDER BY event_ordinal,event_id LIMIT 100",
        )?;
        let events = events_statement
            .query_map(libsql_rusqlite::params![project_id, chapter_json], |row| {
                let object_json: String = row.get(4)?;
                let payload_json: String = row.get(6)?;
                Ok(OpeningActualEvent {
                    event_id: row.get(0)?,
                    chapter_id: row.get(1)?,
                    event_type: row.get(2)?,
                    subject_id: row.get(3)?,
                    object_json: serde_json::from_str(&object_json).unwrap_or(Value::Null),
                    story_time: row.get(5)?,
                    payload_json: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let evidence = snapshot
            .chapters
            .iter()
            .flat_map(|chapter| chapter.evidence.clone())
            .collect::<Vec<_>>();
        let mut state = OpeningActualState {
            chapters: snapshot.chapters.clone(),
            facts,
            relations,
            knowledge,
            events,
            evidence,
            semantic_digest: String::new(),
        };
        let digest_value = serde_json::to_value(&state)?;
        state.semantic_digest = crate::hash::sha256_canonical(&digest_value).map_err(|error| {
            NarrativeRepositoryError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error,
            )))
        })?;
        Ok(state)
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

    pub fn next_chapter_number(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<i64>, NarrativeRepositoryError> {
        let (chapter_start, chapter_end): (i64, i64) = connection
            .query_row(
                "SELECT chapter_start,chapter_end FROM volumes WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| NarrativeRepositoryError::Invariant("VOLUME_NOT_FOUND".into()))?;
        let max_chapter: i64 = connection.query_row(
            "SELECT COALESCE(MAX(chapter_no),?3-1) FROM chapters WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id, chapter_start],
            |row| row.get(0),
        )?;
        let next = max_chapter.max(chapter_start - 1) + 1;
        Ok((next <= chapter_end).then_some(next))
    }

    pub fn has_other_executable_chapter_plan(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canonical_heads h JOIN chapters c ON c.project_id=h.project_id AND json_extract(h.scope_key,'$.chapterId')=c.chapter_id WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND c.chapter_id!=?2 AND c.status IN ('PLANNED','PRODUCING','PAUSED'))",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ensure_planned_chapter(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        chapter_no: i64,
        title: &str,
        budget_id: &str,
        now: &str,
    ) -> Result<(), NarrativeRepositoryError> {
        let valid: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM volumes WHERE project_id=?1 AND volume_id=?2 AND chapter_start<=?3 AND chapter_end>=?3)",
            libsql_rusqlite::params![project_id, volume_id, chapter_no],
            |row| row.get(0),
        )?;
        if !valid {
            return Err(NarrativeRepositoryError::Invariant(
                "CHAPTER_NUMBER_OUTSIDE_VOLUME".into(),
            ));
        }
        connection.execute(
            "INSERT OR IGNORE INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'NORMAL','PLANNED',0,?6,?6)",
            libsql_rusqlite::params![chapter_id, project_id, volume_id, chapter_no, title, now],
        )?;
        let actual: Option<(String, i64)> = connection
            .query_row(
                "SELECT chapter_id,chapter_no FROM chapters WHERE project_id=?1 AND volume_id=?2 AND chapter_no=?3",
                libsql_rusqlite::params![project_id, volume_id, chapter_no],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if actual.as_ref().map(|row| row.0.as_str()) != Some(chapter_id)
            || actual.as_ref().map(|row| row.1) != Some(chapter_no)
        {
            return Err(NarrativeRepositoryError::Invariant(
                "CHAPTER_NUMBER_ALREADY_BOUND".into(),
            ));
        }
        connection.execute(
            "INSERT OR IGNORE INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,?2,?3,'NORMAL',12,0,?4)",
            libsql_rusqlite::params![budget_id, project_id, chapter_id, now],
        )?;
        Ok(())
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

    pub fn revealed_reservation_refs_before_chapter(
        connection: &Connection,
        project_id: &str,
        current_volume_no: i64,
        current_chapter_no: i64,
    ) -> Result<BTreeSet<String>, NarrativeRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT audit_payload.payload_json
             FROM canonical_heads receipt_head
             JOIN artifact_payloads receipt_payload
               ON receipt_payload.artifact_id=receipt_head.artifact_id
              AND receipt_payload.version=receipt_head.version
              AND receipt_payload.payload_hash=receipt_head.payload_hash
             JOIN chapters prior_chapter
               ON prior_chapter.project_id=receipt_head.project_id
              AND prior_chapter.chapter_id=json_extract(receipt_head.scope_key,'$.chapterId')
             JOIN volumes prior_volume
               ON prior_volume.project_id=prior_chapter.project_id
              AND prior_volume.volume_id=prior_chapter.volume_id
             LEFT JOIN artifacts audit_artifact
               ON audit_artifact.project_id=receipt_head.project_id
              AND audit_artifact.artifact_id=json_extract(receipt_payload.payload_json,'$.auditRef.artifactId')
              AND audit_artifact.version=CAST(json_extract(receipt_payload.payload_json,'$.auditRef.version') AS INTEGER)
              AND audit_artifact.payload_hash=json_extract(receipt_payload.payload_json,'$.auditRef.payloadHash')
              AND audit_artifact.artifact_type='ReviewReport'
             LEFT JOIN artifact_payloads audit_payload
               ON audit_payload.artifact_id=audit_artifact.artifact_id
              AND audit_payload.version=audit_artifact.version
              AND audit_payload.payload_hash=audit_artifact.payload_hash
             WHERE receipt_head.project_id=?1
               AND receipt_head.artifact_type='ProductionReceipt'
               AND receipt_head.scope_level='CHAPTER'
               AND (prior_volume.volume_no<?2
                 OR (prior_volume.volume_no=?2 AND prior_chapter.chapter_no<?3))
             ORDER BY prior_volume.volume_no,prior_chapter.chapter_no",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, current_volume_no, current_chapter_no],
            |row| row.get::<_, Option<String>>(0),
        )?;
        let mut revealed = BTreeSet::new();
        for row in rows {
            let raw = row?.ok_or_else(|| {
                NarrativeRepositoryError::Invariant(
                    "PRODUCTION_RECEIPT_AUDIT_REF_UNAVAILABLE".into(),
                )
            })?;
            let report: Value = serde_json::from_str(&raw)?;
            let refs = report
                .pointer("/audit/revealedReservationRefs")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    NarrativeRepositoryError::Invariant(
                        "PRODUCTION_RECEIPT_AUDIT_RESERVATIONS_INVALID".into(),
                    )
                })?;
            for reference in refs {
                let reference = reference
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        NarrativeRepositoryError::Invariant(
                            "PRODUCTION_RECEIPT_AUDIT_RESERVATIONS_INVALID".into(),
                        )
                    })?;
                revealed.insert(reference.to_owned());
            }
        }
        Ok(revealed)
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

    pub fn committed_signing_receipt_ref(
        connection: &Connection,
        project_id: &str,
        artifact_id: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT artifact_id,version,payload_hash FROM artifacts WHERE project_id=?1 AND artifact_id=?2 AND artifact_type='SimulatedSigningReceipt' AND artifact_status='COMMITTED' ORDER BY version DESC LIMIT 1",
                libsql_rusqlite::params![project_id, artifact_id],
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
                "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.artifact_type='SimulatedSigningReceipt' AND a.artifact_status='COMMITTED' ORDER BY a.version DESC LIMIT 1",
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
                "SELECT a.payload_hash FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version AND p.payload_hash=a.payload_hash WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.artifact_type='SimulatedSigningReceipt' AND a.artifact_status='COMMITTED' ORDER BY a.version DESC LIMIT 1",
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
        let confirmed_platform = payload
            .pointer("/confirmation/platformId")
            .and_then(Value::as_str);
        let confirmation_passes = if payload.get("evaluationRuleVersion").and_then(Value::as_str)
            == Some("signing-eval.r1")
        {
            payload
                .pointer("/confirmation/confirmed")
                .and_then(Value::as_bool)
                == Some(true)
                && confirmed_platform.is_some_and(|platform_id| {
                    ["FANQIE", "QIDIAN", "QIMAO", "ZONGHENG"].contains(&platform_id)
                        && payload
                            .get("platformMatrix")
                            .and_then(Value::as_array)
                            .is_some_and(|matrix| {
                                matrix.iter().any(|entry| {
                                    entry.get("platformId").and_then(Value::as_str)
                                        == Some(platform_id)
                                        && entry.get("mainstream").and_then(Value::as_bool)
                                            == Some(true)
                                        && entry.get("status").and_then(Value::as_str)
                                            == Some("PASS")
                                })
                            })
                })
        } else {
            true
        };
        let opening_chapter_refs = payload
            .get("openingChapterRefs")
            .and_then(Value::as_array)
            .map(|refs| refs.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let opening = crate::repositories::project_entry::opening_chapters(connection, project_id)
            .map_err(|error| NarrativeRepositoryError::Invariant(error.to_string()))?;
        let committed_opening = opening
            .iter()
            .filter(|chapter| chapter.status == "COMMITTED")
            .collect::<Vec<_>>();
        let opening_chapters = committed_opening.len()
            == crate::repositories::project_entry::OPENING_CHAPTER_COUNT as usize
            && opening_chapter_refs.len() == committed_opening.len()
            && opening_chapter_refs
                .iter()
                .zip(&committed_opening)
                .all(|(chapter_ref, chapter)| chapter_ref == &chapter.chapter_id)
            && Self::receipt_bound_opening_finals_are_current(connection, project_id, &payload)?;
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
        Ok(verdict_pass && confirmation_passes && opening_chapters && receipt_canon_refs)
    }

    pub fn latest_artifact_payload(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<Value>, NarrativeRepositoryError> {
        connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type=?2 ORDER BY h.scope_key LIMIT 1", libsql_rusqlite::params![project_id, artifact_type], |row| { let raw: String = row.get(0)?; Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)) }).optional().map_err(Into::into)
    }

    pub fn latest_book_plan_seed_ref(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<ArtifactRef>, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT a.artifact_id,a.version,a.payload_hash FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='BookPlan' AND a.artifact_status='COMMITTED' AND json_extract(p.payload_json,'$.planningMaturity')='SEED' ORDER BY a.created_at DESC,a.version DESC,a.artifact_id DESC LIMIT 1",
                [project_id],
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

    pub fn canon_fact_object_matches(
        connection: &Connection,
        project_id: &str,
        fact_id: &str,
        expected: &Value,
    ) -> Result<bool, NarrativeRepositoryError> {
        let object: Option<String> = connection
            .query_row(
                "SELECT object_json FROM canon_facts WHERE project_id=?1 AND fact_id=?2",
                libsql_rusqlite::params![project_id, fact_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(object
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .is_some_and(|value| &value == expected))
    }

    pub fn canon_event_exists(
        connection: &Connection,
        project_id: &str,
        event_id: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_events WHERE project_id=?1 AND ((?2 IS NOT NULL AND event_id=?2) OR (?3 IS NOT NULL AND event_type=?3)))",
                libsql_rusqlite::params![project_id, event_id, event_type],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn canon_relation_exists(
        connection: &Connection,
        project_id: &str,
        relation_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_relations WHERE project_id=?1 AND relation_id=?2)",
                libsql_rusqlite::params![project_id, relation_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn canon_knowledge_state_matches(
        connection: &Connection,
        project_id: &str,
        knowledge_id: &str,
        epistemic_state: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_knowledge WHERE project_id=?1 AND knowledge_id=?2 AND epistemic_state=?3)",
                libsql_rusqlite::params![project_id, knowledge_id, epistemic_state],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn obligation_lifecycle_matches(
        connection: &Connection,
        project_id: &str,
        obligation_id: &str,
        lifecycle: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2 AND lifecycle=?3)",
                libsql_rusqlite::params![project_id, obligation_id, lifecycle],
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
            "SELECT obligation_type,scope,authority_level,description,must_resolve_before_chapter,prerequisites_json,decision_ref,resume_condition_json FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
            libsql_rusqlite::params![project_id, obligation_id],
            |row| {
                let prerequisites: String = row.get(5)?;
                let resume_condition: String = row.get(7)?;
                Ok(ObligationAuthorityRecord {
                    obligation_type: row.get(0)?, scope: row.get(1)?, authority_level: row.get(2)?,
                    description: row.get(3)?, must_resolve_before_chapter: row.get(4)?,
                    prerequisites: serde_json::from_str(&prerequisites).unwrap_or_else(|_| Value::Array(vec![])),
                    decision_ref: row.get(6)?,
                    resume_condition: serde_json::from_str(&resume_condition)
                        .unwrap_or_else(|_| Value::Object(Default::default())),
                })
            },
        ).optional().map_err(Into::into)
    }

    pub fn responsibility_adjustment_record(
        connection: &Connection,
        project_id: &str,
        obligation_id: &str,
    ) -> Result<Option<ResponsibilityAdjustmentRecord>, NarrativeRepositoryError> {
        let record = connection
            .query_row(
                "SELECT o.scope,o.obligation_type,o.authority_level,o.lifecycle,o.description,o.must_resolve_before_chapter,o.prerequisites_json,o.decision_ref,o.resume_condition_json,o.source_artifact_id,o.source_version,(SELECT d.payload_json FROM decisions d WHERE d.project_id=o.project_id AND d.decision_id=o.decision_ref LIMIT 1) FROM narrative_obligations o WHERE o.project_id=?1 AND o.obligation_id=?2",
                libsql_rusqlite::params![project_id, obligation_id],
                |row| {
                    let prerequisites: String = row.get(6)?;
                    let resume_condition: String = row.get(8)?;
                    let promotion_payload: Option<String> = row.get(11)?;
                    let fulfillment_object = promotion_payload
                        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                        .and_then(|payload| payload.get("fulfillmentObject").and_then(Value::as_str).map(ToOwned::to_owned));
                    Ok(ResponsibilityAdjustmentRecord {
                        obligation_id: obligation_id.to_owned(),
                        scope: row.get(0)?,
                        obligation_type: row.get(1)?,
                        authority_level: row.get(2)?,
                        lifecycle: row.get(3)?,
                        description: row.get(4)?,
                        must_resolve_before_chapter: row.get(5)?,
                        prerequisites: serde_json::from_str(&prerequisites)
                            .unwrap_or_else(|_| Value::Array(vec![])),
                        decision_ref: row.get(7)?,
                        resume_condition: serde_json::from_str(&resume_condition)
                            .unwrap_or_else(|_| Value::Object(Default::default())),
                        fulfillment_object,
                        source_artifact_id: row.get(9)?,
                        source_version: row.get(10)?,
                        evidence_refs: Vec::new(),
                    })
                },
            )
            .optional()?;
        match record {
            Some(mut record) => {
                record.evidence_refs =
                    Self::obligation_evidence_refs(connection, project_id, obligation_id)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    pub fn responsibility_adjustment_is_user_editable(
        connection: &Connection,
        project_id: &str,
        obligation_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        let thread_ref = format!("thread:{obligation_id}");
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM decisions d WHERE d.project_id=?1 AND d.source_ref=?2 AND d.decision_type='THREAD_PROMOTION_RULE' AND d.outcome IN ('B','C') AND json_extract(d.payload_json,'$.ruleVersion')=?3) OR EXISTS(SELECT 1 FROM decisions d WHERE d.project_id=?1 AND d.source_ref=?2 AND d.decision_type='USER_THREAD_RESPONSIBILITY_ADJUSTMENT' AND d.outcome='UPGRADE')",
                libsql_rusqlite::params![
                    project_id,
                    thread_ref,
                    crate::narrative::THREAD_PROMOTION_RULE_VERSION
                ],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn final_chapter_body_for_ref(
        connection: &Connection,
        project_id: &str,
        artifact_id: &str,
        version: i64,
    ) -> Result<Option<String>, NarrativeRepositoryError> {
        let raw: Option<String> = connection
            .query_row(
                "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.version=?3 AND a.artifact_type='FinalChapter'",
                libsql_rusqlite::params![project_id, artifact_id, version],
                |row| row.get(0),
            )
            .optional()
            .map_err(NarrativeRepositoryError::from)?;
        Ok(raw
            .and_then(|payload| serde_json::from_str::<Value>(&payload).ok())
            .and_then(|payload| {
                payload
                    .get("body")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            }))
    }

    pub fn continuity_thread_exists(
        connection: &Connection,
        project_id: &str,
        thread_id: &str,
    ) -> Result<bool, NarrativeRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2) OR EXISTS(SELECT 1 FROM foreshadows WHERE project_id=?1 AND foreshadow_id=?2) OR EXISTS(SELECT 1 FROM mystery_states WHERE project_id=?1 AND mystery_id=?2)",
                libsql_rusqlite::params![project_id, thread_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn obligation_evidence_refs(
        connection: &Connection,
        project_id: &str,
        obligation_id: &str,
    ) -> Result<Vec<crate::generated::types::EvidenceRef>, NarrativeRepositoryError> {
        let mut grouped = Self::obligation_evidence_refs_by_ids(
            connection,
            project_id,
            &[obligation_id.to_owned()],
        )?;
        Ok(grouped.remove(obligation_id).unwrap_or_default())
    }

    fn obligation_evidence_refs_by_ids(
        connection: &Connection,
        project_id: &str,
        obligation_ids: &[String],
    ) -> Result<HashMap<String, Vec<crate::generated::types::EvidenceRef>>, NarrativeRepositoryError>
    {
        let mut grouped = obligation_ids
            .iter()
            .cloned()
            .map(|id| (id, Vec::new()))
            .collect::<HashMap<_, _>>();
        if obligation_ids.is_empty() {
            return Ok(grouped);
        }

        let ids_json = serde_json::to_string(obligation_ids)?;
        let mut evidence_statement = connection.prepare(
            "SELECT e.obligation_id,e.artifact_id,e.version,p.payload_json FROM json_each(?2) requested CROSS JOIN obligation_evidence e INDEXED BY idx_obligation_evidence_project_obligation_created JOIN artifact_payloads p ON p.artifact_id=e.artifact_id AND p.version=e.version WHERE e.project_id=?1 AND e.obligation_id=requested.value ORDER BY e.obligation_id,e.created_at,e.evidence_id",
        )?;
        let evidence_rows = evidence_statement.query_map(
            libsql_rusqlite::params![project_id, ids_json],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        for row in evidence_rows {
            let (obligation_id, artifact_id, version, raw_payload) = row?;
            let payload: Value = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
            let evidence = payload
                .get("validatedObligationEvidence")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find(|item| {
                        item.get("obligationId").and_then(Value::as_str)
                            == Some(obligation_id.as_str())
                    })
                });
            if let Some(references) = grouped.get_mut(&obligation_id) {
                references.push(crate::generated::types::EvidenceRef {
                    artifact_id,
                    version,
                    range_start: evidence
                        .and_then(|item| item.get("rangeStart"))
                        .and_then(Value::as_i64),
                    range_end: evidence
                        .and_then(|item| item.get("rangeEnd"))
                        .and_then(Value::as_i64),
                    quote: evidence
                        .and_then(|item| item.get("quote"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
        drop(evidence_statement);

        let mut source_statement = connection.prepare(
            "SELECT o.obligation_id,a.artifact_id,a.version,a.artifact_type,p.payload_json FROM json_each(?2) requested CROSS JOIN narrative_obligations o JOIN artifacts a ON a.project_id=o.project_id AND a.artifact_id=o.source_artifact_id AND a.version=o.source_version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE o.project_id=?1 AND o.obligation_id=requested.value ORDER BY o.obligation_id",
        )?;
        let source_rows =
            source_statement.query_map(libsql_rusqlite::params![project_id, ids_json], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
        for row in source_rows {
            let (obligation_id, artifact_id, version, artifact_type, raw_payload) = row?;
            let payload: Value = serde_json::from_str(&raw_payload).unwrap_or(Value::Null);
            let references = grouped.entry(obligation_id.clone()).or_default();
            if artifact_type == "FinalChapter" {
                if let Some(item) = payload
                    .get("newResponsibilityEvidence")
                    .and_then(Value::as_array)
                    .and_then(|items| {
                        items.iter().find(|item| {
                            item.get("obligationId").and_then(Value::as_str)
                                == Some(obligation_id.as_str())
                        })
                    })
                {
                    let reference = crate::generated::types::EvidenceRef {
                        artifact_id,
                        version,
                        range_start: item.get("rangeStart").and_then(Value::as_i64),
                        range_end: item.get("rangeEnd").and_then(Value::as_i64),
                        quote: item
                            .get("quote")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    };
                    if !references.contains(&reference) {
                        references.push(reference);
                    }
                }
            } else if artifact_type == "NarrativeObligationSet" {
                let historical = payload
                    .get("sourceEvidenceRefs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for value in historical {
                    if let Ok(reference) =
                        serde_json::from_value::<crate::generated::types::EvidenceRef>(value)
                    {
                        if !references.contains(&reference) {
                            references.push(reference);
                        }
                    }
                }
            }
        }
        Ok(grouped)
    }

    /// Read timing data from the continuity authority for the planning
    /// projection. Only explicit deadlines count as overdue; missing timing
    /// data remains unknown rather than being treated as overdue.
    pub fn continuity_pressure_records(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<ContinuityPressureRecord>, NarrativeRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT o.obligation_id,o.lifecycle,o.authority_level,o.must_resolve_before_chapter,o.resume_condition_json,c.chapter_no FROM narrative_obligations o LEFT JOIN chapters c ON c.project_id=o.project_id AND c.chapter_id=o.last_touched_chapter_id WHERE o.project_id=?1 AND o.lifecycle IN ('APPROVED','PLANTED','PROGRESSING') ORDER BY o.obligation_id",
        )?;
        let rows = statement.query_map([project_id], |row| {
            let resume_condition: String = row.get(4)?;
            let deadline: Option<String> = row.get(3)?;
            Ok(ContinuityPressureRecord {
                thread_id: format!("thread:{}", row.get::<_, String>(0)?),
                lifecycle: row.get(1)?,
                authority_level: row.get(2)?,
                must_resolve_before_chapter: deadline.and_then(|value| {
                    value
                        .strip_prefix("chapter:")
                        .unwrap_or(&value)
                        .parse::<i64>()
                        .ok()
                }),
                last_touched_chapter_no: row.get(5)?,
                resume_condition: serde_json::from_str(&resume_condition)
                    .unwrap_or_else(|_| Value::Object(Default::default())),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanonFactTimingRecord {
    pub story_time: Option<String>,
    pub source_chapter_no: Option<i64>,
}
