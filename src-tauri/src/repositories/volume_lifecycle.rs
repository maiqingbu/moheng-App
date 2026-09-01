use libsql_rusqlite::{Connection, OptionalExtension};

#[derive(Debug, thiserror::Error)]
pub enum VolumeLifecycleRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
}

#[derive(Debug, Clone)]
pub struct VolumeMetadata {
    pub book_id: String,
    pub volume_no: i64,
    pub status: String,
    pub chapter_start: i64,
    pub chapter_end: i64,
}

pub struct VolumeLifecycleRepository;

impl VolumeLifecycleRepository {
    pub fn project_revision(
        connection: &Connection,
        project_id: &str,
    ) -> Result<i64, VolumeLifecycleRepositoryError> {
        connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn volume_metadata(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<VolumeMetadata>, VolumeLifecycleRepositoryError> {
        connection
            .query_row(
                "SELECT book_id, volume_no, status, chapter_start, chapter_end FROM volumes WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| {
                    Ok(VolumeMetadata {
                        book_id: row.get(0)?,
                        volume_no: row.get(1)?,
                        status: row.get(2)?,
                        chapter_start: row.get(3)?,
                        chapter_end: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn committed_chapters(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<i64, VolumeLifecycleRepositoryError> {
        connection
            .query_row(
                "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND volume_id=?2 AND status='COMMITTED'",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn upsert_planning_projection(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        projection_revision: i64,
        state_digest: &str,
        projection_digest: &str,
        payload_json: &str,
        updated_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project_id,volume_id) DO UPDATE SET projection_revision=excluded.projection_revision,state_digest=excluded.state_digest,projection_digest=excluded.projection_digest,payload_json=excluded.payload_json,updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, volume_id, projection_revision, state_digest, projection_digest, payload_json, updated_at],
        )?;
        Ok(())
    }

    pub fn update_plan_index(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        artifact_id: &str,
        version: i64,
        target_chapters: i64,
        updated_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,?4,'APPROVED',?5,?6) ON CONFLICT(project_id,volume_id) DO UPDATE SET plan_artifact_id=excluded.plan_artifact_id,plan_version=excluded.plan_version,plan_state=excluded.plan_state,target_chapters=excluded.target_chapters,updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, volume_id, artifact_id, version, target_chapters, updated_at],
        )?;
        Ok(())
    }

    pub fn update_planning_ledger(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        coverage_state: &str,
        repair_state: &str,
        updated_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at) VALUES (?1,?2,'OPEN','APPROVED',?3,'[]','[]','[]',?4,?5) ON CONFLICT(project_id,volume_id) DO UPDATE SET coverage_state=excluded.coverage_state,repair_state=excluded.repair_state,updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, volume_id, coverage_state, repair_state, updated_at],
        )?;
        Ok(())
    }

    pub fn has_evidence_ref(
        connection: &Connection,
        project_id: &str,
        evidence_ref: &str,
    ) -> Result<bool, VolumeLifecycleRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM canon_evidence WHERE project_id=?1 AND evidence_id=?2) OR EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND artifact_id=?2 AND artifact_status='COMMITTED')",
                libsql_rusqlite::params![project_id, evidence_ref],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn record_route_replan(
        connection: &Connection,
        replan_id: &str,
        project_id: &str,
        volume_id: &str,
        previous_artifact_id: Option<&str>,
        previous_version: Option<i64>,
        new_artifact_id: &str,
        new_version: i64,
        trigger: &str,
        reason: &str,
        source_revision: i64,
        input_refs_json: &str,
        receipt_id: &str,
        created_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volume_route_replans(replan_id,project_id,volume_id,previous_artifact_id,previous_version,new_artifact_id,new_version,trigger,reason,source_revision,input_refs_json,validation_status,project_command_receipt_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'VALIDATED',?12,?13)",
            libsql_rusqlite::params![replan_id, project_id, volume_id, previous_artifact_id, previous_version, new_artifact_id, new_version, trigger, reason, source_revision, input_refs_json, receipt_id, created_at],
        )?;
        Ok(())
    }

    pub fn record_volume_close(
        connection: &Connection,
        close_id: &str,
        project_id: &str,
        volume_id: &str,
        exit_artifact_id: &str,
        exit_version: i64,
        close_revision: i64,
        next_volume_eligibility: &str,
        blocking_refs_json: &str,
        evidence_refs_json: &str,
        receipt_id: &str,
        created_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volume_close_records(close_id,project_id,volume_id,exit_artifact_id,exit_version,close_revision,status,next_volume_eligibility,blocking_refs_json,evidence_refs_json,project_command_receipt_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,'CLOSED',?7,?8,?9,?10,?11)",
            libsql_rusqlite::params![close_id, project_id, volume_id, exit_artifact_id, exit_version, close_revision, next_volume_eligibility, blocking_refs_json, evidence_refs_json, receipt_id, created_at],
        )?;
        Ok(())
    }

    pub fn close_eligibility(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<Option<String>, VolumeLifecycleRepositoryError> {
        connection
            .query_row(
                "SELECT next_volume_eligibility FROM volume_close_records WHERE project_id=?1 AND volume_id=?2 ORDER BY close_revision DESC LIMIT 1",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_next_volume(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        book_id: &str,
        volume_no: i64,
        title: &str,
        chapter_start: i64,
        chapter_end: i64,
        target_words: i64,
        now: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'PLANNING',?6,?7,?8,?9,?9)",
            libsql_rusqlite::params![volume_id, project_id, book_id, volume_no, title, chapter_start, chapter_end, target_words, now],
        )?;
        Ok(())
    }

    pub fn record_handoff(
        connection: &Connection,
        handoff_id: &str,
        project_id: &str,
        from_volume_id: &str,
        to_volume_id: &str,
        from_exit_artifact_id: &str,
        input_refs_json: &str,
        receipt_id: &str,
        created_at: &str,
    ) -> Result<(), VolumeLifecycleRepositoryError> {
        connection.execute(
            "INSERT INTO volume_handoffs(handoff_id,project_id,from_volume_id,to_volume_id,from_exit_artifact_id,input_refs_json,eligibility,project_command_receipt_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,'ELIGIBLE',?7,?8)",
            libsql_rusqlite::params![handoff_id, project_id, from_volume_id, to_volume_id, from_exit_artifact_id, input_refs_json, receipt_id, created_at],
        )?;
        Ok(())
    }
}
