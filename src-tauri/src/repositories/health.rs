//! Repository boundary for the B13 long-form health projection.
//!
//! This module only reads the already frozen Authority and Projection tables.
//! It deliberately returns bounded, typed inputs; factor policy and user-facing
//! presentation live in `crate::health`.

use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;

const ISSUE_ROW_BOUND: i64 = 51;
const TREND_ROW_BOUND: i64 = 20;

pub const FORMAL_ROUTE_ROWS_SQL: &str = r#"
SELECT v.volume_id,v.volume_no,v.title,v.updated_at,i.plan_state,i.plan_artifact_id,i.plan_version,i.updated_at,
       vp.projection_revision,vp.state_digest,
       CASE WHEN json_valid(vp.payload_json)=1 THEN json_object(
           'planningPressure',json_extract(vp.payload_json,'$.planningPressure'),
           'remainingChapterCapacity',json_extract(vp.payload_json,'$.remainingChapterCapacity'),
           'generatedRevision',json_extract(vp.payload_json,'$.generatedRevision'),
           'stateDigest',vp.state_digest)
            ELSE vp.payload_json END,
       vp.updated_at
FROM volumes v
LEFT JOIN volume_plan_index i ON i.project_id=v.project_id AND i.volume_id=v.volume_id
LEFT JOIN volume_planning_state_projection vp ON vp.project_id=v.project_id AND vp.volume_id=v.volume_id
WHERE v.project_id=?1
  AND (i.volume_id IS NOT NULL OR EXISTS (
      SELECT 1 FROM canonical_heads h WHERE h.project_id=v.project_id
        AND h.artifact_type='VolumePlan'
        AND h.scope_key IN (v.volume_id,json_object('level','VOLUME','volumeId',v.volume_id))))
ORDER BY v.book_id,v.volume_no
LIMIT ?2
"#;
pub const FORMAL_ROUTE_SUMMARY_SQL: &str = r#"
SELECT
    (SELECT COUNT(*) FROM (
        SELECT i.volume_id FROM volume_plan_index i WHERE i.project_id=?1
        UNION
        SELECT CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END
        FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='VolumePlan'
    )),
    COALESCE(SUM(is_stale),0),
    COALESCE(SUM(CASE WHEN capacity=0 AND required_tasks_remaining>0 THEN 1 ELSE 0 END),0),
    COALESCE(SUM(CASE WHEN capacity BETWEEN 1 AND 8 AND required_tasks_remaining>0 THEN 1 ELSE 0 END),0),
    COALESCE(SUM(is_unavailable),0),
    CASE WHEN EXISTS(
        SELECT 1 FROM health_projection_state s JOIN project_meta m ON m.project_id=s.project_id
        WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=m.revision
          AND NOT EXISTS(SELECT 1 FROM health_projection_dirty d WHERE d.project_id=s.project_id)
    ) THEN (
        SELECT COUNT(*) FROM health_issue_projection i
        WHERE i.project_id=?1 AND i.factor_code='TASK_BLOCKAGE' AND i.severity='risk'
          AND NOT EXISTS(SELECT 1 FROM health_issue_projection earlier
                         WHERE earlier.project_id=i.project_id AND earlier.issue_id=i.issue_id
                           AND earlier.volume_id<i.volume_id)
    ) ELSE 0 END,
    CASE WHEN EXISTS(
        SELECT 1 FROM health_projection_state s JOIN project_meta m ON m.project_id=s.project_id
        WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=m.revision
          AND NOT EXISTS(SELECT 1 FROM health_projection_dirty d WHERE d.project_id=s.project_id)
    ) THEN (
        SELECT COUNT(*) FROM health_issue_projection i
        WHERE i.project_id=?1 AND i.factor_code='TASK_BLOCKAGE' AND i.severity='attention'
          AND NOT EXISTS(SELECT 1 FROM health_issue_projection earlier
                         WHERE earlier.project_id=i.project_id AND earlier.issue_id=i.issue_id
                           AND earlier.volume_id<i.volume_id)
    ) ELSE 0 END,
    COALESCE(SUM(CASE WHEN task_summary_available=0 THEN 1 ELSE 0 END),0),
    (SELECT COUNT(*) FROM (
        SELECT task_id FROM health_task_scope_index WHERE project_id=?1
        GROUP BY task_id HAVING COUNT(DISTINCT task_class)>1
    ))
FROM health_route_scope_projection
WHERE project_id=?1
"#;

pub const HEALTH_ROUTE_SCOPE_SQL: &str = r#"
SELECT v.volume_no,v.title,v.updated_at,
       COALESCE(i.plan_state,'APPROVED'),
       COALESCE(h.artifact_id,i.plan_artifact_id),
       COALESCE(h.version,i.plan_version),
       h.payload_hash,
       vp.projection_revision,vp.state_digest,vp.payload_json,vp.updated_at,
       a.artifact_type,a.artifact_status,a.payload_hash,p.payload_json
FROM volumes v
LEFT JOIN volume_plan_index i ON i.project_id=v.project_id AND i.volume_id=v.volume_id
LEFT JOIN canonical_heads h ON h.project_id=v.project_id AND h.artifact_type='VolumePlan'
  AND h.scope_key IN (v.volume_id,json_object('level','VOLUME','volumeId',v.volume_id))
LEFT JOIN volume_planning_state_projection vp ON vp.project_id=v.project_id AND vp.volume_id=v.volume_id
LEFT JOIN artifacts a ON a.project_id=v.project_id
  AND a.artifact_id=COALESCE(h.artifact_id,i.plan_artifact_id)
  AND a.version=COALESCE(h.version,i.plan_version)
LEFT JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version
WHERE v.project_id=?1 AND v.volume_id=?2
  AND (i.volume_id IS NOT NULL OR h.artifact_id IS NOT NULL)
LIMIT 1
"#;

pub const HEALTH_SOURCE_TIME_SQL: &str = "SELECT MAX(source_time) FROM (SELECT updated_at AS source_time FROM project_meta WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM chapters WHERE project_id=?1 AND status='COMMITTED' UNION ALL SELECT MAX(updated_at) FROM narrative_obligations WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM foreshadows WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM reader_desires WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM volume_plan_index WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM volume_planning_state_projection WHERE project_id=?1 UNION ALL SELECT MAX(updated_at) FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' UNION ALL SELECT MAX(updated_at) FROM model_runs WHERE project_id=?1 UNION ALL SELECT MAX(recorded_at) FROM model_usage WHERE project_id=?1 UNION ALL SELECT MAX(created_at) FROM budget_reservations WHERE project_id=?1 UNION ALL SELECT MAX(sampled_at) FROM quality_trend_projection WHERE project_id=?1)";

