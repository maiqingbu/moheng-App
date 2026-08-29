use crate::harness::SupportCheckpoint;
use libsql_rusqlite::{Connection, OptionalExtension};

#[derive(Debug, thiserror::Error)]
pub enum SupportRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn save_checkpoint(
    connection: &Connection,
    checkpoint: &SupportCheckpoint,
) -> Result<(), SupportRepositoryError> {
    connection.execute(
        "INSERT INTO support_checkpoints(checkpoint_id,session_id,project_id,project_revision,question_digest,plan_digest,completed_step_ids_json,capability_spec_digests_json,evidence_refs_json,last_result_digests_json,requested_mode,requested_scope,plan_steps_json,pending_authorization_ref,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(checkpoint_id) DO UPDATE SET completed_step_ids_json=excluded.completed_step_ids_json, capability_spec_digests_json=excluded.capability_spec_digests_json, evidence_refs_json=excluded.evidence_refs_json, last_result_digests_json=excluded.last_result_digests_json, requested_mode=excluded.requested_mode, requested_scope=excluded.requested_scope, plan_steps_json=excluded.plan_steps_json, pending_authorization_ref=excluded.pending_authorization_ref, created_at=excluded.created_at",
        libsql_rusqlite::params![
            checkpoint.checkpoint_id,
            checkpoint.session_id,
            checkpoint.project_id,
            checkpoint.project_revision,
            checkpoint.question_digest,
            checkpoint.plan_digest,
            serde_json::to_string(&checkpoint.completed_step_ids)?,
            serde_json::to_string(&checkpoint.capability_spec_digests)?,
            serde_json::to_string(&checkpoint.evidence_refs)?,
            serde_json::to_string(&checkpoint.last_result_digests)?,
            checkpoint.requested_mode,
            checkpoint.requested_scope,
            serde_json::to_string(&checkpoint.plan_steps)?,
            checkpoint.pending_authorization_ref,
            checkpoint.created_at,
        ],
    )?;
    Ok(())
}

pub fn load_checkpoint(
    connection: &Connection,
    project_id: &str,
    checkpoint_id: &str,
) -> Result<Option<SupportCheckpoint>, SupportRepositoryError> {
    let row = connection
        .query_row(
            "SELECT checkpoint_id,session_id,project_id,project_revision,question_digest,plan_digest,completed_step_ids_json,capability_spec_digests_json,evidence_refs_json,last_result_digests_json,requested_mode,requested_scope,plan_steps_json,pending_authorization_ref,created_at FROM support_checkpoints WHERE project_id=?1 AND checkpoint_id=?2",
            [project_id, checkpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, String>(14)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(
            checkpoint_id,
            session_id,
            project_id,
            project_revision,
            question_digest,
            plan_digest,
            completed_step_ids,
            capability_spec_digests,
            evidence_refs,
            last_result_digests,
            requested_mode,
            requested_scope,
            plan_steps,
            pending_authorization_ref,
            created_at,
        )| {
            Ok(SupportCheckpoint {
                checkpoint_id,
                session_id,
                project_id,
                project_revision,
                question_digest,
                plan_digest,
                completed_step_ids: serde_json::from_str(&completed_step_ids)?,
                capability_spec_digests: serde_json::from_str(&capability_spec_digests)?,
                evidence_refs: serde_json::from_str(&evidence_refs)?,
                last_result_digests: serde_json::from_str(&last_result_digests)?,
                requested_mode,
                requested_scope,
                plan_steps: serde_json::from_str(&plan_steps)?,
                pending_authorization_ref,
                created_at,
            })
        },
    )
    .transpose()
}
