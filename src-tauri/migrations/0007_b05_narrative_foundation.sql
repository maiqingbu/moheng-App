-- B05: additive Narrative Runtime foundation. This migration is applied once
-- by the migration history gate; 0001-0006 remain immutable.

ALTER TABLE narrative_obligations ADD COLUMN source_event_id TEXT;
ALTER TABLE narrative_obligations ADD COLUMN resume_condition_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE canon_events ADD COLUMN event_ordinal INTEGER NOT NULL DEFAULT 0;
ALTER TABLE canon_events ADD COLUMN story_time_end TEXT;
ALTER TABLE canon_events ADD COLUMN source_artifact_id TEXT;
ALTER TABLE canon_events ADD COLUMN source_version INTEGER;

ALTER TABLE planning_state_ledger_index ADD COLUMN route_plan_artifact_id TEXT;
ALTER TABLE planning_state_ledger_index ADD COLUMN route_plan_version INTEGER;
ALTER TABLE planning_state_ledger_index ADD COLUMN task_status_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE planning_state_ledger_index ADD COLUMN active_task_ids_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE planning_state_ledger_index ADD COLUMN blocked_task_ids_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE planning_state_ledger_index ADD COLUMN remaining_chapter_capacity INTEGER;
ALTER TABLE planning_state_ledger_index ADD COLUMN planning_pressure_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE planning_state_ledger_index ADD COLUMN state_digest TEXT;
ALTER TABLE planning_state_ledger_index ADD COLUMN generated_revision INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_canon_events_subject_type_ordinal
  ON canon_events(project_id, subject_id, event_type, event_ordinal);
CREATE INDEX IF NOT EXISTS idx_dependency_edges_entity_type_scope
  ON dependency_edges(project_id, entity_id, dependency_type, scope);