pub const HEALTH_ISSUE_TOTAL_SQL: &str = r#"
SELECT
    CASE WHEN EXISTS(
        SELECT 1 FROM health_projection_state s JOIN project_meta m ON m.project_id=s.project_id
        WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=m.revision
    ) THEN (SELECT COUNT(DISTINCT issue_id) FROM health_issue_projection WHERE project_id=?1 AND (?3 IS NULL OR factor_code=?3))
    ELSE 0 END
    + CASE WHEN ?3 IS NULL OR ?3='OBLIGATION_OVERDUE' THEN (
        SELECT COUNT(*) FROM narrative_obligations o
        LEFT JOIN chapters due ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
        WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL))
      ) ELSE 0 END
    + CASE WHEN ?3 IS NULL OR ?3='FORESHADOW_AGING' THEN (
        SELECT COUNT(*) FROM foreshadows f
        LEFT JOIN chapters due ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
        WHERE f.project_id=?1 AND f.status IN ('PLANTED','REINFORCED','STALE')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (f.status='STALE' AND due.chapter_no>?2)
               OR (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL))
      ) ELSE 0 END
    + CASE WHEN ?3 IS NULL OR ?3='READER_DESIRE' THEN (
        SELECT COUNT(*) FROM reader_desires d
        LEFT JOIN chapters due ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
        WHERE d.project_id=?1 AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (TRIM(COALESCE(d.expected_payoff_chapter,''))<>'' AND due.chapter_id IS NULL))
      ) ELSE 0 END
"#;

pub const HEALTH_ISSUE_PAGE_SQL: &str = r#"
WITH page_candidates AS (
    SELECT * FROM (
        SELECT i.issue_id,i.factor_code,i.severity,i.title,i.detail,i.location,i.due_chapter,
               i.recommended_destination,i.source_kind,i.source_ref,i.severity_rank,i.factor_rank,i.due_sort
        FROM health_issue_projection i
        WHERE i.project_id=?1 AND (?8 IS NULL OR i.factor_code=?8)
          AND NOT EXISTS(SELECT 1 FROM health_issue_projection earlier
                         WHERE earlier.project_id=i.project_id AND earlier.issue_id=i.issue_id
                           AND earlier.volume_id<i.volume_id)
          AND EXISTS(SELECT 1 FROM health_projection_state s JOIN project_meta m ON m.project_id=s.project_id
                     WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=m.revision
                       AND NOT EXISTS(SELECT 1 FROM health_projection_dirty d WHERE d.project_id=s.project_id))
          AND (?3<0 OR i.severity_rank>?3
               OR (i.severity_rank=?3 AND i.factor_rank>?4)
               OR (i.severity_rank=?3 AND i.factor_rank=?4 AND i.due_sort>?5)
               OR (i.severity_rank=?3 AND i.factor_rank=?4 AND i.due_sort=?5 AND i.issue_id>?6))
        ORDER BY i.severity_rank,i.factor_rank,i.due_sort,i.issue_id,i.volume_id LIMIT ?7
    )
    UNION ALL
    SELECT * FROM (
        SELECT 'health:obligation:'||o.obligation_id,
               'OBLIGATION_OVERDUE',
               CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR o.authority_level='C' THEN 'attention' ELSE 'risk' END,
               CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '剧情义务来源暂不可用' ELSE '剧情义务需要处理' END,
               CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '关联章节记录暂时无法读取，未将其误判为逾期。'
                    WHEN TRIM(COALESCE(o.description,''))='' THEN '这项剧情安排已经到达预定处理节点。'
                    ELSE o.description END,
               CASE WHEN due.chapter_no IS NULL THEN '相关章节' ELSE '第 '||due.chapter_no||' 章附近' END,
               due.chapter_no,'卷工作区','obligation',o.obligation_id,
               CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR o.authority_level='C' THEN 1 ELSE 0 END,
               0,COALESCE(due.chapter_no,9223372036854775807)
        FROM narrative_obligations o
        LEFT JOIN chapters due ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
        WHERE o.project_id=?1 AND (?8 IS NULL OR ?8='OBLIGATION_OVERDUE')
          AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL))
          AND (?3<0 OR
               CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR o.authority_level='C' THEN 1 ELSE 0 END>?3
               OR (CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR o.authority_level='C' THEN 1 ELSE 0 END=?3
                   AND 0>?4)
               OR (CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR o.authority_level='C' THEN 1 ELSE 0 END=?3
                   AND 0=?4 AND COALESCE(due.chapter_no,9223372036854775807)>?5)
               OR (CASE WHEN (TRIM(COALESCE(o.must_resolve_before_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR o.authority_level='C' THEN 1 ELSE 0 END=?3
                   AND 0=?4 AND COALESCE(due.chapter_no,9223372036854775807)=?5
                   AND ('health:obligation:'||o.obligation_id)>?6))
        ORDER BY 11,12,13,1 LIMIT ?7
    )
    UNION ALL
    SELECT * FROM (
        SELECT 'health:foreshadow:'||f.foreshadow_id,
               'FORESHADOW_AGING',
               CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR (f.status='STALE' AND due.chapter_no>?2) THEN 'attention' ELSE 'risk' END,
               CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '伏笔来源暂不可用'
                    WHEN due.chapter_no<=?2 THEN '伏笔已到回收节点' ELSE '伏笔需要重新关注' END,
               CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '关联回收章节记录暂时无法读取，未将其误判为逾期。'
                    WHEN TRIM(COALESCE(f.intent,''))='' THEN '这条伏笔需要在卷工作区核对后续安排。'
                    ELSE f.intent END,
               CASE WHEN due.chapter_no IS NULL THEN '相关章节' ELSE '第 '||due.chapter_no||' 章附近' END,
               due.chapter_no,'卷工作区','foreshadow',f.foreshadow_id,
               CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR (f.status='STALE' AND due.chapter_no>?2) THEN 1 ELSE 0 END,
               1,COALESCE(due.chapter_no,9223372036854775807)
        FROM foreshadows f
        LEFT JOIN chapters due ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
        WHERE f.project_id=?1 AND (?8 IS NULL OR ?8='FORESHADOW_AGING')
          AND f.status IN ('PLANTED','REINFORCED','STALE')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (f.status='STALE' AND due.chapter_no>?2)
               OR (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL))
          AND (?3<0 OR
               CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                          OR (f.status='STALE' AND due.chapter_no>?2) THEN 1 ELSE 0 END>?3
               OR (CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR (f.status='STALE' AND due.chapter_no>?2) THEN 1 ELSE 0 END=?3
                   AND 1>?4)
               OR (CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR (f.status='STALE' AND due.chapter_no>?2) THEN 1 ELSE 0 END=?3
                   AND 1=?4 AND COALESCE(due.chapter_no,9223372036854775807)>?5)
               OR (CASE WHEN (TRIM(COALESCE(f.recover_window_end_chapter,''))<>'' AND due.chapter_id IS NULL)
                              OR (f.status='STALE' AND due.chapter_no>?2) THEN 1 ELSE 0 END=?3
                   AND 1=?4 AND COALESCE(due.chapter_no,9223372036854775807)=?5
                   AND ('health:foreshadow:'||f.foreshadow_id)>?6))
        ORDER BY 11,12,13,1 LIMIT ?7
    )
    UNION ALL
    SELECT * FROM (
        SELECT 'health:reader-desire:'||d.desire_id,
               'READER_DESIRE','attention',
               CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '读者期待来源暂不可用' ELSE '读者期待需要回应' END,
               CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,''))<>'' AND due.chapter_id IS NULL
                    THEN '关联回应章节记录暂时无法读取，未将其误判为逾期。'
                    ELSE d.desire_type||' 已到预期回应章节，可在卷工作区核对安排。' END,
               CASE WHEN due.chapter_no IS NULL THEN '相关章节' ELSE '第 '||due.chapter_no||' 章附近' END,
               due.chapter_no,'卷工作区','reader-desire',d.desire_id,
               1,6,COALESCE(due.chapter_no,9223372036854775807)
        FROM reader_desires d
        LEFT JOIN chapters due ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
        WHERE d.project_id=?1 AND (?8 IS NULL OR ?8='READER_DESIRE')
          AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')
          AND ((due.chapter_no IS NOT NULL AND due.chapter_no<=?2)
               OR (TRIM(COALESCE(d.expected_payoff_chapter,''))<>'' AND due.chapter_id IS NULL))
          AND (?3<0 OR 1>?3 OR (1=?3 AND 6>?4)
               OR (1=?3 AND 6=?4 AND COALESCE(due.chapter_no,9223372036854775807)>?5)
               OR (1=?3 AND 6=?4 AND COALESCE(due.chapter_no,9223372036854775807)=?5
                   AND ('health:reader-desire:'||d.desire_id)>?6))
        ORDER BY 11,12,13,1 LIMIT ?7
    )
)
SELECT issue_id,factor_code,severity,title,detail,location,due_chapter,recommended_destination,
       source_kind,source_ref,severity_rank,factor_rank,due_sort
