use crate::endgame::{relevant_state_digest, CompletionInput};
use crate::generated::types::ArtifactRef;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum EndgameRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct EndgameRepository;

impl EndgameRepository {
    pub fn latest_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
        scope_level: Option<&str>,
        scope_key: Option<&str>,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        let row = match (scope_level, scope_key) {
            (Some(level), Some(key)) => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 AND scope_level=?3 AND scope_key=?4",
                    libsql_rusqlite::params![project_id, artifact_type, level, key],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
            (Some(level), None) => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 AND scope_level=?3 ORDER BY version DESC, scope_key LIMIT 1",
                    libsql_rusqlite::params![project_id, artifact_type, level],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
            _ => connection
                .query_row(
                    "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2 ORDER BY version DESC, scope_key LIMIT 1",
                    libsql_rusqlite::params![project_id, artifact_type],
                    |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
                )
                .optional()?,
        };
        Ok(row)
    }

    pub fn completion_already_committed(
        connection: &Connection,
        project_id: &str,
    ) -> Result<bool, EndgameRepositoryError> {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookExitState' AND scope_level='PROJECT') AND EXISTS(SELECT 1 FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookCompletionReport' AND scope_level='PROJECT') AND EXISTS(SELECT 1 FROM domain_receipts WHERE project_id=?1 AND domain='B14_BOOK_COMPLETION')",
            [project_id],
            |row| row.get(0),
        )?)
    }

    pub fn latest_project_head_ref(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        Self::latest_head_ref(connection, project_id, artifact_type, Some("PROJECT"), None)
    }

    pub fn final_volume_exit_ref(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<ArtifactRef>, EndgameRepositoryError> {
        let volume_id: Option<String> = connection
            .query_row(
                "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no DESC, volume_id DESC LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(volume_id) = volume_id else {
            return Ok(None);
        };
        connection
            .query_row(
                "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumeExitState' AND scope_level='VOLUME' AND json_extract(scope_key,'$.volumeId')=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn final_chapter_summary(
        connection: &Connection,
        project_id: &str,
    ) -> Result<(i64, Option<ArtifactRef>, Option<ArtifactRef>), EndgameRepositoryError> {
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND status='COMMITTED' AND final_artifact_id IS NOT NULL AND final_version IS NOT NULL",
            [project_id],
            |row| row.get(0),
        )?;
        let first = connection.query_row(
            "SELECT final_artifact_id,final_version,a.payload_hash FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version WHERE c.project_id=?1 AND c.status='COMMITTED' AND c.final_artifact_id IS NOT NULL AND c.final_version IS NOT NULL ORDER BY c.chapter_no LIMIT 1",
            [project_id],
            |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
        ).optional()?;
        let last = connection.query_row(
            "SELECT final_artifact_id,final_version,a.payload_hash FROM chapters c JOIN artifacts a ON a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version WHERE c.project_id=?1 AND c.status='COMMITTED' AND c.final_artifact_id IS NOT NULL AND c.final_version IS NOT NULL ORDER BY c.chapter_no DESC LIMIT 1",
            [project_id],
            |row| Ok(ArtifactRef { artifact_id: row.get(0)?, version: row.get(1)?, payload_hash: row.get(2)? }),
        ).optional()?;
        Ok((count, first, last))
    }

    pub fn read_completion_input(
        connection: &Connection,
        project_id: &str,
    ) -> Result<(CompletionInput, i64, Option<Value>), EndgameRepositoryError> {
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1 AND maintenance_state='READY'",
            [project_id],
            |row| row.get(0),
        )?;

        let unresolved_hard_obligations: i64 = connection.query_row(
            "SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1 AND authority_level IN ('A','B') AND lifecycle NOT IN ('RESOLVED','ABANDONED','SUPERSEDED')",
            [project_id],
            |row| row.get(0),
        )?;
        let open_hard_continuity: i64 = connection.query_row(
            "SELECT COUNT(*) FROM character_arc_states WHERE project_id=?1 AND state NOT IN ('RESOLVED','CLOSED','DONE')",
            [project_id],
            |row| row.get(0),
        )?;
        let blocked_required_tasks: i64 = connection.query_row(
            "SELECT COUNT(*) FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' AND COALESCE(json_extract(value_json,'$.status'),json_extract(payload_json,'$.status')) IN ('NOT_STARTED','ACTIVE','BLOCKED') AND COALESCE(json_extract(value_json,'$.class'),json_extract(payload_json,'$.class')) IN ('REQUIRED','CLIMAX','RESERVED')",
            [project_id],
            |row| row.get(0),
        )?;
        let pending_relevant_change: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM change_requests WHERE project_id=?1 AND status IN ('DRAFT','IMPACT_READY','PENDING_APPROVAL','APPROVED','APPLYING')) OR EXISTS(SELECT 1 FROM retcon_plans WHERE project_id=?1 AND status IN ('DRAFT','PENDING_APPROVAL','APPROVED'))",
            [project_id],
            |row| row.get(0),
        )?;
        let plan_stale: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM stale_queue WHERE project_id=?1 AND processed=0)",
            [project_id],
            |row| row.get(0),
        )?;

        let final_volume: Option<(String, String)> = connection
            .query_row(
                "SELECT volume_id,status FROM volumes WHERE project_id=?1 ORDER BY volume_no DESC, volume_id DESC LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let final_volume_closed = final_volume
            .as_ref()
            .is_some_and(|(_, status)| status == "CLOSED");
        let volume_exit_state_available = final_volume.as_ref().is_some_and(|(volume_id, _)| {
            connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumeExitState' AND (scope_key=?2 OR scope_key=json_object('level','VOLUME','volumeId',?2)))",
                    libsql_rusqlite::params![project_id, volume_id],
                    |row| row.get(0),
                )
                .unwrap_or(false)
        });

        let export_integrity_failures: i64 = connection.query_row(
            "SELECT COUNT(*) FROM chapters c WHERE c.project_id=?1 AND c.status='COMMITTED' AND (c.final_artifact_id IS NULL OR c.final_version IS NULL OR NOT EXISTS(SELECT 1 FROM artifacts a WHERE a.project_id=c.project_id AND a.artifact_id=c.final_artifact_id AND a.version=c.final_version AND a.artifact_type='FinalChapter' AND a.artifact_status='COMMITTED'))",
            [project_id],
            |row| row.get(0),
        )?;

        let endgame_contract: Option<Value> = connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='EndgameContract' ORDER BY h.version DESC LIMIT 1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload))
            .transpose()?;
        let requested_ending_confirmed = endgame_contract
            .as_ref()
            .and_then(|payload| payload.pointer("/requestedEnding/confirmationSource"))
            .and_then(Value::as_str)
            == Some("USER_CONFIRMED");

        let book_plan: Option<Value> = connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan' ORDER BY h.version DESC LIMIT 1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload))
            .transpose()?;
        let unmapped_mysteries = book_plan
            .as_ref()
            .and_then(|value| value.get("majorMysteryGoals"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("mappingRefs")
                            .and_then(Value::as_array)
                            .is_none_or(|refs| refs.is_empty())
                    })
                    .count() as i64
            })
            .unwrap_or(0);
        let unfulfilled_hard_reservations = book_plan
            .as_ref()
            .and_then(|value| value.get("futureReservations"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item.get("authorityLevel").and_then(Value::as_str),
                            Some("A" | "B")
                        ) && !matches!(
                            item.get("status").and_then(Value::as_str),
                            Some("FULFILLED" | "RESOLVED" | "PAID" | "CLOSED")
                        )
                    })
                    .count() as i64
            })
            .unwrap_or(0);

        let mut input = CompletionInput {
            source_available: true,
            requested_ending_confirmed,
            endgame_contract_fresh: false,
            unmapped_mysteries,
            unresolved_hard_obligations,
            unresolved_hard_payoffs: 0,
            unfulfilled_hard_reservations,
            open_hard_continuity,
            blocked_required_tasks,
            final_volume_closed,
            volume_exit_state_available,
            pending_relevant_change,
            plan_stale,
            export_integrity_failures,
            soft_advisories: 0,
            detail_limit: 30,
        };
        let current_digest = relevant_state_digest(&input)?;
        input.endgame_contract_fresh = endgame_contract
            .as_ref()
            .and_then(|payload| payload.get("compiledFromRelevantStateDigest"))
            .and_then(Value::as_str)
            == Some(current_digest.as_str())
            && endgame_contract
                .as_ref()
                .and_then(|payload| payload.get("compiledFromProjectRevision"))
                .and_then(Value::as_i64)
                == Some(revision);
        Ok((input, revision, endgame_contract))
    }
}