FROM page_candidates
ORDER BY severity_rank,factor_rank,due_sort,issue_id
LIMIT ?7
"#;

#[derive(Debug, thiserror::Error)]
pub enum HealthRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("authority: {0}")]
    Authority(String),
    #[error(transparent)]
    Narrative(#[from] crate::repositories::narrative::NarrativeRepositoryError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HEALTH_CURSOR_STALE")]
    CursorStale,
    #[error("HEALTH_SOURCE_REVISION_CHANGED_DURING_QUERY")]
    ConcurrentMutation,
}

#[derive(Debug, Clone)]
pub struct ObligationInput {
    pub obligation_id: String,
    pub authority_level: String,
    pub description: String,
    pub deadline: Option<String>,
    pub due_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ForeshadowInput {
    pub foreshadow_id: String,
    pub intent: String,
    pub status: String,
    pub recover_window_end_chapter: Option<String>,
    pub end_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ReaderDesireInput {
    pub desire_id: String,
    pub desire_type: String,
    pub status: String,
    pub expected_payoff_chapter: Option<String>,
    pub payoff_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RouteInput {
    pub volume_id: String,
    pub volume_no: Option<i64>,
    pub volume_title: Option<String>,
    pub plan_state: String,
    pub generated_revision: i64,
    pub remaining_chapter_capacity: Option<i64>,
    pub planning_pressure_json: String,
    pub task_status_json: String,
    pub route_plan_artifact_id: Option<String>,
    pub route_plan_version: Option<i64>,
    pub updated_at: String,
    pub route_payload_json: Option<String>,
    pub compiled_state_digest: Option<String>,
    pub current_state_digest: Option<String>,
    pub formal_stale: bool,
    pub source_unavailable: bool,
}

#[derive(Debug, Clone)]
pub struct TaskRealityInput {
    pub state_id: String,
    pub value_json: String,
    pub chapter_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ContextSample {
    pub capability: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: i64,
    pub recorded_at: String,
    pub model_run_id: String,
}

#[derive(Debug, Clone)]
pub struct CostSample {
    pub predicted_cost_cents: i64,
    pub actual_cost_cents: i64,
    pub recorded_at: String,
    pub model_run_id: String,
}

#[derive(Debug, Clone)]
pub struct QualitySample {
    pub sample_chapter_id: String,
    pub sample_chapter_no: i64,
    pub quality_vector_json: String,
    pub sampled_at: String,
}

#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub health_projection_ready: bool,
    pub factor_source_pages: BTreeMap<String, HealthSourcePage>,
    pub project_revision: i64,
    pub checked_at: String,
    pub current_chapter_no: i64,
    pub open_obligations: i64,
    pub overdue_obligations: i64,
    pub overdue_obligations_authority_ab: i64,
    pub overdue_obligations_authority_c: i64,
    pub unavailable_obligations: i64,
    pub obligations: Vec<ObligationInput>,
    pub open_foreshadows: i64,
    pub overdue_foreshadows: i64,
    pub stale_not_overdue_foreshadows: i64,
    pub unavailable_foreshadows: i64,
    pub foreshadows: Vec<ForeshadowInput>,
    pub routes: Vec<RouteInput>,
    pub formal_route_summary_queries: usize,
    pub formal_route_page_queries: usize,
    pub route_detail_rows_read: usize,
    pub route_payloads_read: usize,
    pub route_payloads_parsed: usize,
    pub route_scope_lookups: usize,
    pub route_artifact_lookups: usize,
    pub route_replan_lookups: usize,
    pub route_stale_queue_lookups: usize,
    pub task_state_batches_read: usize,
    pub task_states_read: usize,
    pub task_reality_rows_read: usize,
    pub route_scope_count: i64,
    pub route_stale_scope_count: i64,
    pub route_capacity_risk_scope_count: i64,
    pub route_capacity_attention_scope_count: i64,
    pub unavailable_routes: i64,
    pub task_reality: Vec<TaskRealityInput>,
    pub blocked_required_tasks: i64,
    pub blocked_optional_tasks: i64,
    pub task_summary_available: bool,
    pub open_reader_desires: i64,
    pub overdue_reader_desires: i64,
    pub unavailable_reader_desires: i64,
    pub reader_desires: Vec<ReaderDesireInput>,
    pub context_samples: Vec<ContextSample>,
    pub cost_samples: Vec<CostSample>,
    pub has_over_limit_reservation: bool,
    pub quality_samples: Vec<QualitySample>,
    pub has_formal_activity: bool,
}

pub struct HealthRepository;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthIssueCursor {
    pub source_revision: i64,
    pub severity_rank: i64,
    pub factor_rank: i64,
    pub due_sort: i64,
    pub issue_id: String,
}

#[derive(Debug, Clone)]
pub struct HealthIssueRow {
    pub issue_id: String,
    pub factor_code: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub location: String,
    pub due_chapter: Option<i64>,
    pub recommended_destination: String,
    pub source_kind: String,
    pub source_ref: String,
}

#[derive(Debug, Clone)]
pub struct HealthIssuePage {
    pub rows: Vec<HealthIssueRow>,
    pub total: i64,
    pub source_revision: i64,
    pub rows_read: usize,
    pub query_count: usize,
    pub json_scopes_processed: usize,
    pub next_cursor: Option<HealthIssueCursor>,
}

#[derive(Debug, Clone, Default)]
pub struct HealthSourceRefInput {
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, Default)]
pub struct HealthSourcePage {
    pub items: Vec<HealthSourceRefInput>,
    pub total: i64,
    pub source_revision: i64,
    pub next_cursor: Option<HealthIssueCursor>,
}

#[derive(Debug, Clone, Default)]
pub struct HealthProjectionRefreshStats {
    pub route_scopes_refreshed: usize,
    pub route_json_scopes_read: usize,
    pub task_rows_refreshed: usize,
}

struct FormalRouteSummary {
    route_scope_count: i64,
    route_stale_scope_count: i64,
    route_capacity_risk_scope_count: i64,
    route_capacity_attention_scope_count: i64,
    unavailable_routes: i64,
    blocked_required_tasks: i64,
    blocked_optional_tasks: i64,
    task_summary_available: bool,
}

struct HealthRouteScopeRow {
    volume_no: i64,
    title: String,
    volume_updated_at: String,
    plan_state: String,
    artifact_id: Option<String>,
    artifact_version: Option<i64>,
    canonical_hash: Option<String>,
    state_digest: Option<String>,
    projection_raw: Option<String>,
    projection_updated_at: Option<String>,
    artifact_type: Option<String>,
    artifact_status: Option<String>,
    artifact_hash: Option<String>,
    route_raw: Option<String>,
}

fn formal_route_summary(
    connection: &Connection,
    project_id: &str,
) -> Result<FormalRouteSummary, HealthRepositoryError> {
    connection
        .query_row(FORMAL_ROUTE_SUMMARY_SQL, [project_id], |row| {
            let route_scope_count: i64 = row.get(0)?;
            let route_stale_scope_count: i64 = row.get(1)?;
            let route_capacity_risk_scope_count: i64 = row.get(2)?;
            let route_capacity_attention_scope_count: i64 = row.get(3)?;
            let unavailable_routes: i64 = row.get(4)?;
            let blocked_required_tasks: i64 = row.get(5)?;
            let blocked_optional_tasks: i64 = row.get(6)?;
            let task_summary_missing: i64 = row.get(7)?;
            let task_class_conflicts: i64 = row.get(8)?;
            Ok(FormalRouteSummary {
                route_scope_count,
                route_stale_scope_count,
                route_capacity_risk_scope_count,
                route_capacity_attention_scope_count,
                unavailable_routes,
                blocked_required_tasks,
                blocked_optional_tasks,
                task_summary_available: task_summary_missing == 0 && task_class_conflicts == 0,
            })
        })
        .map_err(Into::into)
}

impl HealthRepository {
    /// Persistently invalidates one Health scope before an owner performs a
    /// post-commit B09 rebuild. This marker is intentionally separate from the
    /// later projection write so failure/restart cannot make stale Health look
    /// current.
    pub fn mark_scope_dirty(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        reason: &str,
        now: &str,
    ) -> Result<(), HealthRepositoryError> {
        let scope_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM volumes WHERE project_id=?1 AND volume_id=?2)",
            libsql_rusqlite::params![project_id, volume_id],
            |row| row.get(0),
        )?;
        if !scope_exists {
            return Err(HealthRepositoryError::Authority(
                "health dirty scope does not belong to project".into(),
            ));
        }
        let state_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM health_projection_state WHERE project_id=?1)",
            [project_id],
            |row| row.get(0),
        )?;
        if !state_exists {
            return Err(HealthRepositoryError::Authority(
                "health projection state missing".into(),
            ));
        }
        connection.execute(
            "INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at",
            libsql_rusqlite::params![project_id, volume_id, reason, now],
        )?;
        connection.execute(
            "UPDATE health_projection_state SET status='DIRTY',updated_at=?1 WHERE project_id=?2",
            libsql_rusqlite::params![now, project_id],
        )?;
        Ok(())
    }

    /// Persists the B09 prerequisite for a B08 chapter's post-commit planning
    /// rebuild. Generic Health refresh/open calls may process other scopes but
    /// must leave this scope unavailable until B09 reaches the committed rev.
    pub fn mark_b09_rebuild_pending(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        now: &str,
    ) -> Result<(), HealthRepositoryError> {
        Self::mark_scope_dirty(
            connection,
            project_id,
            volume_id,
            "B08_POST_COMMIT_B09_REBUILD",
            now,
        )?;
        let required_revision = Self::current_project_revision(connection, project_id)?;
        connection.execute(
            "INSERT INTO health_b09_rebuild_pending(project_id,volume_id,required_revision,reason,marked_at)
             VALUES (?1,?2,?3,'B08_POST_COMMIT_B09_REBUILD',?4)
             ON CONFLICT(project_id,volume_id) DO UPDATE SET
               required_revision=MAX(health_b09_rebuild_pending.required_revision,excluded.required_revision),
               reason=excluded.reason,marked_at=excluded.marked_at",
            libsql_rusqlite::params![project_id, volume_id, required_revision, now],
        )?;
        Ok(())
    }

    /// Called only by the B09 owner after its planning projection has been
    /// persisted in the same transaction as the subsequent Health refresh.
    pub fn complete_b09_rebuild_pending(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
    ) -> Result<(), HealthRepositoryError> {
        let pending_revision: Option<i64> = connection
            .query_row(
                "SELECT required_revision FROM health_b09_rebuild_pending WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(pending_revision) = pending_revision else {
            return Ok(());
        };
        let projection_revision: Option<i64> = connection
            .query_row(
                "SELECT projection_revision FROM volume_planning_state_projection WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .optional()?;
        if projection_revision.is_none_or(|revision| revision < pending_revision) {
            return Err(HealthRepositoryError::Authority(
                "B09 planning projection has not reached the required revision".into(),
            ));
        }
        connection.execute(
            "DELETE FROM health_b09_rebuild_pending WHERE project_id=?1 AND volume_id=?2 AND required_revision<=?3",
            libsql_rusqlite::params![project_id, volume_id, projection_revision],
        )?;
        Ok(())
    }

    pub fn current_project_revision(
        connection: &Connection,
        project_id: &str,
    ) -> Result<i64, HealthRepositoryError> {
        connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn projection_is_current(
        connection: &Connection,
        project_id: &str,
    ) -> Result<bool, HealthRepositoryError> {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM health_projection_state s JOIN project_meta p ON p.project_id=s.project_id WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=p.revision AND NOT EXISTS(SELECT 1 FROM health_projection_dirty d WHERE d.project_id=s.project_id))",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn current_chapter_no(
        connection: &Connection,
        project_id: &str,
    ) -> Result<i64, HealthRepositoryError> {
        connection
            .query_row(
                "SELECT COALESCE(MAX(chapter_no),0) FROM chapters WHERE project_id=?1 AND status='COMMITTED'",
                [project_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn ensure_current(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<HealthProjectionRefreshStats, HealthRepositoryError> {
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let state: Option<(String, i64)> = connection
            .query_row(
                "SELECT status,source_revision FROM health_projection_state WHERE project_id=?1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let dirty_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM health_projection_dirty WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        if state.as_ref().is_some_and(|(status, source_revision)| {
            status == "READY" && *source_revision == revision && dirty_count == 0
        }) {
            return Ok(HealthProjectionRefreshStats::default());
        }
        if state.is_none() {
            return Err(HealthRepositoryError::Authority(
                "health projection state missing".into(),
            ));
        }
        if dirty_count == 0 {
            connection.execute(
                "INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
                 SELECT ?1,volume_id,'EXPLICIT_REBUILD',?2 FROM volume_plan_index WHERE project_id=?1
                 ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at",
                libsql_rusqlite::params![project_id, now],
            )?;
            connection.execute(
                "INSERT INTO health_projection_dirty(project_id,volume_id,reason,marked_at)
                 SELECT h.project_id,CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END,'EXPLICIT_REBUILD',?2
                 FROM canonical_heads h JOIN volumes v ON v.project_id=h.project_id
                   AND v.volume_id=CASE WHEN json_valid(h.scope_key)=1 THEN COALESCE(json_extract(h.scope_key,'$.volumeId'),h.scope_key) ELSE h.scope_key END
                 WHERE h.project_id=?1 AND h.artifact_type='VolumePlan'
                 ON CONFLICT(project_id,volume_id) DO UPDATE SET reason=excluded.reason,marked_at=excluded.marked_at",
                libsql_rusqlite::params![project_id, now],
            )?;
        }
        Self::flush_dirty(connection, project_id, now)
    }

    /// Rebuilds only scopes invalidated by formal writes. Call from the
    /// CommitService transaction after the project revision has advanced.
    pub fn flush_dirty(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<HealthProjectionRefreshStats, HealthRepositoryError> {
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let dirty = {
            let mut statement = connection.prepare(
                "SELECT d.volume_id FROM health_projection_dirty d
                 WHERE d.project_id=?1 AND NOT EXISTS(
                   SELECT 1 FROM health_b09_rebuild_pending p
                   LEFT JOIN volume_planning_state_projection vp
                     ON vp.project_id=p.project_id AND vp.volume_id=p.volume_id
                   WHERE p.project_id=d.project_id AND p.volume_id=d.volume_id
                     AND COALESCE(vp.projection_revision,-1)<p.required_revision
                 )
                 ORDER BY d.volume_id",
            )?;
            let rows = statement.query_map([project_id], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut stats = HealthProjectionRefreshStats::default();
        for volume_id in dirty {
            stats.route_json_scopes_read +=
                Self::refresh_route_scope(connection, project_id, &volume_id, revision, now)?;
            stats.route_scopes_refreshed += 1;
            stats.task_rows_refreshed += connection.query_row(
                "SELECT COUNT(*) FROM health_task_scope_index WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get::<_, i64>(0),
            )? as usize;
            connection.execute(
                "DELETE FROM health_projection_dirty WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
            )?;
        }
        let remaining: i64 = connection.query_row(
            "SELECT COUNT(*) FROM health_projection_dirty WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let b09_pending: i64 = connection.query_row(
            "SELECT COUNT(*) FROM health_b09_rebuild_pending p
             LEFT JOIN volume_planning_state_projection vp
               ON vp.project_id=p.project_id AND vp.volume_id=p.volume_id
             WHERE p.project_id=?1 AND COALESCE(vp.projection_revision,-1)<p.required_revision",
            [project_id],
            |row| row.get(0),
        )?;
        if b09_pending != 0 {
            return Ok(stats);
        }
        if remaining != 0 {
            return Err(HealthRepositoryError::Authority(
                "health projection still has dirty scopes".into(),
            ));
        }
        connection.execute(
            "INSERT INTO health_projection_state(project_id,status,source_revision,updated_at)
             VALUES (?1,'READY',?2,?3)
             ON CONFLICT(project_id) DO UPDATE SET status='READY',source_revision=excluded.source_revision,updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, revision, now],
        )?;
        Ok(stats)
    }

    fn refresh_route_scope(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        source_revision: i64,
        _now: &str,
    ) -> Result<usize, HealthRepositoryError> {
        connection.execute(
            "DELETE FROM health_issue_projection WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )?;
        connection.execute(
            "DELETE FROM health_task_scope_index WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )?;
        connection.execute(
            "DELETE FROM health_route_scope_projection WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )?;

        let row: Option<HealthRouteScopeRow> = connection
            .query_row(
                HEALTH_ROUTE_SCOPE_SQL,
                libsql_rusqlite::params![project_id, volume_id],
                |row| {
                    Ok(HealthRouteScopeRow {
                        volume_no: row.get(0)?,
                        title: row.get(1)?,
                        volume_updated_at: row.get(2)?,
                        plan_state: row.get(3)?,
                        artifact_id: row.get(4)?,
                        artifact_version: row.get(5)?,
                        canonical_hash: row.get(6)?,
                        state_digest: row.get(8)?,
                        projection_raw: row.get(9)?,
                        projection_updated_at: row.get(10)?,
                        artifact_type: row.get(11)?,
                        artifact_status: row.get(12)?,
                        artifact_hash: row.get(13)?,
                        route_raw: row.get(14)?,
                    })
                },
            )
            .optional()?;
        let Some(HealthRouteScopeRow {
            volume_no,
            title,
            volume_updated_at,
            plan_state,
            artifact_id,
            artifact_version,
            canonical_hash,
            state_digest,
            projection_raw,
            projection_updated_at,
            artifact_type,
            artifact_status,
            artifact_hash,
            route_raw,
        }) = row
        else {
            return Ok(0);
        };

        let projection = projection_raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        let route = match (artifact_type.as_deref(), route_raw.as_deref()) {
            // FR-030: an untyped route is never interpreted; it surfaces as this
            // scope's unavailable source instead of failing every other scope.
            (Some("VolumePlan"), Some(raw)) => {
                crate::repositories::narrative::parse_formal_payload("VolumePlan", raw).ok()
            }
            (_, Some(raw)) => serde_json::from_str::<Value>(raw).ok(),
            (_, None) => None,
        };
        let route_payload_valid = route.is_some();
        let projection_valid = projection.is_some();
        let mut task_summary_available = false;
        let mut tasks = BTreeMap::<String, (String, bool, bool)>::new();
        if let Some(route_tasks) = route
            .as_ref()
            .and_then(|value| value.get("tasks"))
            .and_then(Value::as_array)
        {
            task_summary_available = true;
            for task in route_tasks {
                let Some(task_id) = task
                    .get("taskId")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    task_summary_available = false;
                    continue;
                };
                let class = task
                    .get("class")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let Some((required_for_capacity, health_required)) = (match class {
                    "REQUIRED" => Some((true, true)),
                    "CLIMAX" | "RESERVED" => Some((false, true)),
                    "OPTIONAL" | "FLEXIBLE" => Some((false, false)),
                    _ => None,
                }) else {
                    task_summary_available = false;
                    continue;
                };
                match tasks.entry(task_id.to_string()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert((class.to_string(), required_for_capacity, health_required));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let value = entry.get_mut();
                        if value.0 != class {
                            task_summary_available = false;
                            value.1 |= required_for_capacity;
                            value.2 |= health_required;
                        }
                    }
                }
            }
        }

        let task_ids_json = serde_json::to_string(&tasks.keys().collect::<Vec<_>>())?;
        let task_states = {
            let mut statement = connection.prepare(
                "SELECT sd.state_id,sd.value_json FROM state_deltas sd JOIN json_each(?2) requested ON requested.value=sd.state_id WHERE sd.project_id=?1 AND sd.state_type='volume.task.progress'",
            )?;
            let rows = statement
                .query_map(libsql_rusqlite::params![project_id, task_ids_json], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
            rows.collect::<Result<BTreeMap<_, _>, _>>()?
        };
        let mut blocked_required = 0_i64;
        let mut blocked_optional = 0_i64;
        let mut required_tasks_remaining = 0_i64;
        let mut required_tasks_remaining_known = true;
        let mut blocked_tasks = Vec::<(String, bool)>::new();
        for (task_id, (_, required_for_capacity, health_required)) in &tasks {
            let status = match task_states.get(task_id) {
                None => Some("NOT_STARTED".to_string()),
                Some(raw) => serde_json::from_str::<Value>(raw).ok().and_then(|value| {
                    let embedded_task_id = value.get("taskId").and_then(Value::as_str);
                    if embedded_task_id.is_some_and(|value| value != task_id) {
                        return None;
                    }
                    value
                        .get("status")
                        .and_then(Value::as_str)
                        .filter(|status| {
                            matches!(
                                *status,
                                "NOT_STARTED" | "ACTIVE" | "BLOCKED" | "SATISFIED" | "INVALIDATED"
                            )
                        })
                        .map(ToOwned::to_owned)
                }),
            };
            let Some(status) = status else {
                task_summary_available = false;
                if *required_for_capacity {
                    required_tasks_remaining_known = false;
                }
                continue;
            };
            if *required_for_capacity && status != "SATISFIED" {
                required_tasks_remaining += 1;
            }
            if status == "BLOCKED" {
                blocked_tasks.push((task_id.clone(), *health_required));
                if *health_required {
                    blocked_required += 1;
                } else {
                    blocked_optional += 1;
                }
            }
        }
        let required_tasks_remaining =
            required_tasks_remaining_known.then_some(required_tasks_remaining);
        let source_unavailable = !projection_valid
            || !route_payload_valid
            || artifact_id.is_none()
            || artifact_version.is_none()
            || artifact_type.as_deref() != Some("VolumePlan")
            || artifact_status.as_deref() != Some("COMMITTED")
            || canonical_hash
                .as_deref()
                .is_some_and(|expected| artifact_hash.as_deref() != Some(expected));

        let capacity = projection.as_ref().and_then(|value| {
            value
                .pointer("/planningPressure/remainingChapterCapacity")
                .or_else(|| value.get("remainingChapterCapacity"))
                .and_then(Value::as_i64)
        });
        let required = required_tasks_remaining;
        let artifact_id_ref = artifact_id.as_deref().unwrap_or_default();
        let pending_stale: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM stale_queue WHERE project_id=?1 AND processed=0 AND target_ref IN (?2,?3))",
            libsql_rusqlite::params![project_id, volume_id, artifact_id_ref],
            |row| row.get(0),
        )?;
        let compiled_digest = route
            .as_ref()
            .and_then(|value| value.get("compiledFromStateDigest"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                let (Some(artifact_id), Some(version)) = (artifact_id.as_deref(), artifact_version)
                else {
                    return None;
                };
                let raw: Option<String> = connection
                    .query_row(
                        "SELECT input_refs_json FROM volume_route_replans WHERE project_id=?1 AND volume_id=?2 AND new_artifact_id=?3 AND new_version=?4 ORDER BY created_at DESC,replan_id DESC LIMIT 1",
                        libsql_rusqlite::params![project_id,volume_id,artifact_id,version],
                        |row| row.get(0),
                    )
                    .optional()
                    .ok()
                    .flatten();
                raw.and_then(|value| serde_json::from_str::<Value>(&value).ok())
                    .and_then(|value| value.pointer("/actualReality/stateDigest").or_else(|| value.pointer("/compiledInput/actualReality/stateDigest")).and_then(Value::as_str).map(ToOwned::to_owned))
            });
        let current_digest = state_digest.clone().or_else(|| {
            projection
                .as_ref()
                .and_then(|value| value.get("stateDigest"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
        let stale = pending_stale
            || matches!((compiled_digest.as_deref(), current_digest.as_deref()), (Some(a),Some(b)) if a!=b)
            || matches!(plan_state.as_str(), "STALE" | "PLAN_STALE");
        let capacity_risk = capacity == Some(0) && required.is_some_and(|value| value > 0);
        let capacity_attention = capacity.is_some_and(|value| (1..=8).contains(&value))
            && required.is_some_and(|value| value > 0);
        let display_location = if title.trim().is_empty() {
            format!("第 {volume_no} 卷")
        } else {
            format!("第 {volume_no} 卷 · {}", title.trim())
        };

        connection.execute(
            "INSERT INTO health_route_scope_projection(project_id,volume_id,volume_no,volume_title,plan_state,is_stale,is_unavailable,capacity,required_tasks_remaining,blocked_required_tasks,blocked_optional_tasks,task_summary_available,source_revision,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            libsql_rusqlite::params![project_id,volume_id,volume_no,title,plan_state,i64::from(stale),i64::from(source_unavailable),capacity,required,blocked_required,blocked_optional,i64::from(task_summary_available),source_revision,projection_updated_at.unwrap_or(volume_updated_at)],
        )?;

        if source_unavailable || stale || capacity_risk || capacity_attention {
            let severity = if stale || capacity_risk {
                "risk"
            } else {
                "attention"
            };
            let (title, detail, location) = if source_unavailable {
                (
                    "路线来源暂不可用",
                    "关联分卷记录暂时无法读取，未将其误判为路线风险。",
                    "相关分卷".to_string(),
                )
            } else if stale {
                (
                    "规划路线需要重新核对",
                    "当前规划版本落后于项目最新变更，需要在卷工作区重新核对。",
                    display_location.clone(),
                )
            } else if capacity_risk {
                (
                    "卷内安排需要留出空间",
                    "剩余章节容量已用尽，但仍有必需任务未完成。",
                    display_location.clone(),
                )
            } else {
                (
                    "卷内安排需要留出空间",
                    "剩余章节容量较少，仍有必需任务需要安排。",
                    display_location.clone(),
                )
            };
            Self::insert_health_issue(
                connection,
                project_id,
                &format!("health:route:{volume_id}"),
                "ROUTE_DRIFT",
                severity,
                title,
                detail,
                &location,
                capacity,
                "卷工作区",
                "volume-plan",
                volume_id,
                if severity == "risk" { 0 } else { 1 },
                2,
                capacity.unwrap_or(i64::MAX),
                volume_id,
                source_revision,
            )?;
        }

        // Task health has one source of truth: the current formal route's task
        // classes plus state_deltas. Cached B09 healthBlocked* and blockedTaskIds
        // are rebuildable summaries and never override a newer formal status.
        for (task_id, (class, _, _)) in &tasks {
            connection.execute(
                "INSERT OR IGNORE INTO health_task_scope_index(project_id,task_id,volume_id,task_class) VALUES (?1,?2,?3,?4)",
                libsql_rusqlite::params![project_id,task_id,volume_id,class],
            )?;
        }
        for (task_id, required) in blocked_tasks {
            let severity = if required { "risk" } else { "attention" };
            Self::insert_health_issue(
                connection,
                project_id,
                &format!("health:task:{task_id}"),
                "TASK_BLOCKAGE",
                severity,
                if required {
                    "必需任务被阻塞"
                } else {
                    "可选任务被阻塞"
                },
                "请回到卷工作区核对任务条件与现实进度。",
                &format!("{} 的卷内任务", display_location),
                None,
                "卷工作区",
                "task",
                &task_id,
                if required { 0 } else { 1 },
                3,
                i64::MAX,
                volume_id,
                source_revision,
            )?;
        }
        Ok(usize::from(route_raw.is_some()))
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_health_issue(
        connection: &Connection,
        project_id: &str,
        issue_id: &str,
        factor_code: &str,
        severity: &str,
        title: &str,
        detail: &str,
        location: &str,
        due_chapter: Option<i64>,
        recommended_destination: &str,
        source_kind: &str,
        source_ref: &str,
        severity_rank: i64,
        factor_rank: i64,
        due_sort: i64,
        volume_id: &str,
        source_revision: i64,
    ) -> Result<(), HealthRepositoryError> {
        connection.execute(
            "INSERT INTO health_issue_projection(project_id,issue_id,factor_code,severity,title,detail,location,due_chapter,recommended_destination,source_kind,source_ref,severity_rank,factor_rank,due_sort,volume_id,source_revision)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            libsql_rusqlite::params![project_id,issue_id,factor_code,severity,title,detail,location,due_chapter,recommended_destination,source_kind,source_ref,severity_rank,factor_rank,due_sort,volume_id,source_revision],
        )?;
        Ok(())
    }

    pub fn issue_page(
        connection: &Connection,
        project_id: &str,
        current_chapter_no: i64,
        cursor: Option<&HealthIssueCursor>,
        limit: usize,
    ) -> Result<HealthIssuePage, HealthRepositoryError> {
        Self::issue_page_filtered(
            connection,
            project_id,
            current_chapter_no,
            cursor,
            limit,
            None,
        )
    }

    pub fn factor_source_page(
        connection: &Connection,
        project_id: &str,
        current_chapter_no: i64,
        factor_code: &str,
        limit: usize,
    ) -> Result<HealthSourcePage, HealthRepositoryError> {
        Self::factor_source_page_after(
            connection,
            project_id,
            current_chapter_no,
            factor_code,
            None,
            limit,
        )
    }

    pub fn factor_source_page_after(
        connection: &Connection,
        project_id: &str,
        current_chapter_no: i64,
        factor_code: &str,
        cursor: Option<&HealthIssueCursor>,
        limit: usize,
    ) -> Result<HealthSourcePage, HealthRepositoryError> {
        let page = Self::issue_page_filtered(
            connection,
            project_id,
            current_chapter_no,
            cursor,
            limit,
            Some(factor_code),
        )?;
        Ok(HealthSourcePage {
            items: page
                .rows
                .into_iter()
                .map(|row| HealthSourceRefInput {
                    kind: row.source_kind,
                    reference: row.source_ref,
                })
                .collect(),
            total: page.total,
            source_revision: page.source_revision,
            next_cursor: page.next_cursor,
        })
    }

    fn issue_page_filtered(
        connection: &Connection,
        project_id: &str,
        current_chapter_no: i64,
        cursor: Option<&HealthIssueCursor>,
        limit: usize,
        factor_code: Option<&str>,
    ) -> Result<HealthIssuePage, HealthRepositoryError> {
        let limit = limit.clamp(1, 54) as i64;
        let source_revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        if cursor.is_some_and(|value| value.source_revision != source_revision) {
            return Err(HealthRepositoryError::CursorStale);
        }
        let (severity_rank, factor_rank, due_sort, issue_id) =
            cursor.map_or((-1, -1, i64::MIN, String::new()), |value| {
                (
                    value.severity_rank,
                    value.factor_rank,
                    value.due_sort,
                    value.issue_id.clone(),
                )
            });
        let factor_code = factor_code.map(ToOwned::to_owned);
        let total = connection.query_row(
            HEALTH_ISSUE_TOTAL_SQL,
            libsql_rusqlite::params![project_id, current_chapter_no, factor_code],
            |row| row.get::<_, i64>(0),
        )?;
        let mut statement = connection.prepare(HEALTH_ISSUE_PAGE_SQL)?;
        let rows = statement.query_map(
            libsql_rusqlite::params![
                project_id,
                current_chapter_no,
                severity_rank,
                factor_rank,
                due_sort,
                issue_id,
                limit,
                factor_code
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                ))
            },
        )?;
        let mut page = HealthIssuePage {
            rows: Vec::new(),
            total,
            source_revision,
            rows_read: 0,
            query_count: 2,
            json_scopes_processed: 0,
            next_cursor: None,
        };
        for row in rows {
            let (
                issue_id,
                factor_code,
                severity,
                title,
                detail,
                location,
                due_chapter,
                recommended_destination,
                source_kind,
                source_ref,
                severity_rank,
                factor_rank,
                due_sort,
            ) = row?;
            page.rows_read += 1;
            page.next_cursor = Some(HealthIssueCursor {
                source_revision,
                severity_rank,
                factor_rank,
                due_sort,
                issue_id: issue_id.clone(),
            });
            page.rows.push(HealthIssueRow {
                issue_id,
                factor_code,
                severity,
                title,
                detail,
                location,
                due_chapter,
                recommended_destination,
                source_kind,
                source_ref,
            });
        }
        if page.rows_read < limit as usize {
            page.next_cursor = None;
        }
        Ok(page)
    }

    pub fn load(
        connection: &Connection,
        project_id: &str,
    ) -> Result<HealthSnapshot, HealthRepositoryError> {
        let project_revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let health_projection_ready: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM health_projection_state s
                WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=?2
                  AND NOT EXISTS(SELECT 1 FROM health_projection_dirty d WHERE d.project_id=s.project_id)
            )",
            libsql_rusqlite::params![project_id, project_revision],
            |row| row.get(0),
        )?;
        let project_updated_at: String = connection.query_row(
            "SELECT updated_at FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let current_chapter_no: i64 = connection.query_row(
            "SELECT COALESCE(MAX(chapter_no),0) FROM chapters WHERE project_id=?1 AND status='COMMITTED'",
            [project_id],
            |row| row.get(0),
        )?;
        let checked_at: String = connection
            .query_row(HEALTH_SOURCE_TIME_SQL, [project_id], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten()
            .unwrap_or(project_updated_at);

        let (
            open_obligations,
            overdue_obligations,
            overdue_obligations_authority_ab,
            overdue_obligations_authority_c,
            unavailable_obligations,
        ): (i64, i64, i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 AND o.authority_level IN ('A','B') THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 AND o.authority_level='C' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM narrative_obligations o
             LEFT JOIN chapters due
               ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
             WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        let obligations = {
            let mut statement = connection.prepare(
                "SELECT o.obligation_id, o.authority_level, o.description,
                        o.must_resolve_before_chapter, o.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM narrative_obligations o
                 LEFT JOIN chapters due
                   ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
                 WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), o.obligation_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ObligationInput {
                        obligation_id: row.get(0)?,
                        authority_level: row.get(1)?,
                        description: row.get(2)?,
                        deadline: row.get(3)?,
                        due_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let (
            open_foreshadows,
            overdue_foreshadows,
            stale_not_overdue_foreshadows,
            unavailable_foreshadows,
        ): (i64, i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN f.status='STALE' AND due.chapter_no IS NOT NULL AND due.chapter_no > ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM foreshadows f
             LEFT JOIN chapters due
               ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
             WHERE f.project_id=?1 AND f.status IN ('PLANTED','REINFORCED','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let foreshadows = {
            let mut statement = connection.prepare(
                "SELECT f.foreshadow_id, f.intent, f.status,
                        f.recover_window_end_chapter, f.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM foreshadows f
                 LEFT JOIN chapters due
                   ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
                 WHERE f.project_id=?1 AND f.status IN ('PLANTED','REINFORCED','STALE')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), f.foreshadow_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ForeshadowInput {
                        foreshadow_id: row.get(0)?,
                        intent: row.get(1)?,
                        status: row.get(2)?,
                        recover_window_end_chapter: row.get(3)?,
                        end_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let FormalRouteSummary {
            route_scope_count,
            mut route_stale_scope_count,
            mut route_capacity_risk_scope_count,
            mut route_capacity_attention_scope_count,
            mut unavailable_routes,
            mut blocked_required_tasks,
            mut blocked_optional_tasks,
            mut task_summary_available,
        } = formal_route_summary(connection, project_id)?;
        if !health_projection_ready {
            route_stale_scope_count = 0;
            route_capacity_risk_scope_count = 0;
            route_capacity_attention_scope_count = 0;
            unavailable_routes = route_scope_count;
            blocked_required_tasks = 0;
            blocked_optional_tasks = 0;
            task_summary_available = false;
        }
        let factor_source_pages = [
            "OBLIGATION_OVERDUE",
            "FORESHADOW_AGING",
            "ROUTE_DRIFT",
            "TASK_BLOCKAGE",
            "READER_DESIRE",
        ]
        .into_iter()
        .map(|factor_code| {
            Ok((
                factor_code.to_string(),
                Self::factor_source_page(
                    connection,
                    project_id,
                    current_chapter_no,
                    factor_code,
                    50,
                )?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, HealthRepositoryError>>()?;

        let (open_reader_desires, overdue_reader_desires, unavailable_reader_desires): (i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM reader_desires d
             LEFT JOIN chapters due
               ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
             WHERE d.project_id=?1 AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let reader_desires = {
            let mut statement = connection.prepare(
                "SELECT d.desire_id, d.desire_type, d.status,
                        d.expected_payoff_chapter, d.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM reader_desires d
                 LEFT JOIN chapters due
                   ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
                 WHERE d.project_id=?1 AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), d.desire_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ReaderDesireInput {
                        desire_id: row.get(0)?,
                        desire_type: row.get(1)?,
                        status: row.get(2)?,
                        expected_payoff_chapter: row.get(3)?,
                        payoff_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let context_samples = Self::context_samples(connection, project_id)?;
        let cost_samples = Self::cost_samples(connection, project_id)?;
        let has_over_limit_reservation: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM budget_reservations WHERE project_id=?1 AND status='OVER_LIMIT')",
            [project_id],
            |row| row.get(0),
        )?;
        let quality_samples = {
            let mut statement = connection.prepare(
                "SELECT sample_chapter_id, sample_chapter_no, quality_vector_json, sampled_at
                 FROM quality_trend_projection
                 WHERE project_id=?1
                 ORDER BY sample_chapter_no DESC, sample_chapter_id DESC
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, TREND_ROW_BOUND],
                |row| {
                    Ok(QualitySample {
                        sample_chapter_id: row.get(0)?,
                        sample_chapter_no: row.get(1)?,
                        quality_vector_json: row.get(2)?,
                        sampled_at: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let has_formal_activity = current_chapter_no > 0
            || open_obligations > 0
            || open_foreshadows > 0
            || route_scope_count > 0
            || open_reader_desires > 0
            || !context_samples.is_empty()
            || !cost_samples.is_empty()
            || !quality_samples.is_empty();

        Ok(HealthSnapshot {
            health_projection_ready,
            factor_source_pages,
            project_revision,
            checked_at,
            current_chapter_no,
            open_obligations,
            overdue_obligations,
            overdue_obligations_authority_ab,
            overdue_obligations_authority_c,
            unavailable_obligations,
            obligations,
            open_foreshadows,
            overdue_foreshadows,
            stale_not_overdue_foreshadows,
            unavailable_foreshadows,
            foreshadows,
            routes: Vec::new(),
            formal_route_summary_queries: 1,
            formal_route_page_queries: 0,
            route_detail_rows_read: 0,
            route_payloads_read: 0,
            route_payloads_parsed: 0,
            route_scope_lookups: 0,
            route_artifact_lookups: 0,
            route_replan_lookups: 0,
            route_stale_queue_lookups: 0,
            task_state_batches_read: 0,
            task_states_read: 0,
            task_reality_rows_read: 0,
            route_scope_count,
            route_stale_scope_count,
            route_capacity_risk_scope_count,
            route_capacity_attention_scope_count,
            unavailable_routes,
            task_reality: Vec::new(),
            blocked_required_tasks,
            blocked_optional_tasks,
            task_summary_available: task_summary_available && health_projection_ready,
            open_reader_desires,
            overdue_reader_desires,
            unavailable_reader_desires,
            reader_desires,
            context_samples,
            cost_samples,
            has_over_limit_reservation,
            quality_samples,
            has_formal_activity,
        })
    }

    fn context_samples(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<ContextSample>, HealthRepositoryError> {
        let latest: Option<(String, String, String)> = connection
            .query_row(
                "SELECT capability, provider, model
                 FROM model_runs
                 WHERE project_id=?1 AND status='COMPLETED'
                 ORDER BY updated_at DESC, model_run_id DESC
                 LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((capability, provider, model)) = latest else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT r.capability, r.provider, r.model, u.input_tokens, u.recorded_at, r.model_run_id
             FROM model_runs r JOIN model_usage u ON u.project_id=r.project_id AND u.model_run_id=r.model_run_id
             WHERE r.project_id=?1 AND r.status='COMPLETED' AND r.capability=?2 AND r.provider=?3 AND r.model=?4
             ORDER BY r.updated_at DESC, r.model_run_id DESC
             LIMIT 16",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, capability, provider, model],
            |row| {
                Ok(ContextSample {
                    capability: row.get(0)?,
                    provider: row.get(1)?,
                    model: row.get(2)?,
                    input_tokens: row.get(3)?,
                    recorded_at: row.get(4)?,
                    model_run_id: row.get(5)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn cost_samples(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<CostSample>, HealthRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT r.predicted_cost_cents, u.cost_cents, u.recorded_at, r.model_run_id
             FROM model_runs r JOIN model_usage u ON u.project_id=r.project_id AND u.model_run_id=r.model_run_id
             WHERE r.project_id=?1 AND r.status='COMPLETED' AND r.predicted_cost_cents > 0
             ORDER BY r.updated_at DESC, r.model_run_id DESC
             LIMIT 8",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(CostSample {
                predicted_cost_cents: row.get(0)?,
                actual_cost_cents: row.get(1)?,
                recorded_at: row.get(2)?,
                model_run_id: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
